//! Retention only releases immutable objects. User workspace files are never classified or cleaned.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageAction {
    Compact,
    ReleaseBackup,
    DeleteVersion,
    Recompress,
    Purge,
}

pub(super) fn require_full(snapshot: &Snapshot) -> Result<()> {
    if snapshot.storage.kind != "full" {
        return Err(CoreError::Invalid("Cette version conserve uniquement son aperçu ; son dossier Blender n’est plus disponible.".into()));
    }
    Ok(())
}

fn preview_entry(snapshot: &Snapshot) -> Result<ManifestEntry> {
    if snapshot.storage.kind == "deleted" {
        return Err(CoreError::NotFound(snapshot.id.clone()));
    }
    if !snapshot.storage.preview_trusted {
        return Err(CoreError::Invalid(
            "Capture de sécurité sans GLB certifié : ses sources doivent rester complètes.".into(),
        ));
    }
    let candidates: Vec<_> = snapshot
        .entries
        .iter()
        .filter(|e| {
            e.kind == "file"
                && Path::new(&e.path)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("glb"))
        })
        .collect();
    if let Some(path) = &snapshot.storage.preview_path {
        if let Some(entry) = candidates.iter().find(|entry| &entry.path == path) {
            return Ok((*entry).clone());
        }
    } else if let Some(entry) = snapshot
        .config
        .generated_glb_file
        .as_ref()
        .and_then(|path| candidates.iter().find(|entry| &entry.path == path))
    {
        return Ok((*entry).clone());
    } else if !candidates.is_empty() {
        let paired = snapshot.config.primary_file.as_deref().map(|primary| {
            Path::new(primary)
                .with_extension("glb")
                .to_string_lossy()
                .replace('\\', "/")
        });
        return Ok((*candidates
            .iter()
            .find(|entry| Some(entry.path.as_str()) == paired.as_deref())
            .unwrap_or(&candidates[0]))
        .clone());
    }
    Err(CoreError::Invalid(
        "Aucun GLB historique autonome disponible ; les sources sont conservées.".into(),
    ))
}

impl Library {
    pub fn extract_snapshot_file(
        &self,
        id: &str,
        relative: &str,
        destination: &Path,
        ctx: &OperationContext,
    ) -> Result<()> {
        safe_relative(relative)?;
        let snapshot = self.get_snapshot(id)?;
        if snapshot.storage.kind == "deleted" {
            return Err(CoreError::NotFound(id.into()));
        }
        let entry = snapshot
            .entries
            .iter()
            .find(|e| e.kind == "file" && e.path == relative)
            .ok_or_else(|| CoreError::NotFound(relative.into()))?;
        let parent = destination
            .parent()
            .ok_or_else(|| CoreError::Invalid("Destination invalide.".into()))?;
        ensure_real_directory(parent)?;
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        self.store.copy_object(
            entry
                .hash
                .as_deref()
                .ok_or_else(|| CoreError::Inconsistent("Hash absent.".into()))?,
            entry.size,
            &mut temp,
            ctx,
        )?;
        temp.as_file().sync_all()?;
        ctx.check_cancelled()?;
        temp.persist_noclobber(destination)
            .map_err(|e| CoreError::Io(e.error))?;
        Ok(())
    }

    pub fn snapshot_glb_path(&self, id: &str) -> Result<String> {
        Ok(preview_entry(&self.get_snapshot(id)?)?.path)
    }

    fn verified_previews(
        &self,
        snapshot: &Snapshot,
        ctx: &OperationContext,
    ) -> Result<Vec<ManifestEntry>> {
        let principal = preview_entry(snapshot)?;
        let mut entries = vec![principal];
        entries.extend(
            snapshot
                .entries
                .iter()
                .filter(|entry| {
                    entry.kind == "file"
                        && crate::sync::is_glb(&entry.path)
                        && entry.path != entries[0].path
                })
                .cloned()
                .collect::<Vec<_>>(),
        );
        let temporary = tempfile::tempdir_in(&self.store.staging)?;
        for (index, entry) in entries.iter().enumerate() {
            let path = temporary.path().join(format!("preview-{index}.glb"));
            self.extract_snapshot_file(&snapshot.id, &entry.path, &path, ctx)?;
            crate::glb::guard_preview(&path)?;
        }
        Ok(entries)
    }

