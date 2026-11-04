use edge_lock_core::{AssetFilter, ExportConfig, Library, OperationContext};
use std::{fs, path::Path};

fn import(
    library: &Library,
    root: &Path,
    name: &str,
    content: &[u8],
) -> edge_lock_core::AssetDetail {
    let source = root.join(name);
    fs::create_dir(&source).unwrap();
    fs::write(source.join("main.blend"), content).unwrap();
    library
        .import_folder(
            &source,
            name,
            ExportConfig::default(),
            &OperationContext::default(),
        )
        .unwrap()
}

fn archive(library: &Library, asset: &edge_lock_core::AssetDetail) {
    library
        .update_asset(
            &asset.asset.id,
            &asset.asset.name,
            None,
            vec![],
            vec![],
            true,
        )
        .unwrap();
}

#[test]
fn permanent_deletion_removes_all_owned_versions_and_keeps_shared_objects_and_originals() {
    let temp = tempfile::tempdir().unwrap();
    let library = Library::open(temp.path().join("library")).unwrap();
    let ctx = OperationContext::default();
    let asset = import(&library, temp.path(), "Removed", b"shared bytes");
    let survivor = import(&library, temp.path(), "Survivor", b"shared bytes");
    library
        .create_variant(&asset.variants[0].head_snapshot_id, "Fork", &ctx)
        .unwrap();
    archive(&library, &asset);
    let plan = library
        .plan_archive_deletion(Some(&asset.asset.id), &ctx)
        .unwrap();
    assert_eq!(plan.asset_count, 1);
    assert_eq!(plan.variant_count, 2);
    let report = library
        .apply_archive_deletion(Some(&asset.asset.id), &plan.plan_token, &ctx)
        .unwrap();
    assert_eq!(report.deleted_asset_ids, vec![asset.asset.id.clone()]);
    assert!(!report.cleanup_pending);
    assert!(
        !library
            .root()
            .join("workspaces")
            .join(&asset.asset.id)
            .exists()
    );
    assert!(library.get_asset(&asset.asset.id).is_err());
    let output = temp.path().join("survivor-copy");
    library
        .materialize(&survivor.variants[0].head_snapshot_id, &output, &ctx)
        .unwrap();
    assert_eq!(
        fs::read(output.join("main.blend")).unwrap(),
        b"shared bytes"
    );
    assert_eq!(
        fs::read(temp.path().join("Removed/main.blend")).unwrap(),
        b"shared bytes"
    );
}

#[test]
fn archive_plan_is_frozen_and_refuses_active_assets_and_catalog_changes() {
    let temp = tempfile::tempdir().unwrap();
    let library = Library::open(temp.path().join("library")).unwrap();
    let ctx = OperationContext::default();
    let asset = import(&library, temp.path(), "Asset", b"content");
    assert!(
        library
            .plan_archive_deletion(Some(&asset.asset.id), &ctx)
            .is_err()
    );
    archive(&library, &asset);
    let plan = library.plan_archive_deletion(None, &ctx).unwrap();
    let other = import(&library, temp.path(), "Other", b"other");
    archive(&library, &other);
    assert!(
        library
            .apply_archive_deletion(None, &plan.plan_token, &ctx)
            .is_err()
    );
    assert!(Path::new(&asset.variants[0].workspace_path).exists());
    let plan = library.plan_archive_deletion(None, &ctx).unwrap();
    let report = library
        .apply_archive_deletion(None, &plan.plan_token, &ctx)
        .unwrap();
    assert_eq!(report.deleted_asset_ids.len(), 2);
    assert_eq!(library.info().unwrap().asset_count, 0);
}

