//! Explicitly discard local edits while keeping the saved head and backup unchanged.
use super::archives::workspace_identity;
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceDiscardState {
    Pending,
    ReconfirmationRequired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDiscardReport {
    pub variant_id: String,
    pub head_snapshot_id: String,
    #[serde(default)]
    pub pending: bool,
    #[serde(default)]
    pub reconfirmation_required: bool,
    pub cleanup_pending: bool,
    pub warnings: Vec<String>,
}

impl WorkspaceDiscardReport {
    fn new(variant_id: &str, head_snapshot_id: &str) -> Self {
        Self {
            variant_id: variant_id.into(),
            head_snapshot_id: head_snapshot_id.into(),
            pending: false,
            reconfirmation_required: false,
            cleanup_pending: false,
            warnings: vec![],
        }
    }
}

struct PendingDiscard {
    variant_id: String,
    expected_head: String,
    fingerprint: Option<String>,
}

#[cfg(windows)]
fn is_sharing_violation(error: &CoreError) -> bool {
    use windows_sys::Win32::Foundation::{ERROR_LOCK_VIOLATION, ERROR_SHARING_VIOLATION};

    matches!(
        error,
        CoreError::Io(error)
            if error.raw_os_error().is_some_and(|code| {
                code == ERROR_SHARING_VIOLATION as i32 || code == ERROR_LOCK_VIOLATION as i32
            })
    )
}

#[cfg(not(windows))]
fn is_sharing_violation(_error: &CoreError) -> bool {
    false
}

#[derive(Serialize, Deserialize)]
struct DiscardJournal {
    version: u32,
    id: String,
    variant_id: String,
    workspace: String,
    head: String,
    before: Vec<ManifestEntry>,
    target: Vec<ManifestEntry>,
}

fn journal(root: &Path, directory: &Path) -> Result<(DiscardJournal, String)> {
    ensure_real_directory(directory)?;
    let path = directory.join("discard.json");
    let metadata = fs::symlink_metadata(&path)?;
    #[cfg(windows)]
    let linked = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let linked = metadata.file_type().is_symlink();
    if linked || !metadata.is_file() || metadata.len() > 32 * 1024 * 1024 {
        return Err(CoreError::Inconsistent(
            "Journal d’abandon invalide.".into(),
        ));
    }
    let bytes = fs::read(path)?;
    let value: DiscardJournal = serde_json::from_slice(&bytes)?;
    if value.version != 1
        || uuid::Uuid::parse_str(&value.id).is_err()
        || directory.parent() != Some(root.join("staging").as_path())
        || directory.file_name().and_then(|v| v.to_str()) != Some(&format!("discard-{}", value.id))
        || value.workspace.split('/').count() != 3
        || !value.workspace.starts_with("workspaces/")
        || !value.workspace.ends_with(&format!("/{}", value.variant_id))
    {
        return Err(CoreError::Inconsistent(
            "Journal d’abandon incohérent ; fichiers préservés.".into(),
        ));
    }
    safe_relative(&value.workspace)?;
    for entry in value.before.iter().chain(&value.target) {
        safe_relative(&entry.path)?;
        if !matches!(entry.kind.as_str(), "file" | "directory") {
            return Err(CoreError::Inconsistent(
                "Type de fichier du journal invalide.".into(),
            ));
        }
    }
    Ok((value, blake3::hash(&bytes).to_hex().to_string()))
}

fn receipt(db: &Connection, id: &str) -> Result<Option<String>> {
    Ok(db
        .query_row(
            "SELECT journal_hash FROM workspace_discards WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()?)
}

fn empty_checkpoint(directory: &Path) -> Result<bool> {
    ensure_real_directory(directory)?;
    Ok(fs::read_dir(directory)?.next().is_none())
}

fn path_exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn ensure_no_discard(root: &Path, db: &Connection) -> Result<()> {
    for item in fs::read_dir(root.join("staging"))? {
        let path = item?.path();
        if !path
            .file_name()
            .is_some_and(|v| v.to_string_lossy().starts_with("discard-"))
        {
            continue;
        }
        // The journal may already have been removed when removing the empty directory failed.
        if empty_checkpoint(&path)? {
            continue;
        }
        let (value, hash) = journal(root, &path)?;
        if receipt(db, &value.id)?.as_deref() != Some(&hash) {
            return Err(CoreError::Busy("Un abandon interrompu attend sa récupération. Rouvrez la bibliothèque ; les fichiers ont été préservés.".into()));
        }
    }
    Ok(())
}

impl Library {
    pub fn discard_status(&self, variant_id: &str) -> Result<Option<WorkspaceDiscardState>> {
        let state = self
            .db()?
            .query_row(
                "SELECT reconfirmation_required FROM pending_workspace_discards WHERE variant_id=?1",
                [variant_id],
                |row| row.get::<_, bool>(0),
            )
            .optional()?;
        Ok(state.map(|required| {
            if required {
                WorkspaceDiscardState::ReconfirmationRequired
            } else {
                WorkspaceDiscardState::Pending
            }
        }))
    }

    pub fn has_pending_discards(&self) -> Result<bool> {
        Ok(self.db()?.query_row(
            "SELECT EXISTS(SELECT 1 FROM pending_workspace_discards WHERE reconfirmation_required=0)",
            [],
            |row| row.get(0),
        )?)
    }

    /// Reattempt accepted requests. A still-locked workspace does not produce a report.
    pub fn retry_pending_discards(
        &self,
        ctx: &OperationContext,
    ) -> Result<Vec<WorkspaceDiscardReport>> {
        if !self.has_pending_discards()? {
            return Ok(vec![]);
        }
        let _guard = self.write_guard()?;
        let pending = self
            .db()?
            .prepare("SELECT variant_id,expected_head,fingerprint FROM pending_workspace_discards WHERE reconfirmation_required=0 ORDER BY variant_id")?
            .query_map([], |row| {
                Ok(PendingDiscard {
                    variant_id: row.get(0)?,
                    expected_head: row.get(1)?,
                    fingerprint: row.get(2)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut reports = vec![];
        for request in &pending {
            ctx.check_cancelled()?;
            match self.discard_changes_locked(
                &request.variant_id,
                &request.expected_head,
                ctx,
                Some(request),
            ) {
                Ok(report) if report.pending => {}
                Ok(report) => reports.push(report),
                Err(error @ CoreError::Cancelled) => return Err(error),
                Err(error) => {
                    // An interrupted swap must recover before the next mutation.
                    ensure_no_discard(&self.root, &*self.db()?)?;
                    let report =
                        WorkspaceDiscardReport::new(&request.variant_id, &request.expected_head);
                    reports.push(self.require_discard_confirmation(
                        report,
                        format!("Confirmez à nouveau l’abandon des modifications : {error}"),
                    )?);
                }
            }
        }
        Ok(reports)
    }

    pub fn discard_changes(
        &self,
        variant_id: &str,
        expected_head: &str,
        ctx: &OperationContext,
    ) -> Result<WorkspaceDiscardReport> {
        let _guard = self.write_guard()?;
        self.discard_changes_locked(variant_id, expected_head, ctx, None)
    }

    fn defer_discard(
        &self,
        mut report: WorkspaceDiscardReport,
        fingerprint: Option<&str>,
        retry: bool,
    ) -> Result<WorkspaceDiscardReport> {
        if !retry {
            self.db()?.execute(
                "INSERT INTO pending_workspace_discards(variant_id,expected_head,fingerprint)
                 VALUES(?1,?2,?3)
                 ON CONFLICT(variant_id) DO UPDATE SET expected_head=excluded.expected_head,
                    fingerprint=excluded.fingerprint,reconfirmation_required=0",
                params![report.variant_id, report.head_snapshot_id, fingerprint],
            )?;
        }
        report.pending = true;
        report.warnings.push(
            "Un fichier ou dossier est utilisé ou inaccessible. L’annulation reprendra lorsque l’accès sera rétabli."
                .into(),
        );
        Ok(report)
    }

    fn require_discard_confirmation(
        &self,
        mut report: WorkspaceDiscardReport,
        warning: String,
    ) -> Result<WorkspaceDiscardReport> {
        self.db()?.execute(
            "UPDATE pending_workspace_discards SET reconfirmation_required=1 WHERE variant_id=?1",
            [&report.variant_id],
        )?;
        report.reconfirmation_required = true;
        report.warnings.push(warning);
        Ok(report)
    }

    fn discard_changes_locked(
        &self,
        variant_id: &str,
        expected_head: &str,
        ctx: &OperationContext,
        pending: Option<&PendingDiscard>,
    ) -> Result<WorkspaceDiscardReport> {
        ctx.check_cancelled()?;
        let mut report = WorkspaceDiscardReport::new(variant_id, expected_head);
        let variant = read_variant(&*self.db()?, &self.root, variant_id);
        let variant = match variant {
            Ok(variant) => variant,
            Err(error) if is_sharing_violation(&error) => {
                return self.defer_discard(report, None, pending.is_some());
            }
            Err(error) => return Err(error),
        };
        if variant.head_snapshot_id != expected_head {
            if pending.is_some() {
                return self.require_discard_confirmation(
                    report,
                    "La version courante a changé. Confirmez à nouveau l’abandon des modifications."
                        .into(),
                );
            }
            return Err(CoreError::Busy("La version courante a changé. Actualisez la fiche avant d’abandonner les modifications.".into()));
        }
        let target = self.get_snapshot(expected_head)?;
        history::require_full(&target)?;
        let workspace = PathBuf::from(&variant.workspace_path);
        let observe = || -> Result<_> {
            let identity = workspace_identity(&workspace)?;
            let entries = self.store.capture(&workspace, false, ctx)?;
            if workspace_identity(&workspace)? != identity {
                return Err(CoreError::Busy(
                    "Le workspace a été remplacé pendant sa lecture.".into(),
                ));
            }
            Ok((identity, entries))
        };
        let (identity, before) = match observe() {
            Ok(observation) => observation,
            Err(error) if is_sharing_violation(&error) => {
                return self.defer_discard(report, None, pending.is_some());
            }
            Err(error) => return Err(error),
        };
        let fingerprint =
            blake3::hash(&serde_json::to_vec(&(&identity, &variant.config, &before))?)
                .to_hex()
                .to_string();
        if let Some(request) = pending
            && request.fingerprint.as_deref() != Some(&fingerprint)
        {
            let warning = if request.fingerprint.is_none() {
                "Le workspace était illisible lors de la demande. Confirmez à nouveau l’abandon des modifications."
            } else {
                "Le workspace ou ses réglages ont changé. Confirmez à nouveau l’abandon des modifications."
            };
            return self.require_discard_confirmation(report, warning.into());
        }
        if before == target.entries && variant.config == target.config {
            self.db()?.execute(
                "DELETE FROM pending_workspace_discards WHERE variant_id=?1",
                [variant_id],
            )?;
            return Ok(report);
        }
        let preparing = tempfile::Builder::new()
            .prefix(".discard-preparing-")
            .tempdir_in(&self.store.staging)?;
        self.store
            .materialize(&target.entries, &preparing.path().join("next"), ctx)?;
        let relative: String = self.db()?.query_row(
            "SELECT workspace FROM variants WHERE id=?1",
            [variant_id],
            |r| r.get(0),
        )?;
        let value = DiscardJournal {
            version: 1,
            id: new_id(),
            variant_id: variant_id.into(),
            workspace: relative,
            head: expected_head.into(),
            before,
            target: target.entries,
        };
        let bytes = serde_json::to_vec(&value)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(preparing.path().join("discard.json"))?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        ctx.check_cancelled()?;
        let directory = self.store.staging.join(format!("discard-{}", value.id));
        fs::rename(preparing.path(), &directory)?;
        ctx.report("Abandon : journal publié", 1, Some(4));
        // Once the journal exists, cancellation waits until rollback or publication is safe.
        let committed = OperationContext {
            progress: ctx.progress.clone(),
            ..Default::default()
        };
        let mut workspace_rename_denied = false;
        let result = (|| -> Result<()> {
            let mut db = self.db()?;
            let tx = db.transaction()?;
            if self.store.capture(&workspace, false, &committed)? != value.before
                || workspace_identity(&workspace)? != identity
            {
                return Err(CoreError::Busy("Le workspace a changé pendant la préparation. Fermez les applications qui le modifient puis réessayez.".into()));
            }
            fs::rename(&workspace, directory.join("previous")).inspect_err(|error| {
                // Windows may return access denied when an open descendant prevents a directory move.
                workspace_rename_denied =
                    cfg!(windows) && error.kind() == std::io::ErrorKind::PermissionDenied;
            })?;
            ctx.report("Abandon : précédent préservé", 2, Some(4));
            if self
                .store
                .capture(&directory.join("previous"), false, &committed)?
                != value.before
                || workspace_identity(&directory.join("previous"))? != identity
            {
                return Err(CoreError::Busy(
                    "Écriture concurrente pendant l’abandon ; modifications préservées.".into(),
                ));
            }
            fs::rename(directory.join("next"), &workspace)?;
            ctx.report("Abandon : dossier remplacé", 3, Some(4));
            if self.store.capture(&workspace, false, &committed)? != value.target {
                return Err(CoreError::Busy(
                    "Écriture concurrente dans le dossier restauré ; fichiers préservés.".into(),
                ));
            }
            tx.execute("UPDATE variants SET config=?2,working_bytes=?3,triangles=NULL WHERE id=?1 AND head_snapshot_id=?4", params![variant_id,serde_json::to_string(&target.config)?,sql_size(target.logical_bytes)?,expected_head])?;
            tx.execute(
                "INSERT INTO workspace_discards(id,journal_hash) VALUES(?1,?2)",
                params![value.id, blake3::hash(&bytes).to_hex().to_string()],
            )?;
            tx.execute(
                "DELETE FROM pending_workspace_discards WHERE variant_id=?1",
                [variant_id],
            )?;
            tx.commit()?;
            Ok(())
        })();
        if let Err(error) = result {
            let published = receipt(&*self.db()?, &value.id)?;
            if published.as_deref() != Some(blake3::hash(&bytes).to_hex().as_ref()) {
                recover_one(&self.root, &mut *self.db()?, &directory, &committed)?;
                if workspace_rename_denied || is_sharing_violation(&error) {
                    return self.defer_discard(report, Some(&fingerprint), pending.is_some());
                }
                return Err(error);
            }
        }
        ctx.report("Abandon : catalogue publié", 4, Some(4));
        if let Err(error) = recover_one(&self.root, &mut *self.db()?, &directory, &committed) {
            report.cleanup_pending = true;
            report.warnings.push(format!("Modifications abandonnées ; nettoyage de l’ancien dossier à reprendre à la réouverture : {error}"));
        }
        Ok(report)
    }
}

fn remove_subset(
    store: &FileStore,
    path: &Path,
    expected: &[ManifestEntry],
    ctx: &OperationContext,
) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
        Ok(_) => ensure_real_directory(path)?,
    }
    let current = store.capture(path, false, ctx)?;
    let allowed = expected
        .iter()
        .map(|entry| (&entry.path, entry))
        .collect::<BTreeMap<_, _>>();
    if current
        .iter()
        .any(|entry| allowed.get(&entry.path).copied() != Some(entry))
    {
        return Err(CoreError::Inconsistent(format!(
            "Ancien dossier modifié après l’abandon ; fichiers préservés : {}",
            path.display()
        )));
    }
    for entry in current.iter().filter(|entry| entry.kind == "file") {
        let relative = safe_relative(&entry.path)?;
        let mut parent = path.to_path_buf();
        ensure_real_directory(&parent)?;
        if let Some(components) = relative.parent() {
            for component in components.components() {
                parent.push(component);
                ensure_real_directory(&parent)?;
            }
        }
        let file = path.join(relative);
        let before = cleanup_file_metadata(&file)?;
        if before.len() != entry.size
            || crate::preview::hash_file(&file, ctx)?.as_deref() != entry.hash.as_deref()
        {
            return Err(CoreError::Inconsistent(format!(
                "Fichier modifié pendant le nettoyage ; contenu préservé : {}",
                file.display()
            )));
        }
        let after = cleanup_file_metadata(&file)?;
        if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
            return Err(CoreError::Inconsistent(format!(
                "Fichier modifié pendant sa vérification ; contenu préservé : {}",
                file.display()
            )));
        }
        fs::remove_file(&file)?;
        ctx.report("Abandon : fichier nettoyé", 1, None);
    }
    let mut directories = current
        .iter()
        .filter(|entry| entry.kind == "directory")
        .collect::<Vec<_>>();
    directories.sort_by_key(|entry| std::cmp::Reverse(entry.path.split('/').count()));
    for entry in directories {
        let directory = path.join(safe_relative(&entry.path)?);
        ensure_real_directory(&directory)?;
        fs::remove_dir(directory)?;
    }
    fs::remove_dir(path)?;
    Ok(())
}

fn cleanup_file_metadata(path: &Path) -> Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    let linked = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let linked = metadata.file_type().is_symlink();
    if linked || !metadata.is_file() {
        return Err(CoreError::Invalid(
            "Le nettoyage exige un fichier ordinaire sans lien.".into(),
        ));
    }
    Ok(metadata)
}

fn recover_one(
    root: &Path,
    db: &mut Connection,
    directory: &Path,
    ctx: &OperationContext,
) -> Result<()> {
    if empty_checkpoint(directory)?
        && let Some(id) = directory
            .file_name()
            .and_then(|v| v.to_str())
            .and_then(|v| v.strip_prefix("discard-"))
    {
        if uuid::Uuid::parse_str(id).is_err() {
            return Err(CoreError::Inconsistent(
                "Identifiant de récupération d’abandon invalide.".into(),
            ));
        }
        fs::remove_dir(directory)?;
        db.execute("DELETE FROM workspace_discards WHERE id=?1", [id])?;
        return Ok(());
    }
    let (value, hash) = journal(root, directory)?;
    let published = receipt(db, &value.id)?;
    if published.as_ref().is_some_and(|receipt| receipt != &hash) {
        return Err(CoreError::Inconsistent(
            "Journal d’abandon modifié ; ancien dossier préservé.".into(),
        ));
    }
    let store = FileStore {
        objects: root.join("objects"),
        staging: root.join("staging"),
    };
    let previous = directory.join("previous");
    let next = directory.join("next");
    if published.is_some() {
        // This checkpoint owns only the detached old tree, never the live workspace.
        remove_subset(&store, &previous, &value.before, ctx)?;
    } else {
        // The live directory can be temporarily absent between the two renames.
        let (current_head, relative): (String, String) = db.query_row(
            "SELECT head_snapshot_id,workspace FROM variants WHERE id=?1",
            [&value.variant_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let head = read_snapshot(db, &value.head)?;
        if current_head != value.head || relative != value.workspace || head.entries != value.target
        {
            return Err(CoreError::Inconsistent(
                "Le catalogue ne correspond plus à l’abandon interrompu ; fichiers préservés."
                    .into(),
            ));
        }
        let workspace = root.join(safe_relative(&relative)?);
        ensure_real_directory(&root.join("workspaces"))?;
        ensure_real_directory(
            workspace
                .parent()
                .ok_or_else(|| CoreError::Invalid("Workspace invalide.".into()))?,
        )?;
        if path_exists(&previous)? {
            ensure_real_directory(&previous)?;
            if path_exists(&workspace)? {
                if path_exists(&next)? || store.capture(&workspace, false, ctx)? != value.target {
                    return Err(CoreError::Inconsistent(
                        "Le workspace a changé après interruption ; récupération manuelle requise."
                            .into(),
                    ));
                }
                fs::rename(&workspace, &next)?;
            }
            fs::rename(&previous, &workspace)?;
        } else {
            ensure_real_directory(&workspace)?;
        }
    }
    remove_subset(&store, &next, &value.target, ctx)?;
    fs::remove_file(directory.join("discard.json"))?;
    fs::remove_dir(directory)?;
    db.execute("DELETE FROM workspace_discards WHERE id=?1", [&value.id])?;
    Ok(())
}

pub(super) fn recover_discards(root: &Path, db: &mut Connection) -> Result<Vec<String>> {
    let mut notices = vec![];
    for item in fs::read_dir(root.join("staging"))? {
        let path = item?.path();
        if path
            .file_name()
            .is_some_and(|v| v.to_string_lossy().starts_with("discard-"))
            && let Err(error) = recover_one(root, db, &path, &OperationContext::default())
        {
            notices.push(format!("Abandon à récupérer : {error}"));
        }
    }
    let ids = db
        .prepare("SELECT id FROM workspace_discards")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for id in ids {
        if !path_exists(&root.join("staging").join(format!("discard-{id}")))? {
            db.execute("DELETE FROM workspace_discards WHERE id=?1", [id])?;
        }
    }
    Ok(notices)
}