    fn scope_snapshots(&self, variant_id: &str) -> Result<Vec<Snapshot>> {
        let mut offset = 0;
        let mut result = Vec::new();
        loop {
            let page = self.list_snapshots(variant_id, offset, 500)?;
            let count = page.items.len();
            result.extend(page.items);
            offset += count as u32;
            if count == 0 || u64::from(offset) >= page.total {
                break;
            }
        }
        Ok(result)
    }

    fn downgrade(
        &self,
        id: &str,
        entries: &[ManifestEntry],
        release_variant: Option<&str>,
    ) -> Result<()> {
        let mut db = self.db()?;
        let tx = db.transaction()?;
        if let Some(variant) = release_variant {
            tx.execute(
                "UPDATE variants SET backup_snapshot_id=NULL WHERE id=?1 AND backup_snapshot_id=?2",
                params![variant, id],
            )?;
        }
        if read_snapshot_summary(&tx, id)?.storage.protected {
            // Releasing a role must not release data still protected by another variant.
            tx.commit()?;
            return Ok(());
        }
        let retained: BTreeSet<_> = entries.iter().map(|entry| entry.path.as_str()).collect();
        for entry in read_snapshot(&tx, id)?.entries {
            if retained.contains(entry.path.as_str()) {
                continue;
            }
            if entry.kind == "file" {
                tx.execute(
                    "INSERT OR IGNORE INTO garbage_candidates(hash,size) VALUES(?1,?2)",
                    params![entry.hash, sql_size(entry.size)?],
                )?;
            }
            tx.execute(
                "DELETE FROM snapshot_entries WHERE snapshot_id=?1 AND path=?2",
                params![id, entry.path],
            )?;
        }
        tx.execute(
            "UPDATE snapshots SET storage_kind='preview',preview_path=?2,legacy=0 WHERE id=?1",
            params![id, entries[0].path],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub(super) fn rotate_history(&self, variant: &str, ctx: &OperationContext) {
        let Ok(snapshots) = self.scope_snapshots(variant) else {
            return;
        };
        for summary in snapshots {
            if summary.variant_id != variant
                || summary.storage.kind != "full"
                || summary.storage.protected
                || summary.storage.legacy
            {
                continue;
            }
            let result = (|| {
                let snapshot = self.get_snapshot(&summary.id)?;
                let entries = self.verified_previews(&snapshot, ctx)?;
                ctx.check_cancelled()?;
                self.downgrade(&snapshot.id, &entries, None)
            })();
            if result.is_err() && ctx.cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            // Failed candidates stay full and are explicitly reported by the storage planner/UI.
        }
        let _ = self.collect_garbage(ctx);
    }

    fn referenced_objects(&self) -> Result<BTreeMap<String, u64>> {
        let db = self.db()?;
        Ok(db
            .prepare("SELECT hash,MAX(size) FROM snapshot_entries WHERE kind='file' GROUP BY hash")?
            .query_map([], |row| Ok((row.get(0)?, sql_u64(row, 1)?)))?
            .collect::<std::result::Result<_, _>>()?)
    }

    fn garbage(&self) -> Result<Vec<(String, u64)>> {
        self.check_storage_integrity()?;
        let db = self.db()?;
        Ok(db.prepare("SELECT hash,size FROM garbage_candidates g WHERE NOT EXISTS(SELECT 1 FROM snapshot_entries e WHERE e.hash=g.hash)")?
            .query_map([], |r| Ok((r.get(0)?,sql_u64(r,1)?)))?.collect::<std::result::Result<_,_>>()?)
    }

    fn object_bytes(&self, hash: &str) -> Result<u64> {
        let raw = self.store.object_path(hash)?;
        let mut total = 0;
        for path in [&raw, &raw.with_extension("zst")] {
            match fs::symlink_metadata(path) {
                Ok(meta) if meta.is_file() => {
                    #[cfg(windows)]
                    {
                        use std::os::windows::fs::MetadataExt;
                        if meta.file_attributes() & 0x400 != 0 {
                            return Err(CoreError::Invalid(
                                "Lien interdit dans les objets.".into(),
                            ));
                        }
                    }
                    if meta.file_type().is_symlink() {
                        return Err(CoreError::Invalid("Lien interdit dans les objets.".into()));
                    }
                    total += meta.len();
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                _ => {
                    return Err(CoreError::Inconsistent(
                        "Objet de stockage invalide.".into(),
                    ));
                }
            }
        }
        Ok(total)
    }

    pub(super) fn check_storage_integrity(&self) -> Result<()> {
        ensure_no_restore(&self.root)?;
        discard::ensure_no_discard(&self.root, &*self.db()?)?;
        // Unknown/interrupted imports may own otherwise unreferenced objects. Fail closed.
        for item in fs::read_dir(&self.store.staging)? {
            let item = item?;
            if item.path().join("operation.json").exists() {
                return Err(CoreError::Busy(
                    "Une opération interrompue doit être récupérée avant de libérer le stockage."
                        .into(),
                ));
            }
        }
        let db = self.db()?;
        let check: String = db.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        let invalid: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM variants v LEFT JOIN snapshots h ON h.id=v.head_snapshot_id LEFT JOIN snapshots b ON b.id=v.backup_snapshot_id WHERE v.deleted_name IS NULL AND (h.id IS NULL OR h.storage_kind<>'full' OR h.variant_id<>v.id OR (v.backup_snapshot_id IS NOT NULL AND (b.id IS NULL OR b.storage_kind<>'full'))))", [], |r| r.get(0))?;
        if check != "ok" || invalid || db.prepare("PRAGMA foreign_key_check")?.exists([])? {
            return Err(CoreError::Inconsistent(
                "Catalogue incohérent : aucune purge effectuée.".into(),
            ));
        }
        Ok(())
    }

    fn collect_garbage(&self, ctx: &OperationContext) -> Result<u64> {
        let candidates = self.garbage()?;
        if candidates.is_empty() {
            return Ok(0);
        }
        // Replacement previews are verified before the transaction releases their sources.
        // Reachability is checked globally under the mutation lock; do not rehash the entire
        // library for each manual version. Object hashes are checked on every actual read.
        let mut released = 0;
        for (hash, _) in candidates {
            ctx.check_cancelled()?;
            released += self.object_bytes(&hash)?;
            let raw = self.store.object_path(&hash)?;
            for path in [&raw, &raw.with_extension("zst")] {
                match fs::remove_file(path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
            }
            self.db()?
                .execute("DELETE FROM garbage_candidates WHERE hash=?1", [&hash])?;
        }
        Ok(released)
    }

    fn plan_storage(
        &self,
        variant_id: Option<&str>,
        action: StorageAction,
        snapshot_id: Option<&str>,
        ctx: &OperationContext,
    ) -> Result<StorageReport> {
        self.check_storage_integrity()?;
        if matches!(action, StorageAction::Recompress | StorageAction::Purge) {
            if snapshot_id.is_some() {
                return Err(CoreError::Invalid(
                    "Cette action concerne toute la bibliothèque, sans version ciblée.".into(),
                ));
            }
            return self.plan_library_storage(action, ctx);
        }
        let variant_id = variant_id.ok_or_else(|| {
            CoreError::Invalid("Une variante est requise pour cette action historique.".into())
        })?;
        let variant = self.get_variant(variant_id)?.variant;
        let scope = self.scope_snapshots(variant_id)?;
        let mut report = StorageReport::default();
        let refs = self.referenced_objects()?;
        report.logical_bytes = refs.values().sum();
        report.physical_bytes = directory_bytes(&self.store.objects)?;
        let selected: Vec<String> = match action {
            StorageAction::Compact => scope
                .iter()
                .filter(|s| s.storage.kind == "full" && !s.storage.protected)
                .map(|s| s.id.clone())
                .collect(),
            StorageAction::ReleaseBackup => vec![
                variant
                    .backup_snapshot_id
                    .ok_or_else(|| CoreError::Invalid("Aucun backup à libérer.".into()))?,
            ],
            StorageAction::DeleteVersion => {
                let id =
                    snapshot_id.ok_or_else(|| CoreError::Invalid("Version requise.".into()))?;
                let snapshot = scope
                    .iter()
                    .find(|s| s.id == id)
                    .ok_or_else(|| CoreError::Invalid("Version hors de cet historique.".into()))?;
                if snapshot.storage.protected || snapshot.storage.kind != "preview" {
                    return Err(CoreError::Invalid(
                        "Seule une version de consultation non protégée peut être supprimée."
                            .into(),
                    ));
                }
                vec![id.into()]
            }
            _ => Vec::new(),
        };
        let mut discarded = BTreeMap::<String, BTreeSet<String>>::new();
        let mut retained_hashes = BTreeSet::new();
        for id in selected {
            ctx.check_cancelled()?;
            let snapshot = self.get_snapshot(&id)?;
            let kept = if action == StorageAction::DeleteVersion {
                BTreeSet::new()
            } else {
                match self.verified_previews(&snapshot, ctx) {
                    Ok(entries) => {
                        retained_hashes
                            .extend(entries.iter().filter_map(|entry| entry.hash.clone()));
                        entries
                            .into_iter()
                            .map(|entry| entry.path)
                            .collect::<BTreeSet<_>>()
                    }
                    Err(CoreError::Cancelled) => return Err(CoreError::Cancelled),
                    Err(error) => {
                        if action == StorageAction::ReleaseBackup {
                            return Err(error);
                        }
                        report
                            .warnings
                            .push(format!("{} : {}", snapshot.message, error));
                        continue;
                    }
                }
            };
            let other_protection: bool = self.db()?.query_row("SELECT EXISTS(SELECT 1 FROM variants WHERE deleted_name IS NULL AND (head_snapshot_id=?1 OR (backup_snapshot_id=?1 AND id<>?2)))", params![id,variant_id], |r|r.get(0))?;
            if other_protection {
                report.warnings.push(format!(
                    "{} : contenu conservé car protégé par une autre variante.",
                    snapshot.message
                ));
            } else {
                for entry in &snapshot.entries {
                    if entry.kind == "file" && !kept.contains(&entry.path) {
                        discarded
                            .entry(
                                entry.hash.clone().ok_or_else(|| {
                                    CoreError::Inconsistent("Hash absent.".into())
                                })?,
                            )
                            .or_default()
                            .insert(id.clone());
                    }
                }
            }
            report.versions.push(StorageVersion {
                id,
                message: snapshot.message,
            });
        }
        let mut reclaim = BTreeSet::new();
        for (hash, removed_versions) in discarded {
            let db = self.db()?;
            let owners: BTreeSet<String> = db
                .prepare("SELECT DISTINCT snapshot_id FROM snapshot_entries WHERE hash=?1")?
                .query_map([&hash], |r| r.get(0))?
                .collect::<std::result::Result<_, _>>()?;
            drop(db);
            // A retained GLB can share a hash with an entry being discarded; count remaining paths too.
            let preserved = retained_hashes.contains(&hash);
            if !preserved && owners.is_subset(&removed_versions) {
                reclaim.insert(hash);
            }
        }
        for (hash, _) in self.garbage()? {
            reclaim.insert(hash);
        }
        for hash in reclaim {
            report.reclaimable_bytes += self.object_bytes(&hash)?;
        }
        if action == StorageAction::Recompress {
            report.reclaimable_bytes = 0;
            for (hash, size) in refs {
                report.reclaimable_bytes += self.store.recompression_savings(&hash, size, ctx)?;
            }
        }
        let payload = serde_json::to_vec(&(
            variant_id,
            action,
            snapshot_id,
            &variant.head_snapshot_id,
            &report,
        ))?;
        report.plan_token = blake3::hash(&payload).to_hex().to_string();
        Ok(report)
    }

    fn plan_library_storage(
        &self,
        action: StorageAction,
        ctx: &OperationContext,
    ) -> Result<StorageReport> {
        let refs = self.referenced_objects()?;
        let mut report = StorageReport {
            logical_bytes: refs.values().sum(),
            physical_bytes: directory_bytes(&self.store.objects)?,
            ..Default::default()
        };
        let mut targets = match action {
            StorageAction::Recompress => refs
                .iter()
                .map(|(hash, size)| (hash.clone(), *size))
                .collect::<Vec<_>>(),
            StorageAction::Purge => self.garbage()?,
            _ => {
                return Err(CoreError::Invalid(
                    "Action globale non prise en charge.".into(),
                ));
            }
        };
        targets.sort_unstable();
        let mut representations = Vec::with_capacity(targets.len());
        for (hash, size) in &targets {
            ctx.check_cancelled()?;
            let bytes = self.object_bytes(hash)?;
            report.reclaimable_bytes += if action == StorageAction::Recompress {
                self.store.recompression_savings(hash, *size, ctx)?
            } else {
                bytes
            };
            let raw = self.store.object_path(hash)?;
            let mut state = Vec::new();
            for path in [&raw, &raw.with_extension("zst")] {
                state.push(match fs::symlink_metadata(path) {
                    Ok(meta) => Some((
                        meta.len(),
                        meta.modified()?
                            .duration_since(std::time::UNIX_EPOCH)
                            .ok()
                            .map(|time| time.as_nanos()),
                    )),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(error) => return Err(error.into()),
                });
            }
            representations.push(state);
        }
        // Bind confirmation to the global targets, their references and representations,
        // including metadata changes that leave aggregate byte totals unchanged.
        report.plan_token = blake3::hash(&serde_json::to_vec(&(
            action,
            &refs,
            &targets,
            representations,
            &report,
        ))?)
        .to_hex()
        .to_string();
        Ok(report)
    }

    pub fn preview_storage_action(
        &self,
        variant_id: &str,
        action: StorageAction,
        snapshot_id: Option<&str>,
        ctx: &OperationContext,
    ) -> Result<StorageReport> {
        let _guard = self.write_guard()?;
        self.plan_storage(Some(variant_id), action, snapshot_id, ctx)
    }

    pub fn preview_library_storage_action(
        &self,
        action: StorageAction,
        ctx: &OperationContext,
    ) -> Result<StorageReport> {
        let _guard = self.write_guard()?;
        self.plan_storage(None, action, None, ctx)
    }

    pub fn apply_storage_action(
        &self,
        variant_id: &str,
        action: StorageAction,
        snapshot_id: Option<&str>,
        plan_token: &str,
        ctx: &OperationContext,
    ) -> Result<StorageReport> {
        self.apply_storage_action_inner(Some(variant_id), action, snapshot_id, plan_token, ctx)
    }

    pub fn apply_library_storage_action(
        &self,
        action: StorageAction,
        plan_token: &str,
        ctx: &OperationContext,
    ) -> Result<StorageReport> {
        self.apply_storage_action_inner(None, action, None, plan_token, ctx)
    }

    fn apply_storage_action_inner(
        &self,
        variant_id: Option<&str>,
        action: StorageAction,
        snapshot_id: Option<&str>,
        plan_token: &str,
        ctx: &OperationContext,
    ) -> Result<StorageReport> {
        let _guard = self.write_guard()?;
        let mut report = self.plan_storage(variant_id, action, snapshot_id, ctx)?;
        if report.plan_token != plan_token {
            return Err(CoreError::Busy(
                "Le stockage a changé. Analysez à nouveau avant de confirmer.".into(),
            ));
        }
        for version in &report.versions {
            ctx.check_cancelled()?;
            if action == StorageAction::DeleteVersion {
                let mut db = self.db()?;
                let tx = db.transaction()?;
                tx.execute("INSERT OR IGNORE INTO garbage_candidates(hash,size) SELECT hash,size FROM snapshot_entries WHERE snapshot_id=?1 AND kind='file'", [&version.id])?;
                tx.execute(
                    "DELETE FROM file_selections WHERE context_key=?1",
                    [&version.id],
                )?;
                tx.execute(
                    "DELETE FROM snapshot_entries WHERE snapshot_id=?1",
                    [&version.id],
                )?;
                tx.execute(
                    "UPDATE snapshots SET storage_kind='deleted',preview_path=NULL WHERE id=?1",
                    [&version.id],
                )?;
                tx.commit()?;
            } else {
                let entries = self.verified_previews(&self.get_snapshot(&version.id)?, ctx)?;
                self.downgrade(
                    &version.id,
                    &entries,
                    (action == StorageAction::ReleaseBackup)
                        .then_some(variant_id)
                        .flatten(),
                )?;
            }
        }
        if action == StorageAction::Recompress {
            for (hash, size) in self.referenced_objects()? {
                report.released_bytes += self.store.recompress(&hash, size, ctx)?;
            }
        } else {
            match self.collect_garbage(ctx) {
                Ok(bytes) => report.released_bytes = bytes,
                Err(error) => report.warnings.push(format!(
                    "Les changements publiés sont conservés. Libération à reprendre : {error}"
                )),
            }
        }
        report.physical_bytes = directory_bytes(&self.store.objects)?;
        Ok(report)
    }
}

#[derive(Serialize, Deserialize)]
struct RestoreJournal {
    variant_id: String,
    workspace: String,
    old_head: String,
    before: Snapshot,
    after: Snapshot,
}

pub(super) fn ensure_no_restore(root: &Path) -> Result<()> {
    for entry in fs::read_dir(root.join("staging"))? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with("restore-") {
            return Err(CoreError::Busy(format!(
                "Une restauration attend sa récupération. Fermez puis rouvrez la bibliothèque. Fichiers préservés : {}",
                entry.path().display()
            )));
        }
    }
    Ok(())
}

impl Library {
    pub fn restore_backup(
        &self,
        variant_id: &str,
        expected_head: &str,
        expected_backup: &str,
        ctx: &OperationContext,
    ) -> Result<Snapshot> {
        let _guard = self.write_guard()?;
        let variant = self.get_variant(variant_id)?.variant;
        if variant.head_snapshot_id != expected_head
            || variant.backup_snapshot_id.as_deref() != Some(expected_backup)
        {
            return Err(CoreError::Busy(
                "La version courante ou le backup a changé. Actualisez la fiche.".into(),
            ));
        }
        let target = self.get_snapshot(expected_backup)?;
        require_full(&target)?;
        let workspace = PathBuf::from(&variant.workspace_path);
        let entries = self.store.capture(&workspace, true, ctx)?;
        let current = self.get_snapshot(&variant.head_snapshot_id)?;
        let trusted = entries == current.entries && current.storage.preview_trusted;
        let before = Snapshot {
            id: new_id(),
            variant_id: variant_id.into(),
            parent_id: Some(variant.head_snapshot_id.clone()),
            created_at: now(),
            message: "État sauvegardé avant restauration".into(),
            logical_bytes: entries.iter().map(|e| e.size).sum(),
            config: variant.config.clone(),
            entries,
            storage: SnapshotStorage {
                preview_trusted: trusted,
                ..Default::default()
            },
        };
        let mut after = target.clone();
        after.id = new_id();
        after.variant_id = variant_id.into();
        after.parent_id = Some(before.id.clone());
        after.created_at = now();
        after.message = format!("Restaurée depuis {}", target.id);
        after.storage = SnapshotStorage {
            restored_from: Some(target.id),
            preview_trusted: target.storage.preview_trusted,
            ..Default::default()
        };
        let temporary = tempfile::Builder::new()
            .prefix(".restore-preparing-")
            .tempdir_in(&self.store.staging)?;
        self.store
            .materialize(&after.entries, &temporary.path().join("next"), ctx)?;
        if self.store.capture(&workspace, false, ctx)? != before.entries {
            return Err(CoreError::Inconsistent(
                "Le workspace a changé pendant la préparation. Enregistrez et fermez Blender."
                    .into(),
            ));
        }
        // An exclusive-open preflight catches Windows editors holding the source files.
        #[cfg(windows)]
        for entry in &before.entries {
            if entry.kind == "file" {
                use std::os::windows::fs::OpenOptionsExt;
                let _file = OpenOptions::new()
                    .read(true)
                    .share_mode(0)
                    .open(workspace.join(safe_relative(&entry.path)?))?;
            }
        }
        let relative: String = self.db()?.query_row(
            "SELECT workspace FROM variants WHERE id=?1",
            [variant_id],
            |r| r.get(0),
        )?;
        let journal = RestoreJournal {
            variant_id: variant_id.into(),
            workspace: relative,
            old_head: variant.head_snapshot_id,
            before,
            after,
        };
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(temporary.path().join("restore.json"))?;
        file.write_all(&serde_json::to_vec(&journal)?)?;
        file.sync_all()?;
        drop(file);
        ctx.check_cancelled()?;
        let directory = self
            .store
            .staging
            .join(format!("restore-{}", journal.after.id));
        fs::rename(temporary.path(), &directory)?;
        ctx.report("Restauration : journal publié", 1, Some(4));
        // Beyond this boundary cancellation is deferred until the operation is consistent.
        let committed = OperationContext::default();
        let result = (|| -> Result<()> {
            fs::rename(&workspace, directory.join("previous"))?;
            ctx.report("Restauration : précédent préservé", 2, Some(4));
            if self
                .store
                .capture(&directory.join("previous"), false, &committed)?
                != journal.before.entries
            {
                return Err(CoreError::Inconsistent(
                    "Écriture concurrente pendant la restauration.".into(),
                ));
            }
            fs::rename(directory.join("next"), &workspace)?;
            ctx.report("Restauration : dossier remplacé", 3, Some(4));
            let mut db = self.db()?;
            let tx = db.transaction()?;
            insert_snapshot(&tx, &journal.before)?;
            insert_snapshot(&tx, &journal.after)?;
            tx.execute("UPDATE variants SET head_snapshot_id=?2,backup_snapshot_id=?3,config=?4,working_bytes=?5,triangles=NULL WHERE id=?1",params![variant_id,journal.after.id,journal.before.id,serde_json::to_string(&journal.after.config)?,sql_size(journal.after.logical_bytes)?])?;
            tx.execute(
                "UPDATE assets SET updated_at=?2 WHERE id=?1",
                params![variant.asset_id, journal.after.created_at],
            )?;
            tx.commit()?;
            ctx.report("Restauration : catalogue publié", 4, Some(4));
            Ok(())
        })();
        let recovery = recover_restore(&self.root, &mut *self.db()?, &directory);
        if let Err(error) = result {
            recovery?;
            return Err(error);
        }
        // A committed restoration remains successful even if an externally changed old folder needs review.
        if recovery.is_ok() {
            self.rotate_history(variant_id, &committed);
        }
        self.get_snapshot(&journal.after.id)
    }
}

fn remove_verified(store: &FileStore, path: &Path, entries: &[ManifestEntry]) -> Result<()> {
    if path.exists() {
        ensure_real_directory(path)?;
        if store.capture(path, false, &OperationContext::default())? != entries {
            return Err(CoreError::Inconsistent(format!(
                "Fichiers modifiés après interruption, conservés pour récupération : {}",
                path.display()
            )));
        }
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

fn recover_restore(root: &Path, db: &mut Connection, directory: &Path) -> Result<()> {
    ensure_real_directory(directory)?;
    let path = directory.join("restore.json");
    let metadata = fs::symlink_metadata(&path)?;
    if !metadata.is_file() || metadata.len() > 32 * 1024 * 1024 {
        return Err(CoreError::Inconsistent(
            "Journal de restauration invalide.".into(),
        ));
    }
    let journal: RestoreJournal = serde_json::from_slice(&fs::read(path)?)?;
    let (workspace_relative, head): (String, String) = db.query_row(
        "SELECT workspace,head_snapshot_id FROM variants WHERE id=?1",
        [&journal.variant_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if workspace_relative != journal.workspace
        || !workspace_relative.starts_with("workspaces/")
        || journal.before.variant_id != journal.variant_id
        || journal.after.variant_id != journal.variant_id
        || directory.file_name().and_then(|n| n.to_str())
            != Some(&format!("restore-{}", journal.after.id))
    {
        return Err(CoreError::Inconsistent(
            "Journal de restauration incohérent.".into(),
        ));
    }
    let workspace = root.join(safe_relative(&workspace_relative)?);
    ensure_real_directory(
        workspace
            .parent()
            .ok_or_else(|| CoreError::Invalid("Workspace invalide.".into()))?,
    )?;
    let store = FileStore {
        objects: root.join("objects"),
        staging: root.join("staging"),
    };
    let previous = directory.join("previous");
    let next = directory.join("next");
    if head == journal.after.id {
        ensure_real_directory(&workspace)?;
        // The on-disk previous folder is the last recovery source if stored objects were
        // lost or corrupted after the commit. Never remove it without checking the backup.
        let backup = read_snapshot(db, &journal.before.id)?;
        if backup.entries != journal.before.entries || backup.storage.kind != "full" {
            return Err(CoreError::Inconsistent(
                "Le backup publié ne correspond plus au journal ; ancien dossier conservé.".into(),
            ));
        }
        for entry in &backup.entries {
            if entry.kind == "file" {
                store.verify_object(
                    entry
                        .hash
                        .as_deref()
                        .ok_or_else(|| CoreError::Inconsistent("Hash de backup absent.".into()))?,
                    entry.size,
                    &OperationContext::default(),
                )?;
            }
        }
        remove_verified(&store, &previous, &journal.before.entries)?;
    } else if head == journal.old_head {
        if previous.exists() {
            ensure_real_directory(&previous)?;
            if workspace.exists() {
                // Never overwrite post-crash edits, even in an unpublished destination.
                if next.exists() {
                    return Err(CoreError::Inconsistent(
                        "Deux destinations de restauration présentes.".into(),
                    ));
                }
                if store.capture(&workspace, false, &OperationContext::default())?
                    != journal.after.entries
                {
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
    } else {
        return Err(CoreError::Inconsistent(
            "La tête de variante ne correspond pas au journal.".into(),
        ));
    }
    remove_verified(&store, &next, &journal.after.entries)?;
    fs::remove_file(directory.join("restore.json"))?;
    fs::remove_dir(directory)?;
    Ok(())
}

pub(super) fn recover_restores(root: &Path, db: &mut Connection) -> Result<Vec<String>> {
    let mut notices = Vec::new();
    for entry in fs::read_dir(root.join("staging"))? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with("restore-")
            && let Err(error) = recover_restore(root, db, &entry.path())
        {
            notices.push(format!("Restauration à récupérer : {error}"));
        }
    }
    Ok(notices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_one_upgrade_preserves_raw_objects_history_metadata_and_catalog_backup() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("library");
        fs::create_dir_all(root.join("workspaces/asset/variant")).unwrap();
        fs::create_dir(root.join("staging")).unwrap();
        let bytes = vec![7; 1024 * 1024];
        let hash = blake3::hash(&bytes).to_hex().to_string();
        fs::create_dir_all(root.join("objects").join(&hash[..2])).unwrap();
        fs::write(root.join("objects").join(&hash[..2]).join(&hash), &bytes).unwrap();
        fs::write(root.join("workspaces/asset/variant/main.blend"), &bytes).unwrap();
        fs::write(root.join(".edge-lock-library"), FORMAT).unwrap();
        {
            let db = Connection::open(root.join("library.sqlite3")).unwrap();
            db.execute_batch(SCHEMA).unwrap();
            db.execute("INSERT INTO assets(id,name,created_at,updated_at) VALUES('asset','Original','date','date')",[]).unwrap();
            db.execute("INSERT INTO variants(id,asset_id,name,workspace,head_snapshot_id,config,created_at) VALUES('variant','asset','Main','workspaces/asset/variant','s1','{}','date')",[]).unwrap();
            db.execute("INSERT INTO snapshots(id,variant_id,created_at,message,logical_bytes,config) VALUES('s1','variant','date','Original',?1,'{}')",[bytes.len() as i64]).unwrap();
            db.execute(
                "INSERT INTO snapshot_entries VALUES('s1','main.blend','file',?1,?2)",
                params![hash, bytes.len() as i64],
            )
            .unwrap();
            db.execute("INSERT INTO tags VALUES('keep-me')", [])
                .unwrap();
            db.execute("INSERT INTO asset_tags VALUES('asset','keep-me')", [])
                .unwrap();
        }
        let library = Library::open(&root).unwrap();
        let snapshot = library.get_snapshot("s1").unwrap();
        assert!(snapshot.storage.legacy);
        assert_eq!(snapshot.storage.kind, "full");
        assert_eq!(
            library.get_asset("asset").unwrap().asset.tags,
            vec!["keep-me"]
        );
        let raw = library.store.object_path(&hash).unwrap();
        assert!(raw.exists());
        let plan = library
            .preview_storage_action(
                "variant",
                StorageAction::Recompress,
                None,
                &OperationContext::default(),
            )
            .unwrap();
        assert!(plan.reclaimable_bytes > 900_000);
        let done = library
            .apply_storage_action(
                "variant",
                StorageAction::Recompress,
                None,
                &plan.plan_token,
                &OperationContext::default(),
            )
            .unwrap();
        assert_eq!(done.released_bytes, plan.reclaimable_bytes);
        assert!(!raw.exists());
        library
            .materialize(
                "s1",
                &temp.path().join("restored"),
                &OperationContext::default(),
            )
            .unwrap();
        assert_eq!(
            fs::read(temp.path().join("restored/main.blend")).unwrap(),
            bytes
        );
        assert!(
            library
                .preview_storage_action(
                    "variant",
                    StorageAction::Compact,
                    None,
                    &OperationContext::default()
                )
                .unwrap()
                .versions
                .is_empty()
        );
        drop(library);
        let backup = fs::read_dir(root.join("backups"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let db = Connection::open(backup).unwrap();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            db.query_row("SELECT name FROM tags", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "keep-me"
        );
        assert_eq!(
            Connection::open(root.join("library.sqlite3"))
                .unwrap()
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            SCHEMA_VERSION
        );
    }
}
