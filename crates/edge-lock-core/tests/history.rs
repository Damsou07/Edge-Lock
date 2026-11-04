use edge_lock_core::{ExportConfig, Library, OperationContext, StorageAction};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

fn glb(path: &Path, label: &str) {
    let mut json=serde_json::to_vec(&serde_json::json!({"asset":{"version":"2.0","generator":label},"scenes":[{"nodes":[]}],"scene":0})).unwrap();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let mut bytes = b"glTF".to_vec();
    bytes.extend(2u32.to_le_bytes());
    bytes.extend((20 + json.len() as u32).to_le_bytes());
    bytes.extend((json.len() as u32).to_le_bytes());
    bytes.extend(0x4e4f534au32.to_le_bytes());
    bytes.extend(json);
    fs::write(path, bytes).unwrap();
}

fn fixture() -> (tempfile::TempDir, Library, String, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    fs::create_dir_all(source.join("references/empty")).unwrap();
    fs::write(source.join("main.blend"), vec![1; 128 * 1024]).unwrap();
    fs::write(source.join("script.py"), b"important reusable script").unwrap();
    glb(&source.join("main.glb"), "v1");
    let library = Library::open(temp.path().join("library")).unwrap();
    let asset = library
        .import_folder(
            &source,
            "Asset",
            ExportConfig::default(),
            &OperationContext::default(),
        )
        .unwrap();
    let variant = asset.variants[0].clone();
    (
        temp,
        library,
        variant.id,
        PathBuf::from(variant.workspace_path),
    )
}

fn version(library: &Library, id: &str, work: &Path, n: u8) -> edge_lock_core::Snapshot {
    fs::write(work.join("main.blend"), vec![n; 128 * 1024]).unwrap();
    glb(&work.join("main.glb"), &format!("v{n}"));
    library
        .create_snapshot(id, &format!("V{n}"), &OperationContext::default())
        .unwrap()
}

#[test]
fn compaction_keeps_every_autonomous_glb_and_its_exact_bytes() {
    let (temp, library, id, work) = fixture();
    let ctx = OperationContext::default();
    glb(&work.join("secondary.glb"), "secondary model");
    fs::write(work.join("secondary.blend"), b"second editable source").unwrap();
    let multiple = library
        .create_snapshot(&id, "Multiple models", &ctx)
        .unwrap();
    let expected = fs::read(work.join("secondary.glb")).unwrap();
    version(&library, &id, &work, 3);
    version(&library, &id, &work, 4);
    let compact = library.get_snapshot(&multiple.id).unwrap();
    assert_eq!(compact.storage.kind, "preview");
    assert_eq!(compact.entries.len(), 2);
    assert!(
        compact
            .entries
            .iter()
            .all(|entry| entry.path.ends_with(".glb"))
    );
    let output = temp.path().join("secondary-historical.glb");
    library
        .extract_snapshot_file(&multiple.id, "secondary.glb", &output, &ctx)
        .unwrap();
    assert_eq!(fs::read(output).unwrap(), expected);
    let backup = library
        .preview_storage_action(&id, StorageAction::ReleaseBackup, None, &ctx)
        .unwrap();
    let applied = library
        .apply_storage_action(
            &id,
            StorageAction::ReleaseBackup,
            None,
            &backup.plan_token,
            &ctx,
        )
        .unwrap();
    assert_eq!(backup.reclaimable_bytes, applied.released_bytes);
    let head = library.get_variant(&id).unwrap().variant;
    let preview = library.get_snapshot(&multiple.id).unwrap();
    assert_eq!(preview.entries.len(), 2);
    assert_eq!(head.backup_snapshot_id, None);
}

