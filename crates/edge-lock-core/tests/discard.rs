#[cfg(windows)]
use edge_lock_core::WorkspaceDiscardState;
use edge_lock_core::{ExportConfig, FileSelectionKind, Library, OperationContext};
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
    db.execute_batch("DROP TABLE pending_workspace_discards; DROP TABLE file_selections; ALTER TABLE variants DROP COLUMN deleted_name; DROP TABLE workspace_discards; PRAGMA user_version=4;")
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
fn schema_six_upgrade_preserves_versions_selections_and_live_edits() {
    let (temp, library, id, work) = fixture();
    let head = library.get_variant(&id).unwrap().variant.head_snapshot_id;
    let saved = library.get_snapshot(&head).unwrap();
    library
        .set_file_selection(&id, None, FileSelectionKind::Preview, Some("other.blend"))
        .unwrap();
    fs::write(work.join("main.blend"), b"unsaved source").unwrap();
    drop(library);
    let root = temp.path().join("library");
    let db = rusqlite::Connection::open(root.join("library.sqlite3")).unwrap();
    db.execute_batch("DROP TABLE pending_workspace_discards; PRAGMA user_version=6;")
        .unwrap();
    drop(db);

    let upgraded = Library::open(&root).unwrap();
    assert!(!upgraded.has_pending_discards().unwrap());
    assert_eq!(upgraded.get_snapshot(&head).unwrap().entries, saved.entries);
    assert_eq!(
        upgraded
            .get_file_selections(&id, None)
            .unwrap()
            .preview_file
            .as_deref(),
        Some("other.blend")
    );
    assert_eq!(
        fs::read(work.join("main.blend")).unwrap(),
        b"unsaved source"
    );
    let backup = fs::read_dir(root.join("backups"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("catalog-schema-6-")
        })
        .unwrap();
    let backup = rusqlite::Connection::open(backup).unwrap();
    assert_eq!(
        backup
            .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
            .unwrap(),
        6
    );
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
fn discard_accepts_a_reader_that_shares_delete_access() {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    let (_temp, library, id, work) = fixture();
    let before = library.get_variant(&id).unwrap().variant;
    let file = work.join("main.blend");
    fs::write(&file, b"discarded source").unwrap();
    let reader = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(&file)
        .unwrap();
    let report = library
        .discard_changes(&id, &before.head_snapshot_id, &OperationContext::default())
        .unwrap();
    assert!(!report.reconfirmation_required);
    drop(reader);
    if report.pending {
        let reports = library
            .retry_pending_discards(&OperationContext::default())
            .unwrap();
        assert_eq!(reports.len(), 1);
        assert!(!reports[0].reconfirmation_required);
    }
    assert_eq!(fs::read(&file).unwrap(), b"saved source");
}

#[cfg(windows)]
fn lock_workspace_rename(path: &std::path::Path) -> fs::File {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .unwrap()
}

#[cfg(windows)]
#[test]
fn discard_waits_for_a_blocking_rename_and_survives_library_reopen() {
    let (temp, library, id, work) = fixture();
    let before = library.get_variant(&id).unwrap();
    let head = &before.variant.head_snapshot_id;
    let ctx = OperationContext::default();
    fs::write(work.join("main.blend"), b"unsaved source").unwrap();
    let locked = lock_workspace_rename(&work);
    let report = library.discard_changes(&id, head, &ctx).unwrap();
    assert!(report.pending);
    assert!(!report.cleanup_pending);
    assert!(library.has_pending_discards().unwrap());
    assert!(library.retry_pending_discards(&ctx).unwrap().is_empty());
    assert_eq!(
        fs::read(work.join("main.blend")).unwrap(),
        b"unsaved source"
    );
    library.save_category(None, "Still writable", None).unwrap();

    drop(library);
    let library = Library::open(temp.path().join("library")).unwrap();
    assert_eq!(
        library.discard_status(&id).unwrap(),
        Some(WorkspaceDiscardState::Pending)
    );
    drop(locked);
    let reports = library.retry_pending_discards(&ctx).unwrap();
    assert_eq!(reports.len(), 1);
    assert!(!reports[0].pending);
    assert!(!reports[0].reconfirmation_required);
    assert!(!reports[0].cleanup_pending);
    assert_eq!(library.discard_status(&id).unwrap(), None);
    assert!(!library.has_pending_discards().unwrap());
    assert_eq!(fs::read(work.join("main.blend")).unwrap(), b"saved source");
    let after = library.get_variant(&id).unwrap();
    assert_eq!(after.variant.head_snapshot_id, *head);
    assert_eq!(
        after.variant.backup_snapshot_id,
        before.variant.backup_snapshot_id
    );
    assert_eq!(after.snapshots.len(), before.snapshots.len());
}

#[cfg(windows)]
#[test]
fn deleting_a_variant_drops_its_pending_discard() {
    let (_temp, library, id, _work) = fixture();
    let ctx = OperationContext::default();
    let head = library.get_variant(&id).unwrap().variant.head_snapshot_id;
    let fork = library.create_variant(&head, "Fork", &ctx).unwrap();
    let work = PathBuf::from(&fork.workspace_path);
    fs::write(work.join("main.blend"), b"unsaved source").unwrap();
    let locked = lock_workspace_rename(&work);
    assert!(
        library
            .discard_changes(&fork.id, &fork.head_snapshot_id, &ctx)
            .unwrap()
            .pending
    );
    let plan = library.plan_variant_deletion(&fork.id, &ctx).unwrap();
    assert!(
        library
            .apply_variant_deletion(&fork.id, &plan.plan_token, &ctx)
            .unwrap()
            .cleanup_pending
    );
    assert_eq!(library.discard_status(&fork.id).unwrap(), None);
    assert!(library.retry_pending_discards(&ctx).unwrap().is_empty());
    drop(locked);
}

#[cfg(windows)]
#[test]
fn unreadable_workspace_requires_a_new_confirmation_after_unlock() {
    use std::os::windows::fs::OpenOptionsExt;

    let (_temp, library, id, work) = fixture();
    let head = library.get_variant(&id).unwrap().variant.head_snapshot_id;
    let ctx = OperationContext::default();
    let file = work.join("main.blend");
    fs::write(&file, b"unsaved source").unwrap();
    let locked = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&file)
        .unwrap();
    assert!(library.discard_changes(&id, &head, &ctx).unwrap().pending);
    assert!(library.retry_pending_discards(&ctx).unwrap().is_empty());
    drop(locked);
    let reports = library.retry_pending_discards(&ctx).unwrap();
    assert_eq!(reports.len(), 1);
    assert!(reports[0].reconfirmation_required);
    assert_eq!(
        library.discard_status(&id).unwrap(),
        Some(WorkspaceDiscardState::ReconfirmationRequired)
    );
    assert!(!library.has_pending_discards().unwrap());
    assert_eq!(fs::read(&file).unwrap(), b"unsaved source");
    assert!(!library.discard_changes(&id, &head, &ctx).unwrap().pending);
    assert_eq!(library.discard_status(&id).unwrap(), None);
    assert_eq!(fs::read(&file).unwrap(), b"saved source");
}

