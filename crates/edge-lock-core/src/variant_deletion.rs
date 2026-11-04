//! Variant deletion retains immutable ancestry while retiring exclusive contents.
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariantDeletionPlan {
    pub plan_token: String,
    pub variant_id: String,
    pub variant_name: String,
    pub snapshot_count: u64,
    pub retained_snapshot_count: u64,
    pub workspace_bytes: u64,
    pub workspace_bytes_complete: bool,
    pub warnings: Vec<String>,
}

struct PreparedVariant {
    plan: VariantDeletionPlan,
    target: Target,
    retired: Vec<String>,
    candidates: BTreeMap<String, u64>,
}

#[derive(Serialize)]
struct VariantState {
    id: String,
    name: String,
    config: String,
    head_snapshot_id: String,
    backup_snapshot_id: Option<String>,
    deleted_name: Option<String>,
}

impl Library {
    pub fn plan_variant_deletion(
        &self,
        variant_id: &str,
        ctx: &OperationContext,
    ) -> Result<VariantDeletionPlan> {
        let _guard = self.write_guard()?;
        self.check_storage_integrity()?;
        Ok(self.prepare_variant_deletion(variant_id, ctx)?.plan)
    }

    fn prepare_variant_deletion(
        &self,
        variant_id: &str,
        ctx: &OperationContext,
    ) -> Result<PreparedVariant> {
        ctx.check_cancelled()?;
        let db = self.db()?;
        let variant = read_variant(&db, &self.root, variant_id)?;
        ensure_asset_history_scope(&db, &variant.asset_id)?;
        let active_count: i64 = db.query_row(
            "SELECT COUNT(*) FROM variants WHERE asset_id=?1 AND deleted_name IS NULL",
            [&variant.asset_id],
            |row| row.get(0),
        )?;
        if active_count <= 1 {
            return Err(CoreError::Invalid("La dernière variante d’un asset ne peut pas être supprimée. Archivez l’asset pour le supprimer.".into()));
        }
        let workspace: String = db.query_row(
            "SELECT workspace FROM variants WHERE id=?1",
            [variant_id],
            |row| row.get(0),
        )?;
        if safe_relative(&variant.asset_id)?.components().count() != 1
            || safe_relative(variant_id)?.components().count() != 1
            || workspace != format!("workspaces/{}/{variant_id}", variant.asset_id)
        {
            return Err(CoreError::Inconsistent(
                "Le workspace ne correspond pas à sa variante ; suppression refusée.".into(),
            ));
        }

        // Start from every surviving owner's versions and backups, then retain
        // both ancestry links. UNION also terminates on damaged cyclic metadata.
        let retained: BTreeSet<String> = db.prepare("WITH RECURSIVE retained(id) AS (
            SELECT s.id FROM snapshots s JOIN variants v ON v.id=s.variant_id WHERE v.asset_id=?2 AND v.deleted_name IS NULL AND v.id<>?1
            UNION SELECT backup_snapshot_id FROM variants WHERE asset_id=?2 AND deleted_name IS NULL AND id<>?1 AND backup_snapshot_id IS NOT NULL
            UNION SELECT s.parent_id FROM snapshots s JOIN retained r ON r.id=s.id WHERE s.parent_id IS NOT NULL
            UNION SELECT s.restored_from FROM snapshots s JOIN retained r ON r.id=s.id WHERE s.restored_from IS NOT NULL
        ) SELECT id FROM retained")?.query_map(params![variant_id, variant.asset_id], |row| row.get(0))?.collect::<std::result::Result<_, _>>()?;
        let owners: Vec<VariantState> = db.prepare("SELECT id,name,config,head_snapshot_id,backup_snapshot_id,deleted_name FROM variants WHERE asset_id=?1 ORDER BY id")?
            .query_map([&variant.asset_id], |row| Ok(VariantState { id:row.get(0)?,name:row.get(1)?,config:row.get(2)?,head_snapshot_id:row.get(3)?,backup_snapshot_id:row.get(4)?,deleted_name:row.get(5)? }))?
            .collect::<std::result::Result<_, _>>()?;
        let removed_owners: BTreeSet<_> = owners
            .iter()
            .filter(|owner| owner.id == variant_id || owner.deleted_name.is_some())
            .map(|owner| owner.id.as_str())
            .collect();
        let ids: Vec<String> = db.prepare("SELECT s.id FROM snapshots s JOIN variants v ON v.id=s.variant_id WHERE v.asset_id=?1 ORDER BY s.id")?
            .query_map([&variant.asset_id], |row| row.get(0))?.collect::<std::result::Result<_, _>>()?;
        let mut snapshots = Vec::with_capacity(ids.len());
        let mut retired = Vec::new();
        let mut candidates = BTreeMap::new();
        let mut retained_snapshot_count = 0;
        for id in ids {
            ctx.check_cancelled()?;
            let snapshot = read_snapshot(&db, &id)?;
            if snapshot.variant_id == variant_id
                && retained.contains(&id)
                && snapshot.storage.kind != "deleted"
            {
                retained_snapshot_count += 1;
            }
            if removed_owners.contains(snapshot.variant_id.as_str())
                && !retained.contains(&id)
                && snapshot.storage.kind != "deleted"
            {
                for entry in &snapshot.entries {
                    if let Some(hash) = &entry.hash {
                        known_hash(hash)?;
                        candidates.insert(hash.clone(), entry.size);
                    }
                }
                retired.push(id);
            }
            snapshots.push(snapshot);
        }
        let backup_hashes = db
            .prepare("SELECT hash FROM glb_backup_owners WHERE variant_id=?1 ORDER BY hash")?
            .query_map([variant_id], |row| row.get(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        for hash in &backup_hashes {
            known_hash(hash)?;
        }
        let mut target = Target {
            asset_id: variant.asset_id.clone(),
            variant_id: Some(variant_id.into()),
            workspace,
            variant_ids: vec![variant_id.into()],
            backup_hashes,
            workspace_evidence: None,
        };
        drop(db);
        ensure_real_directory(&self.root.join("workspaces"))?;
        ensure_real_directory(&self.root.join("workspaces").join(&target.asset_id))?;
        let mut plan = VariantDeletionPlan {
            plan_token: String::new(),
            variant_id: variant_id.into(),
            variant_name: variant.name,
            snapshot_count: retired.len() as u64,
            retained_snapshot_count,
            workspace_bytes: 0,
            workspace_bytes_complete: true,
            warnings: vec![],
        };
        let path = self.root.join(&target.workspace);
        match WorkspaceEvidence::capture(&path) {
            Ok(evidence) => {
                plan.workspace_bytes = evidence.bytes();
                target.workspace_evidence = Some(evidence);
            }
            Err(CoreError::Io(error))
                if error.kind() == std::io::ErrorKind::NotFound
                    && fs::symlink_metadata(&path)
                        .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound) => {}
            Err(error) => {
                plan.workspace_bytes_complete = false;
                plan.warnings.push(format!("Workspace non vérifiable ; fichiers conservés pour récupération : {} ({error})", path.display()));
            }
        }
        plan.plan_token = blake3::hash(&serde_json::to_vec(&(
            self.root.to_string_lossy(),
            &owners,
            snapshots,
            &target,
            &retired,
        ))?)
        .to_hex()
        .to_string();
        Ok(PreparedVariant {
            plan,
            target,
            retired,
            candidates,
        })
    }

    pub fn apply_variant_deletion(
        &self,
        variant_id: &str,
        plan_token: &str,
        ctx: &OperationContext,
    ) -> Result<ArchiveDeletionReport> {
        let _guard = self.write_guard()?;
        self.check_storage_integrity()?;
        let prepared = self.prepare_variant_deletion(variant_id, ctx)?;
        if prepared.plan.plan_token != plan_token {
            return Err(CoreError::Busy("La variante, son historique ou ses fichiers ont changé. Vérifiez à nouveau la confirmation.".into()));
        }
        let report = ArchiveDeletionReport {
            deleted_asset_ids: vec![],
            deleted_variant_ids: vec![variant_id.into()],
            released_bytes: 0,
            cleanup_pending: false,
            warnings: prepared.plan.warnings,
        };
        let journal = DeletionJournal {
            version: 3,
            id: new_id(),
            targets: vec![prepared.target],
            candidates: prepared.candidates,
        };
        self.publish_deletion(journal, report, ctx, |tx, journal| {
            let target = &journal.targets[0];
            // User names cannot contain control characters. This internal name
            // frees the old unique name while retaining its immutable owner ID.
            let internal_name = format!("\u{1f}deleted:{variant_id}");
            tx.execute("UPDATE variants SET deleted_name=name,name=?2,backup_snapshot_id=NULL,working_bytes=0,triangles=NULL WHERE id=?1", params![variant_id, internal_name])?;
            tx.execute("DELETE FROM file_selections WHERE variant_id=?1", [variant_id])?;
            tx.execute("DELETE FROM glb_backup_owners WHERE variant_id=?1", [variant_id])?;
            for id in &prepared.retired {
                tx.execute("DELETE FROM file_selections WHERE context_key=?1", [id])?;
                tx.execute("DELETE FROM snapshot_entries WHERE snapshot_id=?1", [id])?;
                tx.execute("UPDATE snapshots SET storage_kind='deleted',preview_path=NULL WHERE id=?1", [id])?;
            }
            tx.execute("UPDATE assets SET updated_at=?2 WHERE id=?1", params![target.asset_id, now()])?;
            Ok(())
        })
    }
}
