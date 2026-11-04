//! Explicit deletion of archived assets, with recoverable filesystem publication.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveDeletionAsset {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveDeletionPlan {
    pub plan_token: String,
    pub assets: Vec<ArchiveDeletionAsset>,
    pub asset_ids: Vec<String>,
    pub variant_ids: Vec<String>,
    pub asset_count: u64,
    pub variant_count: u64,
    pub snapshot_count: u64,
    pub workspace_bytes: u64,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveDeletionReport {
    pub deleted_asset_ids: Vec<String>,
    pub deleted_variant_ids: Vec<String>,
    pub released_bytes: u64,
    pub cleanup_pending: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Target {
    asset_id: String,
    workspace: String,
    variant_ids: Vec<String>,
    backup_hashes: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct DeletionJournal {
    version: u32,
    id: String,
    targets: Vec<Target>,
    candidates: BTreeMap<String, u64>,
}

struct Prepared {
    plan: ArchiveDeletionPlan,
    targets: Vec<Target>,
    candidates: BTreeMap<String, u64>,
    entries: BTreeMap<String, Vec<ManifestEntry>>,
}

fn ordinary_file(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 {
                    return Err(CoreError::Invalid(
                        "Lien interdit dans les fichiers à supprimer.".into(),
                    ));
                }
            }
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(CoreError::Invalid(
                    "Fichier ordinaire requis pour la suppression.".into(),
                ));
            }
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn known_hash(value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(CoreError::Inconsistent(
            "Hash de suppression invalide.".into(),
        ));
    }
    Ok(())
}

pub(super) fn ensure_no_deletion(root: &Path) -> Result<()> {
    for entry in fs::read_dir(root.join("staging"))? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with("delete-") {
            return Err(CoreError::Busy("Une suppression attend son nettoyage. Fermez les fichiers concernés puis rouvrez la bibliothèque.".into()));
        }
    }
    Ok(())
}

impl Library {
    pub fn register_glb_backup(&self, variant_id: &str, hash: &str) -> Result<()> {
        known_hash(hash)?;
        let _guard = self.write_guard()?;
        let db = self.db()?;
        require_id(&db, "variants", variant_id)?;
        db.execute(
            "INSERT OR IGNORE INTO glb_backup_owners(variant_id,hash) VALUES(?1,?2)",
            params![variant_id, hash],
        )?;
        Ok(())
    }

    /// Removes the category subtree, archiving its assets without touching their files.
    pub fn delete_category(&self, id: &str) -> Result<u64> {
        let _guard = self.write_guard()?;
        let mut db = self.db()?;
        let tx = db.transaction()?;
        require_id(&tx, "categories", id)?;
        let ids: Vec<String> = tx.prepare("WITH RECURSIVE descendants(id) AS (SELECT id FROM categories WHERE id=?1 UNION SELECT c.id FROM categories c JOIN descendants d ON c.parent_id=d.id) SELECT id FROM descendants")?
            .query_map([id], |row| row.get(0))?.collect::<std::result::Result<_,_>>()?;
        let mut affected = 0;
        for category in &ids {
            affected += tx.execute(
                "UPDATE assets SET archived=1,category_id=NULL,updated_at=?2 WHERE category_id=?1",
                params![category, now()],
            )? as u64;
            tx.execute(
                "UPDATE categories SET parent_id=NULL WHERE id=?1",
                [category],
            )?;
        }
        for category in &ids {
            tx.execute("DELETE FROM categories WHERE id=?1", [category])?;
        }
        tx.commit()?;
        Ok(affected)
    }

    /// Archives every linked asset, retaining its links to other projects.
    pub fn delete_game(&self, id: &str) -> Result<u64> {
        let _guard = self.write_guard()?;
        let mut db = self.db()?;
        let tx = db.transaction()?;
        require_id(&tx, "games", id)?;
        let affected = tx.execute("UPDATE assets SET archived=1,updated_at=?2 WHERE id IN (SELECT asset_id FROM asset_games WHERE game_id=?1)", params![id,now()])?;
        tx.execute("DELETE FROM asset_games WHERE game_id=?1", [id])?;
        tx.execute("DELETE FROM games WHERE id=?1", [id])?;
        tx.commit()?;
        Ok(affected as u64)
    }

