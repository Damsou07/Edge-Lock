use edge_lock_core::{ExportConfig, Library, OperationContext};
use std::{fs, path::PathBuf, sync::Arc};

fn fixture() -> (tempfile::TempDir, Library, String, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    fs::create_dir_all(source.join("empty")).unwrap();
    fs::write(source.join("main.blend"), b"saved source").unwrap();
    fs::write(source.join("other.blend"), b"other source").unwrap();
    fs::write(source.join("main.blend1"), b"saved Blender backup").unwrap();
    fs::write(source.join("user.glb"), b"saved user model").unwrap();
    fs::write(source.join(".hidden"), b"saved hidden file").unwrap();
    let library = Library::open(temp.path().join("library")).unwrap();
    let asset = library
        .import_folder(
            &source,
            "Asset",
            ExportConfig {
                primary_file: Some("main.blend".into()),
                ..Default::default()
            },
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

#[test]
fn schema_four_upgrade_keeps_versions_config_and_creates_a_catalog_backup() {
    let (temp, library, id, _work) = fixture();
    let before = library.get_variant(&id).unwrap();
    drop(library);
    let root = temp.path().join("library");
    let db = rusqlite::Connection::open(root.join("library.sqlite3")).unwrap();
    db.execute_batch("DROP TABLE workspace_discards; PRAGMA user_version=4;")
        .unwrap();
    drop(db);
    let upgraded = Library::open(&root).unwrap();
    let after = upgraded.get_variant(&id).unwrap();
    assert_eq!(
        after.variant.head_snapshot_id,
        before.variant.head_snapshot_id
    );
    assert_eq!(
        after.variant.backup_snapshot_id,
        before.variant.backup_snapshot_id
    );
    assert_eq!(after.variant.config, before.variant.config);
    assert_eq!(after.snapshots.len(), before.snapshots.len());
    assert!(fs::read_dir(root.join("backups")).unwrap().any(|item| {
        item.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("catalog-schema-4-")
    }));
}

#[test]
fn corrupt_saved_object_refuses_discard_without_touching_local_files() {
    let (temp, library, id, work) = fixture();
    let head = library.get_variant(&id).unwrap().variant.head_snapshot_id;
    let saved = library.get_snapshot(&head).unwrap();
    let hash = saved
        .entries
        .iter()
        .find(|entry| entry.path == "main.blend")
        .unwrap()
        .hash
        .as_ref()
        .unwrap();
    let raw = temp
        .path()
        .join("library/objects")
        .join(&hash[..2])
        .join(hash);
    let stored = if raw.exists() {
        raw
    } else {
        raw.with_extension("zst")
    };
    fs::write(stored, b"corrupt saved object").unwrap();
    fs::write(work.join("main.blend"), b"important local source").unwrap();
    assert!(
        library
            .discard_changes(&id, &head, &OperationContext::default())
            .is_err()
    );
    assert_eq!(
        fs::read(work.join("main.blend")).unwrap(),
        b"important local source"
    );
    assert_eq!(
        library.get_variant(&id).unwrap().variant.head_snapshot_id,
        head
    );
}

#[test]
fn cleanup_preserves_a_later_file_changed_after_the_first_file_was_removed() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let (temp, library, id, work) = fixture();
    let before = library.get_variant(&id).unwrap().variant;
    fs::write(work.join("main.blend"), b"discarded source").unwrap();
    let staging = temp.path().join("library/staging");
    let changed = Arc::new(AtomicBool::new(false));
    let changed_in_hook = changed.clone();
    let inspect = staging.clone();
    let ctx = OperationContext {
        progress: Some(Arc::new(move |event| {
            if event.phase == "Abandon : fichier nettoyé"
                && !changed_in_hook.swap(true, Ordering::Relaxed)
            {
                let directory = fs::read_dir(&inspect)
                    .unwrap()
                    .map(|item| item.unwrap().path())
                    .find(|path| path.join("discard.json").exists())
                    .unwrap();
                fs::write(
                    directory.join("previous/user.glb"),
                    b"new external write after cleanup started",
                )
                .unwrap();
            }
        })),
        ..Default::default()
    };
    let report = library
        .discard_changes(&id, &before.head_snapshot_id, &ctx)
        .unwrap();
    assert!(changed.load(Ordering::Relaxed));
    assert!(report.cleanup_pending);
    let directory = fs::read_dir(&staging)
        .unwrap()
        .map(|item| item.unwrap().path())
        .find(|path| path.join("discard.json").exists())
        .unwrap();
    assert_eq!(
        fs::read(directory.join("previous/user.glb")).unwrap(),
        b"new external write after cleanup started"
    );
    library.update_config(&id, before.config).unwrap();
}

#[cfg(windows)]
#[test]
fn committed_cleanup_failure_is_successful_and_does_not_block_other_operations() {
    use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt, sync::Mutex};
    let (temp, library, id, work) = fixture();
    let before = library.get_variant(&id).unwrap().variant;
    fs::write(work.join("main.blend"), b"discarded source").unwrap();
    let root = temp.path().join("library");
    let holder = Arc::new(Mutex::new(None));
    let lock_file = holder.clone();
    let staging = root.join("staging");
    let ctx = OperationContext {
        progress: Some(Arc::new(move |event| {
            if event.phase == "Abandon : catalogue publié" {
                let directory = fs::read_dir(&staging)
                    .unwrap()
                    .map(|item| item.unwrap().path())
                    .find(|path| path.join("discard.json").exists())
                    .unwrap();
                *lock_file.lock().unwrap() = Some(
                    OpenOptions::new()
                        .read(true)
                        .share_mode(0)
                        .open(directory.join("previous/main.blend"))
                        .unwrap(),
                );
            }
        })),
        ..Default::default()
    };
    let report = library
        .discard_changes(&id, &before.head_snapshot_id, &ctx)
        .unwrap();
    assert!(report.cleanup_pending);
    assert_eq!(report.warnings.len(), 1);
    // A committed checkpoint must not freeze the library while an old file is locked.
    library.update_config(&id, before.config.clone()).unwrap();
    assert_eq!(fs::read(work.join("main.blend")).unwrap(), b"saved source");
    fs::write(work.join("main.blend"), b"new edits after discard").unwrap();
    drop(holder.lock().unwrap().take());
    drop(library);
    let reopened = Library::open(&root).unwrap();
    assert!(reopened.info().unwrap().recovery_notices.is_empty());
    assert_eq!(
        fs::read(work.join("main.blend")).unwrap(),
        b"new edits after discard"
    );
}