#[cfg(windows)]
#[test]
fn pending_discard_preserves_new_bytes_config_and_replaced_workspace() {
    for changed in ["bytes", "config", "workspace", "head"] {
        let (temp, library, id, work) = fixture();
        let before = library.get_variant(&id).unwrap().variant;
        let ctx = OperationContext::default();
        let file = work.join("main.blend");
        fs::write(&file, b"old edits").unwrap();
        let locked = lock_workspace_rename(&work);
        assert!(
            library
                .discard_changes(&id, &before.head_snapshot_id, &ctx)
                .unwrap()
                .pending
        );
        drop(locked);
        match changed {
            "bytes" => {
                let modified = fs::metadata(&file).unwrap().modified().unwrap();
                fs::write(&file, b"new edits").unwrap();
                fs::OpenOptions::new()
                    .write(true)
                    .open(&file)
                    .unwrap()
                    .set_times(fs::FileTimes::new().set_modified(modified))
                    .unwrap();
            }
            "config" => {
                let mut config = before.config;
                config.primary_file = Some("other.blend".into());
                library.update_config(&id, config).unwrap();
            }
            "workspace" => {
                let moved = temp.path().join("previous");
                fs::rename(&work, &moved).unwrap();
                fs::create_dir(&work).unwrap();
                for item in fs::read_dir(moved).unwrap() {
                    let item = item.unwrap();
                    if item.file_type().unwrap().is_dir() {
                        fs::create_dir(work.join(item.file_name())).unwrap();
                    } else {
                        fs::copy(item.path(), work.join(item.file_name())).unwrap();
                    }
                }
            }
            "head" => {
                library.create_snapshot(&id, "New head", &ctx).unwrap();
            }
            _ => unreachable!(),
        }
        let reports = library.retry_pending_discards(&ctx).unwrap();
        assert_eq!(reports.len(), 1, "{changed}");
        assert!(reports[0].reconfirmation_required, "{changed}");
        assert_eq!(
            fs::read(&file).unwrap().as_slice(),
            if changed == "bytes" {
                b"new edits"
            } else {
                b"old edits"
            }
        );
        assert_eq!(
            library.discard_status(&id).unwrap(),
            Some(WorkspaceDiscardState::ReconfirmationRequired)
        );
        assert!(library.retry_pending_discards(&ctx).unwrap().is_empty());
    }
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
    if std::env::var("EDGE_LOCK_DISCARD_RETRY").as_deref() == Ok("1") {
        library.retry_pending_discards(&ctx).unwrap();
    } else {
        library.discard_changes(&id, &head, &ctx).unwrap();
    }
    panic!("crash boundary not reached");
}