    pub fn plan_archive_deletion(
        &self,
        asset_id: Option<&str>,
        ctx: &OperationContext,
    ) -> Result<ArchiveDeletionPlan> {
        let _guard = self.write_guard()?;
        self.check_storage_integrity()?;
        Ok(self.prepare_archive_deletion(asset_id, ctx)?.plan)
    }

    fn prepare_archive_deletion(
        &self,
        asset_id: Option<&str>,
        ctx: &OperationContext,
    ) -> Result<Prepared> {
        ctx.check_cancelled()?;
        let mut targets = Vec::new();
        let mut entries = BTreeMap::new();
        let mut candidates = BTreeMap::new();
        let mut state = Vec::new();
        let mut plan = ArchiveDeletionPlan {
            plan_token: String::new(),
            assets: vec![],
            asset_ids: vec![],
            variant_ids: vec![],
            asset_count: 0,
            variant_count: 0,
            snapshot_count: 0,
            workspace_bytes: 0,
            warnings: vec![],
        };
        let db = self.db()?;
        if let Some(id) = asset_id {
            require_id(&db, "assets", id)?;
            let archived: bool =
                db.query_row("SELECT archived FROM assets WHERE id=?1", [id], |r| {
                    r.get(0)
                })?;
            if !archived {
                return Err(CoreError::Invalid(
                    "Seul un asset archivé peut être supprimé définitivement.".into(),
                ));
            }
        }
        let assets: Vec<(String,String,String)> = db.prepare("SELECT id,name,updated_at FROM assets WHERE archived=1 AND (?1 IS NULL OR id=?1) ORDER BY id")?
            .query_map([asset_id], |row|Ok((row.get(0)?,row.get(1)?,row.get(2)?)))?.collect::<std::result::Result<_,_>>()?;
        for (id, name, updated) in assets {
            ctx.check_cancelled()?;
            safe_relative(&id)?;
            if Path::new(&id).components().count() != 1 {
                return Err(CoreError::Inconsistent(
                    "Identifiant d’asset invalide.".into(),
                ));
            }
            let workspace = format!("workspaces/{id}");
            let variants: Vec<(String,String,String,String,Option<String>)> = db.prepare("SELECT id,workspace,config,head_snapshot_id,backup_snapshot_id FROM variants WHERE asset_id=?1 ORDER BY id")?
                .query_map([&id], |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?.collect::<std::result::Result<_,_>>()?;
            for (variant, relative, ..) in &variants {
                if relative != &format!("{workspace}/{variant}") {
                    return Err(CoreError::Inconsistent(
                        "Le workspace ne correspond pas à son asset ; suppression refusée.".into(),
                    ));
                }
            }
            let foreign: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM snapshots s JOIN variants v ON v.id=s.variant_id WHERE v.asset_id<>?1 AND (s.parent_id IN (SELECT x.id FROM snapshots x JOIN variants a ON a.id=x.variant_id WHERE a.asset_id=?1) OR s.restored_from IN (SELECT x.id FROM snapshots x JOIN variants a ON a.id=x.variant_id WHERE a.asset_id=?1))) OR EXISTS(SELECT 1 FROM variants v WHERE v.asset_id<>?1 AND (v.head_snapshot_id IN (SELECT x.id FROM snapshots x JOIN variants a ON a.id=x.variant_id WHERE a.asset_id=?1) OR v.backup_snapshot_id IN (SELECT x.id FROM snapshots x JOIN variants a ON a.id=x.variant_id WHERE a.asset_id=?1)))", [&id], |r|r.get(0))?;
            if foreign {
                return Err(CoreError::Inconsistent(
                    "Une version est référencée par un autre asset ; aucun fichier supprimé."
                        .into(),
                ));
            }
            let snapshot_ids: Vec<String> = db.prepare("SELECT s.id FROM snapshots s JOIN variants v ON v.id=s.variant_id WHERE v.asset_id=?1 ORDER BY s.id")?
                .query_map([&id], |r|r.get(0))?.collect::<std::result::Result<_,_>>()?;
            let mut snapshots = Vec::new();
            for snapshot in snapshot_ids {
                let value = read_snapshot(&db, &snapshot)?;
                for entry in &value.entries {
                    if let Some(hash) = &entry.hash {
                        known_hash(hash)?;
                        candidates.insert(hash.clone(), entry.size);
                    }
                }
                snapshots.push(value);
            }
            let backup_hashes: Vec<String> = db.prepare("SELECT DISTINCT b.hash FROM glb_backup_owners b JOIN variants v ON v.id=b.variant_id WHERE v.asset_id=?1 ORDER BY b.hash")?
                .query_map([&id],|r|r.get(0))?.collect::<std::result::Result<_,_>>()?;
            for hash in &backup_hashes {
                known_hash(hash)?;
            }
            state.push(serde_json::json!([
                id,
                name,
                updated,
                variants,
                snapshots,
                backup_hashes
            ]));
            plan.snapshot_count += snapshots.len() as u64;
            let variant_ids: Vec<String> = variants.into_iter().map(|v| v.0).collect();
            plan.variant_ids.extend(variant_ids.clone());
            plan.asset_ids.push(id.clone());
            plan.assets.push(ArchiveDeletionAsset {
                id: id.clone(),
                name,
            });
            targets.push(Target {
                asset_id: id,
                workspace,
                variant_ids,
                backup_hashes,
            });
        }
        drop(db);
        ensure_real_directory(&self.root.join("workspaces"))?;
        for target in &targets {
            let path = self.root.join(safe_relative(&target.workspace)?);
            let manifest = match fs::symlink_metadata(&path) {
                Ok(_) => self.store.capture(&path, false, ctx)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => vec![],
                Err(error) => return Err(error.into()),
            };
            plan.workspace_bytes = plan
                .workspace_bytes
                .saturating_add(manifest.iter().map(|e| e.size).sum::<u64>());
            entries.insert(target.asset_id.clone(), manifest);
        }
        plan.asset_count = targets.len() as u64;
        plan.variant_count = plan.variant_ids.len() as u64;
        plan.warnings = legacy_backup_warnings(&self.root, &*self.db()?)?;
        plan.plan_token = blake3::hash(&serde_json::to_vec(&(
            self.root.to_string_lossy(),
            asset_id,
            state,
            &entries,
        ))?)
        .to_hex()
        .to_string();
        Ok(Prepared {
            plan,
            targets,
            candidates,
            entries,
        })
    }