#[test]
fn discard_restores_exact_latest_version_and_config_without_changing_history_or_backup() {
    let (_temp, library, id, work) = fixture();
    let ctx = OperationContext::default();
    fs::write(work.join("main.blend"), b"latest saved source").unwrap();
    let saved = library.create_snapshot(&id, "Latest", &ctx).unwrap();
    let before = library.get_variant(&id).unwrap();
    let fork = library
        .create_variant(&saved.id, "Independent", &ctx)
        .unwrap();
    fs::write(work.join("main.blend"), b"unsaved source").unwrap();
    fs::write(work.join("added.glb"), b"unsaved model").unwrap();
    fs::write(work.join("user.glb"), b"modified user model").unwrap();
    fs::write(work.join("main.blend1"), b"modified Blender backup").unwrap();
    fs::remove_file(work.join(".hidden")).unwrap();
    fs::remove_dir(work.join("empty")).unwrap();
    let mut config = before.variant.config.clone();
    config.primary_file = Some("other.blend".into());
    library.update_config(&id, config).unwrap();
    let report = library.discard_changes(&id, &saved.id, &ctx).unwrap();
    assert!(!report.cleanup_pending);
    assert!(report.warnings.is_empty());
    let after = library.get_variant(&id).unwrap();
    assert_eq!(after.variant.head_snapshot_id, saved.id);
    assert_eq!(
        after.variant.backup_snapshot_id,
        before.variant.backup_snapshot_id
    );
    assert_eq!(after.snapshots.len(), before.snapshots.len());
    assert_eq!(after.variant.config, saved.config);
    assert_eq!(after.variant.working_bytes, saved.logical_bytes);
    assert_eq!(
        fs::read(work.join("main.blend")).unwrap(),
        b"latest saved source"
    );
    assert!(!work.join("added.glb").exists());
    assert_eq!(
        fs::read(work.join("user.glb")).unwrap(),
        b"saved user model"
    );
    assert_eq!(
        fs::read(work.join("main.blend1")).unwrap(),
        b"saved Blender backup"
    );
    assert!(work.join("empty").is_dir());
    assert_eq!(
        fs::read(work.join(".hidden")).unwrap(),
        b"saved hidden file"
    );
    assert_eq!(
        fs::read(PathBuf::from(&fork.workspace_path).join("main.blend")).unwrap(),
        b"latest saved source"
    );
    assert!(
        !library
            .discard_changes(&id, &saved.id, &ctx)
            .unwrap()
            .cleanup_pending
    );
}