#[test]
fn global_actions_work_without_any_variant_and_keep_legacy_calls_compatible() {
    let empty = tempfile::tempdir().unwrap();
    let library = Library::open(empty.path().join("library")).unwrap();
    let ctx = OperationContext::default();
    for action in [StorageAction::Recompress, StorageAction::Purge] {
        let plan = library
            .preview_library_storage_action(action, &ctx)
            .unwrap();
        let applied = library
            .apply_library_storage_action(action, &plan.plan_token, &ctx)
            .unwrap();
        assert_eq!(applied.released_bytes, 0);
    }
    assert!(
        library
            .preview_library_storage_action(StorageAction::Compact, &ctx)
            .is_err()
    );

    let (temp, library, id, work) = fixture();
    let source = temp.path().join("other-source");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("main.blend"), vec![2; 128 * 1024]).unwrap();
    glb(&source.join("main.glb"), "other");
    let other = library
        .import_folder(&source, "Other", ExportConfig::default(), &ctx)
        .unwrap();
    for variant in [
        &library.get_variant(&id).unwrap().variant,
        &other.variants[0],
    ] {
        let snapshot = library.get_snapshot(&variant.head_snapshot_id).unwrap();
        let entry = snapshot
            .entries
            .iter()
            .find(|e| e.path == "main.blend")
            .unwrap();
        let hash = entry.hash.as_deref().unwrap();
        let raw = library.root().join("objects").join(&hash[..2]).join(hash);
        fs::write(
            &raw,
            fs::read(Path::new(&variant.workspace_path).join("main.blend")).unwrap(),
        )
        .unwrap();
        fs::remove_file(raw.with_extension("zst")).unwrap();
    }
    let plan = library
        .preview_library_storage_action(StorageAction::Recompress, &ctx)
        .unwrap();
    let legacy = library
        .preview_storage_action(&id, StorageAction::Recompress, None, &ctx)
        .unwrap();
    assert_eq!(plan.plan_token, legacy.plan_token);
    assert!(plan.reclaimable_bytes > 200_000);
    let applied = library
        .apply_library_storage_action(StorageAction::Recompress, &plan.plan_token, &ctx)
        .unwrap();
    assert_eq!(plan.reclaimable_bytes, applied.released_bytes);
    assert_eq!(
        fs::read(work.join("main.blend")).unwrap(),
        vec![1; 128 * 1024]
    );
    for variant in [
        &library.get_variant(&id).unwrap().variant,
        &other.variants[0],
    ] {
        let output = temp.path().join(&variant.id);
        library
            .materialize(&variant.head_snapshot_id, &output, &ctx)
            .unwrap();
        assert_eq!(
            fs::read(output.join("main.blend")).unwrap(),
            fs::read(Path::new(&variant.workspace_path).join("main.blend")).unwrap()
        );
    }
}

#[test]
fn global_purge_confirmation_tracks_targets_even_when_aggregate_sizes_are_equal() {
    let (temp, library, _, _) = fixture();
    let ctx = OperationContext::default();
    let db = rusqlite::Connection::open(temp.path().join("library/library.sqlite3")).unwrap();
    let candidate = |bytes: &[u8]| {
        let hash = blake3::hash(bytes).to_hex().to_string();
        let path = library.root().join("objects").join(&hash[..2]).join(&hash);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        db.execute(
            "INSERT INTO garbage_candidates VALUES(?1,?2)",
            rusqlite::params![hash, bytes.len() as i64],
        )
        .unwrap();
        (hash, path)
    };
    let (old_hash, old_path) = candidate(b"old");
    let plan = library
        .preview_library_storage_action(StorageAction::Purge, &ctx)
        .unwrap();
    db.execute("DELETE FROM garbage_candidates WHERE hash=?1", [&old_hash])
        .unwrap();
    fs::remove_file(old_path).unwrap();
    let (_, new_path) = candidate(b"new");
    let fresh = library
        .preview_library_storage_action(StorageAction::Purge, &ctx)
        .unwrap();
    assert_eq!(plan.physical_bytes, fresh.physical_bytes);
    assert_eq!(plan.reclaimable_bytes, fresh.reclaimable_bytes);
    assert_ne!(plan.plan_token, fresh.plan_token);
    assert!(
        library
            .apply_library_storage_action(StorageAction::Purge, &plan.plan_token, &ctx)
            .is_err()
    );
    assert!(new_path.exists());
    library
        .apply_library_storage_action(StorageAction::Purge, &fresh.plan_token, &ctx)
        .unwrap();
    assert!(!new_path.exists());
}