    pub fn apply_archive_deletion(
        &self,
        asset_id: Option<&str>,
        plan_token: &str,
        ctx: &OperationContext,
    ) -> Result<ArchiveDeletionReport> {
        let _guard = self.write_guard()?;
        self.check_storage_integrity()?;
        let prepared = self.prepare_archive_deletion(asset_id, ctx)?;
        if prepared.plan.plan_token != plan_token {
            return Err(CoreError::Busy(
                "Les archives ou leurs fichiers ont changé. Vérifiez à nouveau la confirmation."
                    .into(),
            ));
        }
        let mut report = ArchiveDeletionReport {
            deleted_asset_ids: prepared.plan.asset_ids,
            deleted_variant_ids: prepared.plan.variant_ids,
            released_bytes: 0,
            cleanup_pending: false,
            warnings: prepared.plan.warnings,
        };
        if prepared.targets.is_empty() {
            return Ok(report);
        }
        let id = new_id();
        let directory = self.store.staging.join(format!("delete-{id}"));
        let preparing = tempfile::Builder::new()
            .prefix(".delete-preparing-")
            .tempdir_in(&self.store.staging)?;
        let journal = DeletionJournal {
            version: 1,
            id,
            targets: prepared.targets,
            candidates: prepared.candidates,
        };
        let journal_bytes = serde_json::to_vec(&journal)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(preparing.path().join("delete.json"))?;
        file.write_all(&journal_bytes)?;
        file.sync_all()?;
        drop(file);
        ctx.check_cancelled()?;
        fs::rename(preparing.path(), &directory)?;
        ctx.report("Suppression : journal publié", 1, Some(4));
        let result = (|| -> Result<()> {
            // Readers cannot observe renamed workspaces with an uncommitted catalogue.
            let mut db = self.db()?;
            let tx = db.transaction()?;
            for target in &journal.targets {
                ctx.check_cancelled()?;
                let source = self.root.join(safe_relative(&target.workspace)?);
                let destination = directory.join(&target.asset_id);
                if source.exists() {
                    ensure_real_directory(&source)?;
                    #[cfg(windows)]
                    for entry in &prepared.entries[&target.asset_id] {
                        if entry.kind == "file" {
                            use std::os::windows::fs::OpenOptionsExt;
                            let _handle = OpenOptions::new()
                                .read(true)
                                .share_mode(0)
                                .open(source.join(safe_relative(&entry.path)?))?;
                        }
                    }
                    fs::rename(source, &destination)?;
                    if self.store.capture(&destination, false, ctx)?
                        != prepared.entries[&target.asset_id]
                    {
                        return Err(CoreError::Busy(
                            "Les fichiers ont changé pendant la suppression.".into(),
                        ));
                    }
                }
            }
            ctx.report("Suppression : dossiers préservés", 2, Some(4));
            ctx.check_cancelled()?;
            for (hash, size) in &journal.candidates {
                tx.execute(
                    "INSERT OR IGNORE INTO garbage_candidates(hash,size) VALUES(?1,?2)",
                    params![hash, sql_size(*size)?],
                )?;
            }
            for target in &journal.targets {
                tx.execute(
                    "UPDATE variants SET backup_snapshot_id=NULL WHERE asset_id=?1",
                    [&target.asset_id],
                )?;
                tx.execute("UPDATE snapshots SET parent_id=NULL,restored_from=NULL WHERE variant_id IN (SELECT id FROM variants WHERE asset_id=?1)",[&target.asset_id])?;
                tx.execute("DELETE FROM snapshot_entries WHERE snapshot_id IN (SELECT s.id FROM snapshots s JOIN variants v ON v.id=s.variant_id WHERE v.asset_id=?1)",[&target.asset_id])?;
                tx.execute("DELETE FROM snapshots WHERE variant_id IN (SELECT id FROM variants WHERE asset_id=?1)",[&target.asset_id])?;
                tx.execute("DELETE FROM glb_backup_owners WHERE variant_id IN (SELECT id FROM variants WHERE asset_id=?1)",[&target.asset_id])?;
                tx.execute("DELETE FROM variants WHERE asset_id=?1", [&target.asset_id])?;
                tx.execute(
                    "DELETE FROM asset_tags WHERE asset_id=?1",
                    [&target.asset_id],
                )?;
                tx.execute(
                    "DELETE FROM asset_games WHERE asset_id=?1",
                    [&target.asset_id],
                )?;
                tx.execute("DELETE FROM assets WHERE id=?1", [&target.asset_id])?;
            }
            tx.execute(
                "INSERT INTO archive_deletions(id,journal_hash) VALUES(?1,?2)",
                params![
                    journal.id,
                    blake3::hash(&journal_bytes).to_hex().to_string()
                ],
            )?;
            tx.commit()?;
            Ok(())
        })();
        if let Err(error) = result {
            recover_one(&self.root, &mut *self.db()?, &directory)?;
            return Err(error);
        }
        ctx.report("Suppression : catalogue publié", 3, Some(4));
        match recover_one(&self.root, &mut *self.db()?, &directory) {
            Ok(bytes) => report.released_bytes = bytes,
            Err(error) => {
                report.cleanup_pending = true;
                report.warnings.push(format!(
                    "Assets supprimés ; nettoyage à reprendre à la réouverture : {error}"
                ));
            }
        }
        ctx.report(
            if report.cleanup_pending {
                "Suppression : nettoyage à reprendre"
            } else {
                "Suppression : nettoyage terminé"
            },
            4,
            Some(4),
        );
        Ok(report)
    }
}