fn crash_at(root: &std::path::Path, variant_id: &str, phase: &str, retry: bool) {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "discard_crash_helper",
            "--nocapture",
        ])
        .env("EDGE_LOCK_DISCARD_ROOT", root)
        .env("EDGE_LOCK_DISCARD_VARIANT", variant_id)
        .env("EDGE_LOCK_DISCARD_PHASE", phase)
        .env("EDGE_LOCK_DISCARD_RETRY", if retry { "1" } else { "0" })
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(91),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(windows)]
#[test]
fn pending_discard_is_consumed_atomically_with_the_workspace_swap() {
    for (phase, committed) in [
        ("Abandon : dossier remplacé", false),
        ("Abandon : catalogue publié", true),
    ] {
        let (temp, library, id, work) = fixture();
        let root = temp.path().join("library");
        let ctx = OperationContext::default();
        let head = library.get_variant(&id).unwrap().variant.head_snapshot_id;
        let file = work.join("main.blend");
        fs::write(&file, b"unsaved source").unwrap();
        let locked = lock_workspace_rename(&work);
        assert!(library.discard_changes(&id, &head, &ctx).unwrap().pending);
        drop(locked);
        drop(library);

        crash_at(&root, &id, phase, true);
        let reopened = Library::open(&root).unwrap();
        assert!(
            reopened.info().unwrap().recovery_notices.is_empty(),
            "{phase}"
        );
        assert_eq!(
            reopened.has_pending_discards().unwrap(),
            !committed,
            "{phase}"
        );
        assert_eq!(
            fs::read(&file).unwrap(),
            if committed {
                b"saved source".as_slice()
            } else {
                b"unsaved source"
            },
            "{phase}",
        );
        assert_eq!(
            reopened.retry_pending_discards(&ctx).unwrap().len(),
            usize::from(!committed)
        );
        assert_eq!(fs::read(&file).unwrap(), b"saved source");
    }
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
        crash_at(&temp.path().join("library"), &id, phase, false);
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