#[test]
fn deleting_nested_category_and_multi_project_game_archives_assets_without_removing_files() {
    let temp = tempfile::tempdir().unwrap();
    let library = Library::open(temp.path().join("library")).unwrap();
    let a = import(&library, temp.path(), "Category asset", b"a");
    let b = import(&library, temp.path(), "Project asset", b"b");
    let parent = library.save_category(None, "Parent", None).unwrap();
    let child = library
        .save_category(None, "Child", Some(&parent.id))
        .unwrap();
    let game = library.save_game(None, "Game").unwrap();
    let keep = library.save_game(None, "Keep").unwrap();
    library
        .update_asset(
            &a.asset.id,
            "Category asset",
            Some(&child.id),
            vec!["tag".into()],
            vec![keep.id.clone()],
            false,
        )
        .unwrap();
    library
        .update_asset(
            &b.asset.id,
            "Project asset",
            None,
            vec![],
            vec![game.id.clone(), keep.id.clone()],
            false,
        )
        .unwrap();
    assert_eq!(library.delete_category(&parent.id).unwrap(), 1);
    assert_eq!(library.delete_game(&game.id).unwrap(), 1);
    assert!(library.taxonomy().unwrap().categories.is_empty());
    for asset in [&a, &b] {
        let detail = library.get_asset(&asset.asset.id).unwrap();
        assert!(detail.asset.archived);
        assert!(detail.asset.category_id.is_none());
        assert_eq!(
            detail.asset.game_ids.as_slice(),
            std::slice::from_ref(&keep.id)
        );
        assert!(Path::new(&detail.variants[0].workspace_path).is_dir());
    }
    assert_eq!(
        library
            .list_assets(AssetFilter {
                archived: true,
                ..Default::default()
            })
            .unwrap()
            .total,
        2
    );
}

#[test]
fn owned_glb_backups_are_released_only_after_their_last_asset_owner() {
    let temp = tempfile::tempdir().unwrap();
    let library = Library::open(temp.path().join("library")).unwrap();
    let ctx = OperationContext::default();
    let a = import(&library, temp.path(), "A", b"a");
    let b = import(&library, temp.path(), "B", b"b");
    let hash = blake3::hash(b"backup").to_hex().to_string();
    let backups = library.root().join("backups/glb");
    fs::create_dir_all(&backups).unwrap();
    let owned = backups.join(format!("{hash}.glb"));
    fs::write(&owned, b"backup").unwrap();
    fs::write(backups.join("legacy.glb"), b"unknown owner").unwrap();
    library
        .register_glb_backup(&a.variants[0].id, &hash)
        .unwrap();
    library
        .register_glb_backup(&b.variants[0].id, &hash)
        .unwrap();
    archive(&library, &a);
    archive(&library, &b);
    let plan = library
        .plan_archive_deletion(Some(&a.asset.id), &ctx)
        .unwrap();
    library
        .apply_archive_deletion(Some(&a.asset.id), &plan.plan_token, &ctx)
        .unwrap();
    assert!(owned.is_file());
    let plan = library
        .plan_archive_deletion(Some(&b.asset.id), &ctx)
        .unwrap();
    assert!(plan.warnings.is_empty());
    library
        .apply_archive_deletion(Some(&b.asset.id), &plan.plan_token, &ctx)
        .unwrap();
    assert!(!owned.exists());
    assert!(backups.join("legacy.glb").is_file());
}