fn legacy_backup_warnings(root: &Path, db: &Connection) -> Result<Vec<String>> {
    let directory = root.join("backups/glb");
    if !directory.exists() {
        return Ok(vec![]);
    }
    ensure_real_directory(&root.join("backups"))?;
    ensure_real_directory(&directory)?;
    let mut count = 0;
    for item in fs::read_dir(directory)? {
        let path = item?.path();
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("glb"))
            && ordinary_file(&path)?
        {
            let hash = path.file_stem().and_then(|n| n.to_str()).unwrap_or("");
            let owned: bool = db.query_row(
                "SELECT EXISTS(SELECT 1 FROM glb_backup_owners WHERE hash=?1)",
                [hash],
                |r| r.get(0),
            )?;
            if !owned {
                count += 1;
            }
        }
    }
    Ok(if count == 0 {
        vec![]
    } else {
        vec![format!(
            "{count} sauvegarde(s) GLB ancienne(s) sans propriétaire identifié seront conservées."
        )]
    })
}

fn read_journal(root: &Path, directory: &Path) -> Result<DeletionJournal> {
    ensure_real_directory(directory)?;
    let path = directory.join("delete.json");
    if !ordinary_file(&path)? || fs::metadata(&path)?.len() > 64 * 1024 * 1024 {
        return Err(CoreError::Inconsistent(
            "Journal de suppression invalide.".into(),
        ));
    }
    let journal: DeletionJournal = serde_json::from_slice(&fs::read(path)?)?;
    if journal.version != 1
        || directory.file_name().and_then(|n| n.to_str()) != Some(&format!("delete-{}", journal.id))
        || directory.parent() != Some(root.join("staging").as_path())
        || journal.targets.is_empty()
    {
        return Err(CoreError::Inconsistent(
            "Journal de suppression incohérent.".into(),
        ));
    }
    let mut ids = BTreeSet::new();
    for target in &journal.targets {
        if safe_relative(&target.asset_id)?.components().count() != 1
            || target.workspace != format!("workspaces/{}", target.asset_id)
            || !ids.insert(&target.asset_id)
        {
            return Err(CoreError::Inconsistent(
                "Propriétaire de suppression invalide.".into(),
            ));
        }
        for hash in &target.backup_hashes {
            known_hash(hash)?;
        }
    }
    for hash in journal.candidates.keys() {
        known_hash(hash)?;
    }
    Ok(journal)
}