#[test]
fn retention_preserves_two_full_states_and_exact_preview_then_deletion_reclaims_objects() {
    let (temp, library, id, work) = fixture();
    let ctx = OperationContext::default();
    let first = library.get_variant(&id).unwrap().variant.head_snapshot_id;
    let original_glb = fs::read(work.join("main.glb")).unwrap();
    let second = version(&library, &id, &work, 2);
    let third = version(&library, &id, &work, 3);
    let variant = library.get_variant(&id).unwrap().variant;
    assert_eq!(variant.head_snapshot_id, third.id);
    assert_eq!(
        variant.backup_snapshot_id.as_deref(),
        Some(second.id.as_str())
    );
    let old = library.get_snapshot(&first).unwrap();
    assert_eq!(old.storage.kind, "preview");
    assert_eq!(old.entries.len(), 1);
    let path = temp.path().join("preview.glb");
    library
        .extract_snapshot_file(&first, "main.glb", &path, &ctx)
        .unwrap();
    assert_eq!(fs::read(path).unwrap(), original_glb);
    assert!(
        library
            .materialize(&first, &temp.path().join("unavailable"), &ctx)
            .is_err()
    );
    assert!(library.create_variant(&first, "Unavailable", &ctx).is_err());
    for protected in [&second.id, &third.id] {
        assert!(
            library
                .preview_storage_action(&id, StorageAction::DeleteVersion, Some(protected), &ctx)
                .is_err()
        );
    }
    let plan = library
        .preview_storage_action(&id, StorageAction::DeleteVersion, Some(&first), &ctx)
        .unwrap();
    assert!(plan.reclaimable_bytes > 0);
    let done = library
        .apply_storage_action(
            &id,
            StorageAction::DeleteVersion,
            Some(&first),
            &plan.plan_token,
            &ctx,
        )
        .unwrap();
    assert_eq!(plan.reclaimable_bytes, done.released_bytes);
    assert_eq!(library.list_snapshots(&id, 0, 100).unwrap().total, 2);
    assert!(work.join("script.py").exists());
    assert!(work.join("references/empty").is_dir());
    let backup = library
        .preview_storage_action(&id, StorageAction::ReleaseBackup, None, &ctx)
        .unwrap();
    library
        .apply_storage_action(
            &id,
            StorageAction::ReleaseBackup,
            None,
            &backup.plan_token,
            &ctx,
        )
        .unwrap();
    assert!(
        library
            .get_variant(&id)
            .unwrap()
            .variant
            .backup_snapshot_id
            .is_none()
    );
    assert_eq!(
        library.get_snapshot(&second.id).unwrap().storage.kind,
        "preview"
    );
    version(&library, &id, &work, 4);
    assert_eq!(
        library
            .get_variant(&id)
            .unwrap()
            .variant
            .backup_snapshot_id
            .as_deref(),
        Some(third.id.as_str())
    );
}