#[test]
fn deletion_plan_rejects_workspace_edits_and_cancellation_rolls_back_every_target() {
    let temp = tempfile::tempdir().unwrap();
    let library = Library::open(temp.path().join("library")).unwrap();
    let ctx = OperationContext::default();
    let a = import(&library, temp.path(), "A", b"a");
    let b = import(&library, temp.path(), "B", b"b");
    archive(&library, &a);
    archive(&library, &b);
    let plan = library.plan_archive_deletion(None, &ctx).unwrap();
    fs::write(
        Path::new(&a.variants[0].workspace_path).join("new.txt"),
        b"new work",
    )
    .unwrap();
    assert!(
        library
            .apply_archive_deletion(None, &plan.plan_token, &ctx)
            .is_err()
    );
    let plan = library.plan_archive_deletion(None, &ctx).unwrap();
    let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = cancelled.clone();
    let ctx = OperationContext {
        cancelled,
        progress: Some(std::sync::Arc::new(move |event| {
            if event.phase == "Suppression : dossiers préservés" {
                flag.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        })),
    };
    assert!(
        library
            .apply_archive_deletion(None, &plan.plan_token, &ctx)
            .is_err()
    );
    for asset in [&a, &b] {
        assert!(library.get_asset(&asset.asset.id).unwrap().asset.archived);
    }
    assert_eq!(
        fs::read(Path::new(&a.variants[0].workspace_path).join("new.txt")).unwrap(),
        b"new work"
    );
    assert_eq!(
        fs::read_dir(library.root().join("staging"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn schema_three_upgrade_preserves_asset_history_and_unattributed_backup_files() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("library");
    let library = Library::open(&root).unwrap();
    let asset = import(&library, temp.path(), "Asset", b"preserved");
    fs::create_dir_all(root.join("backups/glb")).unwrap();
    fs::write(root.join("backups/glb/legacy.glb"), b"legacy backup").unwrap();
    drop(library);
    let db = rusqlite::Connection::open(root.join("library.sqlite3")).unwrap();
    db.execute_batch(
        "DROP TABLE file_selections; ALTER TABLE variants DROP COLUMN deleted_name; DROP TABLE glb_backup_owners; DROP TABLE archive_deletions; DROP TABLE workspace_discards; PRAGMA user_version=3;",
    )
    .unwrap();
    drop(db);
    let library = Library::open(&root).unwrap();
    assert_eq!(
        library.get_asset(&asset.asset.id).unwrap().asset.name,
        "Asset"
    );
    library
        .materialize(
            &asset.variants[0].head_snapshot_id,
            &temp.path().join("restored"),
            &OperationContext::default(),
        )
        .unwrap();
    assert_eq!(
        fs::read(temp.path().join("restored/main.blend")).unwrap(),
        b"preserved"
    );
    assert_eq!(
        fs::read(root.join("backups/glb/legacy.glb")).unwrap(),
        b"legacy backup"
    );
    let db = rusqlite::Connection::open(root.join("library.sqlite3")).unwrap();
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        6
    );
    let backup = fs::read_dir(root.join("backups"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|e| e == "sqlite3"))
        .unwrap();
    let old = rusqlite::Connection::open(backup).unwrap();
    assert_eq!(
        old.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
}

#[test]
fn cache_cleanup_preserves_shared_artifacts_survivor_indexes_and_unknown_files() {
    let temp = tempfile::tempdir().unwrap();
    let library = Library::open(temp.path().join("library")).unwrap();
    let a = import(&library, temp.path(), "A", b"a");
    let b = import(&library, temp.path(), "B", b"b");
    let cache = library.root().join("cache");
    for folder in ["previews", "sync", "canonical", "thumbnails", "exports"] {
        fs::create_dir_all(cache.join(folder)).unwrap();
    }
    let shared = "a".repeat(64);
    let exclusive = "b".repeat(64);
    let record = |asset: &edge_lock_core::AssetDetail, hash: &str| serde_json::json!({"version":2,"identity":"d".repeat(64),"converter":null,"sourceFile":"main.blend","sourceKind":"generatedGlb","artifact":hash,"artifactHash":"c".repeat(64),"sources":[{"path":Path::new(&asset.variants[0].workspace_path).join("main.blend").to_string_lossy(),"members":[]}]});
    fs::write(
        cache.join("previews/index-a.json"),
        serde_json::to_vec(&record(&a, &shared)).unwrap(),
    )
    .unwrap();
    fs::write(
        cache.join("previews/index-b.json"),
        serde_json::to_vec(&record(&b, &shared)).unwrap(),
    )
    .unwrap();
    fs::write(
        cache.join("previews/index-c.json"),
        serde_json::to_vec(&record(&a, &exclusive)).unwrap(),
    )
    .unwrap();
    fs::write(cache.join(format!("previews/{shared}.glb")), b"shared").unwrap();
    fs::write(
        cache.join(format!("previews/{exclusive}.glb")),
        b"exclusive",
    )
    .unwrap();
    fs::write(cache.join("exports/keep.txt"), b"user file").unwrap();
    let variant_key = blake3::hash(a.variants[0].id.as_bytes())
        .to_hex()
        .to_string();
    let snapshot_choice = cache.join(format!(
        "previews/choice-{variant_key}-{}.json",
        "e".repeat(64)
    ));
    let unknown_choice = cache.join(format!("previews/choice-{variant_key}-user-notes.json"));
    fs::write(&snapshot_choice, b"{}").unwrap();
    fs::write(&unknown_choice, b"keep").unwrap();
    fs::write(
        cache.join(format!("exports/{exclusive}.glb")),
        b"opaque legacy cache",
    )
    .unwrap();
    fs::write(cache.join(format!("exports/{exclusive}.json")), b"{}").unwrap();
    archive(&library, &a);
    let plan = library
        .plan_archive_deletion(None, &OperationContext::default())
        .unwrap();
    library
        .apply_archive_deletion(None, &plan.plan_token, &OperationContext::default())
        .unwrap();
    assert!(cache.join(format!("previews/{shared}.glb")).is_file());
    assert!(cache.join("previews/index-b.json").is_file());
    assert!(!cache.join("previews/index-a.json").exists());
    assert!(!snapshot_choice.exists());
    assert!(unknown_choice.is_file());
    assert!(!cache.join(format!("previews/{exclusive}.glb")).exists());
    assert!(!cache.join(format!("exports/{exclusive}.glb")).exists());
    assert_eq!(
        fs::read(cache.join("exports/keep.txt")).unwrap(),
        b"user file"
    );
}

#[test]
#[ignore = "subprocess helper for disposable archive crash fixtures"]
fn archive_crash_helper() {
    let Ok(root) = std::env::var("EDGE_LOCK_DELETE_ROOT") else {
        return;
    };
    let library = Library::open(root).unwrap();
    let phase = std::env::var("EDGE_LOCK_DELETE_PHASE").unwrap();
    let plan = library
        .plan_archive_deletion(None, &OperationContext::default())
        .unwrap();
    let ctx = OperationContext {
        progress: Some(std::sync::Arc::new(move |event| {
            if event.phase == phase {
                std::process::exit(92);
            }
        })),
        ..Default::default()
    };
    library
        .apply_archive_deletion(None, &plan.plan_token, &ctx)
        .unwrap();
    panic!("crash phase not reached");
}

#[test]
fn every_archive_deletion_boundary_recovers_after_process_exit() {
    for phase in [
        "Suppression : journal publié",
        "Suppression : dossiers préservés",
        "Suppression : catalogue publié",
        "Suppression : nettoyage terminé",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("library");
        let library = Library::open(&root).unwrap();
        let a = import(&library, temp.path(), "A", b"preserve until committed");
        let b = import(&library, temp.path(), "B", b"other owned work");
        archive(&library, &a);
        archive(&library, &b);
        drop(library);
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "archive_crash_helper",
                "--nocapture",
            ])
            .env("EDGE_LOCK_DELETE_ROOT", &root)
            .env("EDGE_LOCK_DELETE_PHASE", phase)
            .output()
            .unwrap();
        assert_eq!(
            result.status.code(),
            Some(92),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let library = Library::open(&root).unwrap();
        assert!(
            library.info().unwrap().recovery_notices.is_empty(),
            "{phase}"
        );
        let committed =
            phase == "Suppression : catalogue publié" || phase == "Suppression : nettoyage terminé";
        assert_eq!(
            library.info().unwrap().asset_count,
            if committed { 0 } else { 2 },
            "{phase}"
        );
        assert_eq!(
            Path::new(&a.variants[0].workspace_path).exists(),
            !committed,
            "{phase}"
        );
        drop(library);
        assert!(
            Library::open(&root)
                .unwrap()
                .info()
                .unwrap()
                .recovery_notices
                .is_empty()
        );
    }
}

#[cfg(windows)]
#[test]
fn locked_workspace_is_removed_logically_without_blocking_other_writes() {
    use std::os::windows::fs::OpenOptionsExt;
    let temp = tempfile::tempdir().unwrap();
    let library = Library::open(temp.path().join("library")).unwrap();
    let a = import(&library, temp.path(), "A", b"locked");
    archive(&library, &a);
    let ctx = OperationContext::default();
    let handle = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(Path::new(&a.variants[0].workspace_path).join("main.blend"))
        .unwrap();
    let plan = library.plan_archive_deletion(None, &ctx).unwrap();
    let report = library
        .apply_archive_deletion(None, &plan.plan_token, &ctx)
        .unwrap();
    assert!(report.cleanup_pending);
    assert!(
        report.released_bytes > 0,
        "Successful object cleanup must be counted even while the workspace is locked"
    );
    assert!(library.get_asset(&a.asset.id).is_err());
    library.save_category(None, "Still writable", None).unwrap();
    assert_eq!(library.info().unwrap().asset_count, 0);
    drop(handle);
    let root = library.root().to_path_buf();
    drop(library);
    let library = Library::open(root).unwrap();
    assert_eq!(
        fs::read_dir(library.root().join("staging"))
            .unwrap()
            .count(),
        0
    );
}

#[cfg(windows)]
#[test]
fn committed_deletion_reports_pending_cleanup_and_retries_idempotently_after_reopen() {
    use std::os::windows::fs::OpenOptionsExt;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("library");
    let library = Library::open(&root).unwrap();
    let a = import(&library, temp.path(), "A", b"exclusive asset bytes");
    archive(&library, &a);
    let snapshot = library
        .get_snapshot(&a.variants[0].head_snapshot_id)
        .unwrap();
    let hash = snapshot
        .entries
        .iter()
        .find_map(|e| e.hash.as_ref())
        .unwrap()
        .clone();
    let object = root.join("objects").join(&hash[..2]).join(&hash);
    let locked = std::sync::Arc::new(std::sync::Mutex::new(None));
    let handle = locked.clone();
    let ctx = OperationContext {
        progress: Some(std::sync::Arc::new(move |event| {
            if event.phase == "Suppression : catalogue publié" {
                *handle.lock().unwrap() = Some(
                    fs::OpenOptions::new()
                        .read(true)
                        .share_mode(0)
                        .open(&object)
                        .unwrap(),
                );
            }
        })),
        ..Default::default()
    };
    let plan = library.plan_archive_deletion(None, &ctx).unwrap();
    let report = library
        .apply_archive_deletion(None, &plan.plan_token, &ctx)
        .unwrap();
    assert!(report.cleanup_pending);
    assert_eq!(library.info().unwrap().asset_count, 0);
    assert!(!report.warnings.is_empty());
    assert!(
        library
            .plan_archive_deletion(None, &OperationContext::default())
            .is_ok()
    );
    drop(library);
    let library = Library::open(&root).unwrap();
    assert!(!library.info().unwrap().recovery_notices.is_empty());
    drop(library);
    locked.lock().unwrap().take();
    let library = Library::open(&root).unwrap();
    assert_eq!(library.info().unwrap().asset_count, 0);
    assert!(library.info().unwrap().recovery_notices.is_empty());
    assert_eq!(fs::read_dir(root.join("staging")).unwrap().count(), 0);
    drop(library);
    assert!(
        Library::open(&root)
            .unwrap()
            .info()
            .unwrap()
            .recovery_notices
            .is_empty()
    );
}

#[test]
fn unexpected_cross_asset_history_reference_refuses_deletion_and_preserves_both_assets() {
    let temp = tempfile::tempdir().unwrap();
    let library = Library::open(temp.path().join("library")).unwrap();
    let a = import(&library, temp.path(), "A", b"a");
    let b = import(&library, temp.path(), "B", b"b");
    archive(&library, &a);
    let db = rusqlite::Connection::open(library.root().join("library.sqlite3")).unwrap();
    db.execute(
        "UPDATE snapshots SET parent_id=?1 WHERE id=?2",
        rusqlite::params![
            a.variants[0].head_snapshot_id,
            b.variants[0].head_snapshot_id
        ],
    )
    .unwrap();
    assert!(
        library
            .plan_archive_deletion(None, &OperationContext::default())
            .is_err()
    );
    assert!(library.get_asset(&a.asset.id).is_ok());
    assert!(library.get_asset(&b.asset.id).is_ok());
}

#[test]
fn modified_committed_deletion_journal_is_preserved_until_original_is_restored() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("library");
    let library = Library::open(&root).unwrap();
    let asset = import(&library, temp.path(), "A", b"owned data");
    archive(&library, &asset);
    drop(library);
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "archive_crash_helper",
            "--nocapture",
        ])
        .env("EDGE_LOCK_DELETE_ROOT", &root)
        .env("EDGE_LOCK_DELETE_PHASE", "Suppression : catalogue publié")
        .output()
        .unwrap();
    assert_eq!(
        result.status.code(),
        Some(92),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let directory = fs::read_dir(root.join("staging"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("delete-")
        })
        .unwrap();
    let path = directory.join("delete.json");
    let original = fs::read(&path).unwrap();
    let mut altered: serde_json::Value = serde_json::from_slice(&original).unwrap();
    altered["candidates"]["a".repeat(64)] = serde_json::json!(123);
    fs::write(&path, serde_json::to_vec(&altered).unwrap()).unwrap();
    let library = Library::open(&root).unwrap();
    assert!(!library.info().unwrap().recovery_notices.is_empty());
    assert!(root.join("workspaces").join(&asset.asset.id).is_dir());
    assert!(
        library
            .plan_archive_deletion(None, &OperationContext::default())
            .is_ok()
    );
    library
        .save_category(None, "Unrelated write", None)
        .unwrap();
    drop(library);
    fs::write(path, original).unwrap();
    let library = Library::open(&root).unwrap();
    assert!(library.info().unwrap().recovery_notices.is_empty());
    assert_eq!(library.info().unwrap().asset_count, 0);
    assert!(!directory.exists());
}

#[test]
fn recreated_workspace_after_committed_crash_preserves_new_work_and_staged_originals() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("library");
    let library = Library::open(&root).unwrap();
    let asset = import(&library, temp.path(), "A", b"original owned work");
    archive(&library, &asset);
    drop(library);
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "archive_crash_helper",
            "--nocapture",
        ])
        .env("EDGE_LOCK_DELETE_ROOT", &root)
        .env("EDGE_LOCK_DELETE_PHASE", "Suppression : catalogue publié")
        .output()
        .unwrap();
    assert_eq!(
        result.status.code(),
        Some(92),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let journal = fs::read_dir(root.join("staging"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let recreated = root.join("workspaces").join(&asset.asset.id);
    fs::rename(&recreated, journal.join(&asset.asset.id)).unwrap();
    fs::create_dir_all(&recreated).unwrap();
    fs::write(recreated.join("new.txt"), b"new user work").unwrap();
    let library = Library::open(&root).unwrap();
    assert!(!library.info().unwrap().recovery_notices.is_empty());
    let journal = fs::read_dir(root.join("staging"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(journal.join(&asset.asset.id).is_dir());
    assert_eq!(
        fs::read(recreated.join("new.txt")).unwrap(),
        b"new user work"
    );
    drop(library);
    fs::rename(recreated, temp.path().join("recovered-user-work")).unwrap();
    let library = Library::open(&root).unwrap();
    assert!(library.info().unwrap().recovery_notices.is_empty());
    assert_eq!(
        fs::read(temp.path().join("recovered-user-work/new.txt")).unwrap(),
        b"new user work"
    );
    assert!(!journal.exists());
}

#[test]
fn unrelated_legacy_backup_storage_is_ignored_entirely() {
    let temp = tempfile::tempdir().unwrap();
    let library = Library::open(temp.path().join("library")).unwrap();
    let asset = import(&library, temp.path(), "Asset", b"owned");
    archive(&library, &asset);
    fs::create_dir_all(library.root().join("backups")).unwrap();
    let legacy = library.root().join("backups/glb");
    fs::write(&legacy, b"unrelated old storage").unwrap();
    let ctx = OperationContext::default();
    let plan = library.plan_archive_deletion(None, &ctx).unwrap();
    assert!(plan.warnings.is_empty());
    let report = library
        .apply_archive_deletion(None, &plan.plan_token, &ctx)
        .unwrap();
    assert!(!report.cleanup_pending);
    assert_eq!(fs::read(legacy).unwrap(), b"unrelated old storage");
}

#[cfg(windows)]
fn prevent_workspace_rename(path: &Path) -> fs::File {
    use std::os::windows::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .read(true)
        .share_mode(3)
        .custom_flags(0x02000000)
        .open(path)
        .unwrap()
}

#[cfg(windows)]
#[test]
fn locked_original_directory_does_not_block_other_archive_deletions() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("library");
    let library = Library::open(&root).unwrap();
    let first = import(&library, temp.path(), "First", b"first");
    archive(&library, &first);
    let original = root.join("workspaces").join(&first.asset.id);
    let handle = prevent_workspace_rename(&original);
    let ctx = OperationContext::default();
    let plan = library
        .plan_archive_deletion(Some(&first.asset.id), &ctx)
        .unwrap();
    assert!(plan.workspace_bytes_complete);
    let report = library
        .apply_archive_deletion(Some(&first.asset.id), &plan.plan_token, &ctx)
        .unwrap();
    assert!(report.cleanup_pending);
    assert!(original.is_dir());
    let second = import(&library, temp.path(), "Second", b"second");
    archive(&library, &second);
    let plan = library.plan_archive_deletion(None, &ctx).unwrap();
    let report = library
        .apply_archive_deletion(None, &plan.plan_token, &ctx)
        .unwrap();
    assert_eq!(report.deleted_asset_ids, vec![second.asset.id]);
    assert!(!report.cleanup_pending);
    assert_eq!(library.info().unwrap().asset_count, 0);
    drop(handle);
    drop(library);
    let library = Library::open(&root).unwrap();
    assert!(library.info().unwrap().recovery_notices.is_empty());
    assert!(!original.exists());
}

#[cfg(windows)]
#[test]
fn recreated_original_workspace_is_never_cleaned_after_a_locked_deletion() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("library");
    let library = Library::open(&root).unwrap();
    let asset = import(&library, temp.path(), "Asset", b"original");
    archive(&library, &asset);
    let original = root.join("workspaces").join(&asset.asset.id);
    let handle = prevent_workspace_rename(&original);
    let ctx = OperationContext::default();
    let plan = library.plan_archive_deletion(None, &ctx).unwrap();
    assert!(
        library
            .apply_archive_deletion(None, &plan.plan_token, &ctx)
            .unwrap()
            .cleanup_pending
    );
    drop(handle);
    drop(library);
    fs::rename(&original, temp.path().join("recovered-original")).unwrap();
    fs::create_dir(&original).unwrap();
    fs::write(original.join("new.txt"), b"new work").unwrap();
    let library = Library::open(&root).unwrap();
    assert_eq!(fs::read(original.join("new.txt")).unwrap(), b"new work");
    assert!(!library.info().unwrap().recovery_notices.is_empty());
    library.save_category(None, "Still writable", None).unwrap();
}

#[cfg(windows)]
#[test]
fn uninspectable_workspace_is_removed_logically_and_kept_for_manual_recovery() {
    use std::os::windows::fs::OpenOptionsExt;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("library");
    let library = Library::open(&root).unwrap();
    let asset = import(&library, temp.path(), "Asset", b"preserved");
    archive(&library, &asset);
    let original = root.join("workspaces").join(&asset.asset.id);
    let handle = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .custom_flags(0x02000000)
        .open(&original)
        .unwrap();
    let ctx = OperationContext::default();
    let plan = library.plan_archive_deletion(None, &ctx).unwrap();
    assert!(!plan.workspace_bytes_complete);
    assert!(
        library
            .apply_archive_deletion(None, &plan.plan_token, &ctx)
            .unwrap()
            .cleanup_pending
    );
    let info = library.info().unwrap();
    assert_eq!(info.asset_count, 0);
    assert!(!info.physical_bytes_complete);
    library
        .save_category(None, "Unrelated write", None)
        .unwrap();
    drop(handle);
    drop(library);
    let library = Library::open(&root).unwrap();
    assert!(original.is_dir());
    assert!(!library.info().unwrap().recovery_notices.is_empty());
}

#[cfg(windows)]
#[test]
fn edits_to_a_retained_workspace_are_preserved_on_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("library");
    let library = Library::open(&root).unwrap();
    let asset = import(&library, temp.path(), "Asset", b"original");
    archive(&library, &asset);
    let original = root.join("workspaces").join(&asset.asset.id);
    let handle = prevent_workspace_rename(&original);
    let ctx = OperationContext::default();
    let plan = library.plan_archive_deletion(None, &ctx).unwrap();
    assert!(
        library
            .apply_archive_deletion(None, &plan.plan_token, &ctx)
            .unwrap()
            .cleanup_pending
    );
    drop(handle);
    let file = Path::new(&asset.variants[0].workspace_path).join("main.blend");
    fs::write(&file, b"new work after deletion").unwrap();
    drop(library);
    let library = Library::open(&root).unwrap();
    assert_eq!(fs::read(file).unwrap(), b"new work after deletion");
    assert!(!library.info().unwrap().recovery_notices.is_empty());
}

#[test]
fn committed_v1_journal_remains_authenticated_and_cleans_staged_workspaces() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("library");
    let library = Library::open(&root).unwrap();
    let asset = import(&library, temp.path(), "Asset", b"legacy owned files");
    archive(&library, &asset);
    drop(library);
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "archive_crash_helper",
            "--nocapture",
        ])
        .env("EDGE_LOCK_DELETE_ROOT", &root)
        .env("EDGE_LOCK_DELETE_PHASE", "Suppression : catalogue publié")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(92));
    let directory = fs::read_dir(root.join("staging"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    fs::rename(
        root.join("workspaces").join(&asset.asset.id),
        directory.join(&asset.asset.id),
    )
    .unwrap();
    let path = directory.join("delete.json");
    let mut journal: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    journal["version"] = serde_json::json!(1);
    for target in journal["targets"].as_array_mut().unwrap() {
        target.as_object_mut().unwrap().remove("workspace_evidence");
    }
    let bytes = serde_json::to_vec(&journal).unwrap();
    fs::write(path, &bytes).unwrap();
    let db = rusqlite::Connection::open(root.join("library.sqlite3")).unwrap();
    db.execute(
        "UPDATE archive_deletions SET journal_hash=?1 WHERE id=?2",
        rusqlite::params![
            blake3::hash(&bytes).to_hex().to_string(),
            journal["id"].as_str().unwrap()
        ],
    )
    .unwrap();
    drop(db);
    let library = Library::open(&root).unwrap();
    assert!(library.info().unwrap().recovery_notices.is_empty());
    assert!(!directory.exists());
}