#[test]
fn stale_head_cancelled_and_concurrent_write_keep_the_current_workspace() {
    let (_temp, library, id, work) = fixture();
    let head = library.get_variant(&id).unwrap().variant.head_snapshot_id;
    fs::remove_file(work.join("main.blend")).unwrap();
    fs::write(work.join("added.txt"), b"keep me on failure").unwrap();
    assert!(
        library
            .discard_changes(&id, "stale", &OperationContext::default())
            .is_err()
    );
    let cancelled = OperationContext::default();
    cancelled
        .cancelled
        .store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(library.discard_changes(&id, &head, &cancelled).is_err());
    let changed = work.clone();
    let ctx = OperationContext {
        progress: Some(Arc::new(move |event| {
            if event.phase == "Abandon : journal publié" {
                fs::write(changed.join("added.txt"), b"concurrent save").unwrap();
            }
        })),
        ..Default::default()
    };
    assert!(library.discard_changes(&id, &head, &ctx).is_err());
    assert!(!work.join("main.blend").exists());
    assert_eq!(
        fs::read(work.join("added.txt")).unwrap(),
        b"concurrent save"
    );
    assert_eq!(
        library.get_variant(&id).unwrap().variant.head_snapshot_id,
        head
    );
}

#[test]
#[ignore]
fn discard_crash_helper() {
    let Ok(root) = std::env::var("EDGE_LOCK_DISCARD_ROOT") else {
        return;
    };
    let library = Library::open(root).unwrap();
    let id = std::env::var("EDGE_LOCK_DISCARD_VARIANT").unwrap();
    let phase = std::env::var("EDGE_LOCK_DISCARD_PHASE").unwrap();
    let head = library.get_variant(&id).unwrap().variant.head_snapshot_id;
    let ctx = OperationContext {
        progress: Some(Arc::new(move |event| {
            if event.phase == phase {
                std::process::exit(91);
            }
        })),
        ..Default::default()
    };
    library.discard_changes(&id, &head, &ctx).unwrap();
    panic!("crash boundary not reached");
}

#[test]
fn every_discard_publication_boundary_recovers_without_new_history() {
    for phase in [
        "Abandon : journal publié",
        "Abandon : précédent préservé",
        "Abandon : dossier remplacé",
        "Abandon : catalogue publié",
        "Abandon : fichier nettoyé",
    ] {
        let (temp, library, id, work) = fixture();
        let before = library.get_variant(&id).unwrap();
        fs::write(work.join("main.blend"), b"unsaved source").unwrap();
        fs::write(work.join("added.txt"), b"unsaved addition").unwrap();
        drop(library);
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "discard_crash_helper",
                "--nocapture",
            ])
            .env("EDGE_LOCK_DISCARD_ROOT", temp.path().join("library"))
            .env("EDGE_LOCK_DISCARD_VARIANT", &id)
            .env("EDGE_LOCK_DISCARD_PHASE", phase)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(91),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let reopened = Library::open(temp.path().join("library")).unwrap();
        assert!(
            reopened.info().unwrap().recovery_notices.is_empty(),
            "{phase}"
        );
        let after = reopened.get_variant(&id).unwrap();
        assert_eq!(
            after.variant.head_snapshot_id,
            before.variant.head_snapshot_id
        );
        assert_eq!(
            after.variant.backup_snapshot_id,
            before.variant.backup_snapshot_id
        );
        assert_eq!(after.snapshots.len(), before.snapshots.len());
        let committed =
            ["Abandon : catalogue publié", "Abandon : fichier nettoyé"].contains(&phase);
        assert_eq!(
            fs::read(work.join("main.blend")).unwrap(),
            if committed {
                b"saved source".as_slice()
            } else {
                b"unsaved source".as_slice()
            },
            "{phase}"
        );
        assert_eq!(work.join("added.txt").exists(), !committed, "{phase}");
    }
}