#[test]
fn legacy_history_stays_complete_until_the_reviewed_conversion_is_applied() {
    let (temp, library, id, work) = fixture();
    let ctx = OperationContext::default();
    let first = library.get_variant(&id).unwrap().variant.head_snapshot_id;
    let original_glb = fs::read(work.join("main.glb")).unwrap();
    // Reproduce the conservative flags left by the schema migration. Its full
    // schema-one upgrade is covered separately by the library unit test.
    rusqlite::Connection::open(temp.path().join("library/library.sqlite3"))
        .unwrap()
        .execute(
            "UPDATE snapshots SET legacy=1,preview_path=NULL WHERE id=?1",
            [&first],
        )
        .unwrap();
    let second = version(&library, &id, &work, 2);
    let third = version(&library, &id, &work, 3);
    let old = library.get_snapshot(&first).unwrap();
    assert_eq!(old.storage.kind, "full");
    assert!(old.storage.legacy);
    let plan = library
        .preview_storage_action(&id, StorageAction::Compact, None, &ctx)
        .unwrap();
    assert_eq!(plan.versions.len(), 1);
    assert_eq!(plan.versions[0].id, first);
    assert_eq!(library.get_snapshot(&first).unwrap().entries, old.entries);
    library
        .apply_storage_action(&id, StorageAction::Compact, None, &plan.plan_token, &ctx)
        .unwrap();
    assert_eq!(
        library.get_snapshot(&first).unwrap().storage.kind,
        "preview"
    );
    for protected in [second.id, third.id] {
        assert_eq!(
            library.get_snapshot(&protected).unwrap().storage.kind,
            "full"
        );
    }
    let exported = temp.path().join("legacy.glb");
    library
        .extract_snapshot_file(&first, "main.glb", &exported, &ctx)
        .unwrap();
    assert_eq!(fs::read(exported).unwrap(), original_glb);
    assert!(work.join("script.py").is_file());
}

#[test]
fn restoration_keeps_unversioned_changes_and_supports_returning_to_them() {
    let (temp, library, id, work) = fixture();
    let ctx = OperationContext::default();
    let first = library.get_variant(&id).unwrap().variant.head_snapshot_id;
    let second = version(&library, &id, &work, 2);
    fs::write(work.join("main.blend"), b"broken unsnapshotted model").unwrap();
    fs::write(work.join("new-reference.png"), b"valuable").unwrap();
    fs::remove_file(work.join("script.py")).unwrap();
    let restored = library
        .restore_backup(&id, &second.id, &first, &ctx)
        .unwrap();
    assert_eq!(
        fs::read(work.join("main.blend")).unwrap(),
        vec![1; 128 * 1024]
    );
    assert!(work.join("script.py").exists());
    assert!(!work.join("new-reference.png").exists());
    assert_eq!(
        restored.storage.restored_from.as_deref(),
        Some(first.as_str())
    );
    let backup = library
        .get_variant(&id)
        .unwrap()
        .variant
        .backup_snapshot_id
        .unwrap();
    assert!(
        !library
            .get_snapshot(&backup)
            .unwrap()
            .storage
            .preview_trusted
    );
    assert!(
        library
            .preview_storage_action(&id, StorageAction::ReleaseBackup, None, &ctx)
            .is_err()
    );
    let back = library
        .restore_backup(&id, &restored.id, &backup, &ctx)
        .unwrap();
    assert_eq!(
        fs::read(work.join("main.blend")).unwrap(),
        b"broken unsnapshotted model"
    );
    assert!(work.join("new-reference.png").exists());
    assert!(!work.join("script.py").exists());
    drop(library);
    let reopened = Library::open(temp.path().join("library")).unwrap();
    assert_eq!(
        reopened.get_variant(&id).unwrap().variant.head_snapshot_id,
        back.id
    );
    assert!(reopened.info().unwrap().recovery_notices.is_empty());
}

#[test]
fn shared_objects_and_fork_ancestry_survive_purge_and_stale_plans_are_rejected() {
    let (temp, library, id, work) = fixture();
    let ctx = OperationContext::default();
    let first = library.get_variant(&id).unwrap().variant.head_snapshot_id;
    let fork = library.create_variant(&first, "Fork", &ctx).unwrap();
    version(&library, &id, &work, 2);
    version(&library, &id, &work, 3);
    let plan = library
        .preview_storage_action(&id, StorageAction::DeleteVersion, Some(&first), &ctx)
        .unwrap();
    assert_eq!(plan.reclaimable_bytes, 0); // Fork owns the same GLB and source objects.
    version(&library, &id, &work, 4);
    assert!(
        library
            .apply_storage_action(
                &id,
                StorageAction::DeleteVersion,
                Some(&first),
                &plan.plan_token,
                &ctx
            )
            .is_err()
    );
    let fresh = library
        .preview_storage_action(&id, StorageAction::DeleteVersion, Some(&first), &ctx)
        .unwrap();
    library
        .apply_storage_action(
            &id,
            StorageAction::DeleteVersion,
            Some(&first),
            &fresh.plan_token,
            &ctx,
        )
        .unwrap();
    library
        .materialize(&fork.head_snapshot_id, &temp.path().join("fork-copy"), &ctx)
        .unwrap();
    assert_eq!(
        fs::read(temp.path().join("fork-copy/main.blend")).unwrap(),
        vec![1; 128 * 1024]
    );
    assert_eq!(library.list_snapshots(&fork.id, 0, 100).unwrap().total, 1);
}