fn recover_one(root: &Path, db: &mut Connection, directory: &Path) -> Result<u64> {
    ensure_real_directory(directory)?;
    if !directory.join("delete.json").exists() && fs::read_dir(directory)?.next().is_none() {
        // The final directory removal can be interrupted after the journal unlink.
        fs::remove_dir(directory)?;
        if let Some(id) = directory
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_prefix("delete-"))
        {
            db.execute("DELETE FROM archive_deletions WHERE id=?1", [id])?;
        }
        return Ok(0);
    }
    let journal = read_journal(root, directory)?;
    let mut present = 0;
    for target in &journal.targets {
        if db.query_row(
            "SELECT EXISTS(SELECT 1 FROM assets WHERE id=?1)",
            [&target.asset_id],
            |r| r.get::<_, bool>(0),
        )? {
            present += 1;
        }
    }
    if present != 0 && present != journal.targets.len() {
        return Err(CoreError::Inconsistent(
            "Suppression partiellement publiée : fichiers préservés.".into(),
        ));
    }
    let receipt: Option<String> = db
        .query_row(
            "SELECT journal_hash FROM archive_deletions WHERE id=?1",
            [&journal.id],
            |r| r.get(0),
        )
        .optional()?;
    if present == 0 {
        let digest = blake3::hash(&serde_json::to_vec(&journal)?)
            .to_hex()
            .to_string();
        if receipt.as_deref() != Some(&digest) {
            return Err(CoreError::Inconsistent(
                "Journal de suppression modifié ou sans reçu du catalogue ; fichiers préservés."
                    .into(),
            ));
        }
    } else {
        if receipt.is_some() {
            return Err(CoreError::Inconsistent(
                "Reçu de suppression incohérent avec les assets présents.".into(),
            ));
        }
        for target in &journal.targets {
            let variants: Vec<(String, String)> = db
                .prepare("SELECT id,workspace FROM variants WHERE asset_id=?1 ORDER BY id")?
                .query_map([&target.asset_id], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<std::result::Result<_, _>>()?;
            if variants.iter().map(|v| &v.0).collect::<Vec<_>>()
                != target.variant_ids.iter().collect::<Vec<_>>()
                || variants
                    .iter()
                    .any(|(id, path)| path != &format!("{}/{id}", target.workspace))
            {
                return Err(CoreError::Inconsistent(
                    "Le journal ne correspond plus aux workspaces du catalogue.".into(),
                ));
            }
        }
    }
    let mut released = 0;
    ensure_real_directory(&root.join("workspaces"))?;
    if present == 0 {
        for target in &journal.targets {
            let workspace = root.join(safe_relative(&target.workspace)?);
            match fs::symlink_metadata(&workspace) {
                Ok(_) => {
                    return Err(CoreError::Inconsistent(format!(
                        "Un nouveau dossier est présent après la suppression ; fichiers préservés pour récupération : {}",
                        workspace.display()
                    )));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    for target in &journal.targets {
        let previous = directory.join(&target.asset_id);
        let workspace = root.join(safe_relative(&target.workspace)?);
        if present > 0 {
            if previous.exists() {
                ensure_real_directory(&previous)?;
                if workspace.exists() {
                    return Err(CoreError::Inconsistent(
                        "Deux dossiers de travail présents ; récupération manuelle requise.".into(),
                    ));
                }
                fs::rename(previous, workspace)?;
            }
        } else if previous.exists() {
            ensure_real_directory(&previous)?;
            // Validate the complete tree before recursive deletion; never follow a junction.
            list_files(&previous)?;
            released += directory_bytes(&previous)?;
            fs::remove_dir_all(previous)?;
        }
    }
    if present == 0 {
        released += cleanup_caches(root, db, &journal.targets)?;
        let store = FileStore {
            objects: root.join("objects"),
            staging: root.join("staging"),
        };
        for hash in journal.candidates.keys() {
            let referenced: bool = db.query_row(
                "SELECT EXISTS(SELECT 1 FROM snapshot_entries WHERE hash=?1)",
                [hash],
                |r| r.get(0),
            )?;
            if !referenced {
                let path = store.object_path(hash)?;
                for file in [&path, &path.with_extension("zst")] {
                    released += remove_file_if_regular(file)?;
                }
                db.execute("DELETE FROM garbage_candidates WHERE hash=?1", [hash])?;
            }
        }
        for hash in journal
            .targets
            .iter()
            .flat_map(|t| &t.backup_hashes)
            .collect::<BTreeSet<_>>()
        {
            let referenced: bool = db.query_row(
                "SELECT EXISTS(SELECT 1 FROM glb_backup_owners WHERE hash=?1)",
                [hash],
                |r| r.get(0),
            )?;
            if !referenced {
                let backups = root.join("backups/glb");
                if backups.exists() {
                    ensure_real_directory(&root.join("backups"))?;
                    ensure_real_directory(&backups)?;
                    released += remove_file_if_regular(&backups.join(format!("{hash}.glb")))?;
                }
            }
        }
    }
    fs::remove_file(directory.join("delete.json"))?;
    fs::remove_dir(directory)?;
    db.execute("DELETE FROM archive_deletions WHERE id=?1", [&journal.id])?;
    Ok(released)
}

fn remove_file_if_regular(path: &Path) -> Result<u64> {
    if !ordinary_file(path)? {
        return Ok(0);
    }
    let bytes = fs::metadata(path)?.len();
    fs::remove_file(path)?;
    Ok(bytes)
}

pub(super) fn recover_deletions(root: &Path, db: &mut Connection) -> Result<Vec<String>> {
    let mut notices = Vec::new();
    for entry in fs::read_dir(root.join("staging"))? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with("delete-")
            && let Err(error) = recover_one(root, db, &entry.path())
        {
            notices.push(format!("Suppression à récupérer : {error}"));
        }
    }
    let receipts: Vec<String> = db
        .prepare("SELECT id FROM archive_deletions")?
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    for id in receipts {
        if safe_relative(&id)?.components().count() != 1 {
            continue;
        }
        if !root.join("staging").join(format!("delete-{id}")).exists() {
            db.execute("DELETE FROM archive_deletions WHERE id=?1", [id])?;
        }
    }
    Ok(notices)
}

/// Cache files are disposable, but shared images/artifacts retain their live indexes.
fn cleanup_caches(root: &Path, _db: &Connection, targets: &[Target]) -> Result<u64> {
    let cache = root.join("cache");
    if !cache.exists() {
        return Ok(0);
    }
    ensure_real_directory(&cache)?;
    let variants: Vec<_> = targets.iter().flat_map(|t| &t.variant_ids).collect();
    let mut released = 0;
    for folder in ["sync", "previews", "thumbnails", "canonical", "exports"] {
        let directory = cache.join(folder);
        if !directory.exists() {
            continue;
        }
        ensure_real_directory(&directory)?;
        if matches!(folder, "sync" | "previews" | "thumbnails") {
            for variant in &variants {
                let key = blake3::hash(variant.as_bytes()).to_hex().to_string();
                let name = match folder {
                    "sync" => format!("{key}.json"),
                    "previews" => format!("choice-{key}.json"),
                    _ => format!("index-{key}.json"),
                };
                released += remove_file_if_regular(&directory.join(name))?;
                if folder == "previews" {
                    let prefix = format!("choice-{key}-");
                    for item in fs::read_dir(&directory)? {
                        let path = item?.path();
                        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                        if name
                            .strip_prefix(&prefix)
                            .and_then(|name| name.strip_suffix(".json"))
                            .is_some_and(|hash| known_hash(hash).is_ok())
                        {
                            released += remove_file_if_regular(&path)?;
                        }
                    }
                }
            }
        }
        if folder == "previews" {
            for item in fs::read_dir(&directory)? {
                let path = item?.path();
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !name.starts_with("index-")
                    || !name.ends_with(".json")
                    || !ordinary_file(&path)?
                    || fs::metadata(&path)?.len() > 4 * 1024 * 1024
                {
                    continue;
                }
                let Ok(record) = serde_json::from_slice::<serde_json::Value>(&fs::read(&path)?)
                else {
                    continue;
                };
                let Some(source) = record["sourceFile"].as_str() else {
                    continue;
                };
                let owned = targets.iter().any(|t| {
                    t.variant_ids.iter().any(|v| {
                        let primary = root.join(&t.workspace).join(v).join(source);
                        record["sources"].as_array().is_some_and(|sources| {
                            sources.iter().any(|s| {
                                s["path"].as_str().is_some_and(|p| {
                                    dunce::simplified(Path::new(p)) == dunce::simplified(&primary)
                                })
                            })
                        })
                    })
                });
                if owned {
                    released += remove_file_if_regular(&path)?;
                }
            }
        }
    }
    // Compute references after removing owner indexes. Unknown metadata is preserved.
    let mut live = BTreeSet::new();
    for (folder, key) in [
        ("sync", "artifact_hash"),
        ("previews", "artifact"),
        ("thumbnails", "artifact_key"),
    ] {
        let directory = cache.join(folder);
        if !directory.exists() {
            continue;
        }
        for item in fs::read_dir(directory)? {
            let path = item?.path();
            if path.extension().is_some_and(|e| e == "json")
                && ordinary_file(&path)?
                && fs::metadata(&path)?.len() <= 4 * 1024 * 1024
                && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&fs::read(&path)?)
            {
                // Preview artifact keys and binary hashes differ. Preserve every
                // validated live key; older records may include a .glb suffix.
                for name in [
                    key,
                    "artifactHash",
                    "artifactKey",
                    "artifact_hash",
                    "artifact_key",
                ] {
                    if let Some(value) = value[name].as_str() {
                        let hash = value.strip_suffix(".glb").unwrap_or(value);
                        if known_hash(hash).is_ok() {
                            live.insert(hash.to_owned());
                        }
                    }
                }
            }
        }
    }
    for folder in ["canonical", "previews", "thumbnails", "exports"] {
        let directory = cache.join(folder);
        if !directory.exists() {
            continue;
        }
        for item in fs::read_dir(&directory)? {
            let path = item?.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let Some(hash) = name.get(..64).filter(|h| known_hash(h).is_ok()) else {
                continue;
            };
            let generated = match folder {
                "thumbnails" => name[64..].starts_with("-v") && name.ends_with(".png"),
                _ => name.len() == 68 && name.ends_with(".glb"),
            };
            if generated && !live.contains(hash) {
                released += remove_file_if_regular(&path)?;
                if folder == "exports" {
                    released += remove_file_if_regular(&path.with_extension("json"))?;
                }
            }
        }
    }
    Ok(released)
}