#[test]
fn missing_or_invalid_preview_never_discards_sources_and_pending_journals_block_purge() {
    let (_temp, library, id, work) = fixture();
    let ctx = OperationContext::default();
    fs::write(work.join("main.glb"), b"invalid").unwrap();
    fs::write(work.join("main.blend"), b"source with invalid glb").unwrap();
    let bad = library
        .create_snapshot(&id, "Invalid old GLB", &ctx)
        .unwrap();
    version(&library, &id, &work, 3);
    version(&library, &id, &work, 4);
    assert_eq!(library.get_snapshot(&bad.id).unwrap().storage.kind, "full");
    let plan = library
        .preview_storage_action(&id, StorageAction::Compact, None, &ctx)
        .unwrap();
    assert!(plan.warnings.iter().any(|w| w.contains("Invalid old GLB")));
    assert!(!plan.versions.iter().any(|v| v.id == bad.id));
    let pending = library.root().join("staging/pending");
    fs::create_dir(&pending).unwrap();
    fs::write(pending.join("operation.json"), b"unresolved").unwrap();
    assert!(
        library
            .preview_storage_action(&id, StorageAction::Purge, None, &ctx)
            .is_err()
    );
}

#[test]
fn concurrent_save_before_restore_publication_preserves_workspace_and_head() {
    let (_temp, library, id, work) = fixture();
    let first = library.get_variant(&id).unwrap().variant.head_snapshot_id;
    let second = version(&library, &id, &work, 2);
    let file = work.join("main.blend");
    let ctx = OperationContext {
        progress: Some(Arc::new(move |event| {
            if event.phase == "materialize" {
                fs::write(&file, b"concurrent save").unwrap();
            }
        })),
        ..Default::default()
    };
    assert!(
        library
            .restore_backup(&id, &second.id, &first, &ctx)
            .is_err()
    );
    assert_eq!(
        library.get_variant(&id).unwrap().variant.head_snapshot_id,
        second.id
    );
    assert_eq!(
        fs::read(work.join("main.blend")).unwrap(),
        b"concurrent save"
    );
}

#[test]
#[ignore = "subprocess helper for disposable crash fixtures"]
fn restore_crash_helper() {
    let Ok(root) = std::env::var("EDGE_LOCK_RESTORE_ROOT") else {
        return;
    };
    let library = Library::open(root).unwrap();
    let id = std::env::var("EDGE_LOCK_RESTORE_VARIANT").unwrap();
    let phase = std::env::var("EDGE_LOCK_RESTORE_PHASE").unwrap();
    let variant = library.get_variant(&id).unwrap().variant;
    let ctx = OperationContext {
        progress: Some(Arc::new(move |event| {
            if event.phase == phase {
                std::process::exit(91)
            }
        })),
        ..Default::default()
    };
    library
        .restore_backup(
            &id,
            &variant.head_snapshot_id,
            variant.backup_snapshot_id.as_deref().unwrap(),
            &ctx,
        )
        .unwrap();
    panic!("crash boundary not reached");
}

#[test]
fn every_restore_publication_boundary_recovers_after_abrupt_process_exit() {
    for phase in [
        "Restauration : journal publié",
        "Restauration : précédent préservé",
        "Restauration : dossier remplacé",
        "Restauration : catalogue publié",
    ] {
        let (temp, library, id, work) = fixture();
        let second = version(&library, &id, &work, 2);
        drop(library);
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "restore_crash_helper",
                "--nocapture",
            ])
            .env("EDGE_LOCK_RESTORE_ROOT", temp.path().join("library"))
            .env("EDGE_LOCK_RESTORE_VARIANT", &id)
            .env("EDGE_LOCK_RESTORE_PHASE", phase)
            .output()
            .unwrap();
        assert_eq!(
            result.status.code(),
            Some(91),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let reopened = Library::open(temp.path().join("library")).unwrap();
        assert!(
            reopened.info().unwrap().recovery_notices.is_empty(),
            "{phase}"
        );
        let variant = reopened.get_variant(&id).unwrap().variant;
        let committed = phase == "Restauration : catalogue publié";
        assert_eq!(variant.head_snapshot_id == second.id, !committed);
        assert_eq!(
            fs::read(work.join("main.blend")).unwrap(),
            vec![if committed { 1 } else { 2 }; 128 * 1024]
        );
    }
}

#[test]
fn corrupted_backup_after_committed_crash_preserves_the_previous_folder() {
    let (temp, library, id, work) = fixture();
    version(&library, &id, &work, 2);
    drop(library);
    let root = temp.path().join("library");
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "restore_crash_helper",
            "--nocapture",
        ])
        .env("EDGE_LOCK_RESTORE_ROOT", &root)
        .env("EDGE_LOCK_RESTORE_VARIANT", &id)
        .env("EDGE_LOCK_RESTORE_PHASE", "Restauration : catalogue publié")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(91));
    let journal = fs::read_dir(root.join("staging"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.join("restore.json").exists())
        .unwrap();
    let hash = blake3::hash(&vec![2; 128 * 1024]).to_hex().to_string();
    fs::write(
        root.join("objects")
            .join(&hash[..2])
            .join(format!("{hash}.zst")),
        b"corrupt",
    )
    .unwrap();
    let recovered = Library::open(&root).unwrap();
    assert!(!recovered.info().unwrap().recovery_notices.is_empty());
    assert_eq!(
        fs::read(journal.join("previous/main.blend")).unwrap(),
        vec![2; 128 * 1024]
    );
    assert_eq!(
        fs::read(work.join("main.blend")).unwrap(),
        vec![1; 128 * 1024]
    );
    assert!(
        recovered
            .preview_storage_action(
                &id,
                StorageAction::Purge,
                None,
                &OperationContext::default()
            )
            .is_err()
    );
}

#[test]
fn cancellation_after_journal_publication_finishes_in_a_consistent_state() {
    let (_temp, library, id, work) = fixture();
    let first = library.get_variant(&id).unwrap().variant.head_snapshot_id;
    let second = version(&library, &id, &work, 2);
    let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = cancelled.clone();
    let ctx = OperationContext {
        cancelled,
        progress: Some(Arc::new(move |event| {
            if event.phase == "Restauration : journal publié" {
                flag.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        })),
    };
    let restored = library
        .restore_backup(&id, &second.id, &first, &ctx)
        .unwrap();
    assert_eq!(
        library.get_variant(&id).unwrap().variant.head_snapshot_id,
        restored.id
    );
    assert!(library.info().unwrap().recovery_notices.is_empty());
}

#[cfg(windows)]
#[test]
fn locked_workspace_refuses_restore_without_changing_any_state() {
    use std::os::windows::fs::OpenOptionsExt;
    let (_temp, library, id, work) = fixture();
    let first = library.get_variant(&id).unwrap().variant.head_snapshot_id;
    let second = version(&library, &id, &work, 2);
    let locked = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(work.join("main.blend"))
        .unwrap();
    assert!(
        library
            .restore_backup(&id, &second.id, &first, &OperationContext::default())
            .is_err()
    );
    drop(locked);
    assert_eq!(
        library.get_variant(&id).unwrap().variant.head_snapshot_id,
        second.id
    );
    assert_eq!(
        fs::read(work.join("main.blend")).unwrap(),
        vec![2; 128 * 1024]
    );
}
