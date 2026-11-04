use edge_lock_core::{AssetFilter, ExportConfig, FileSelectionKind, Library, OperationContext};
use std::{
    fs,
    path::Path,
    sync::{Arc, atomic::Ordering},
};

fn fixture() -> (tempfile::TempDir, Library, edge_lock_core::AssetDetail) {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("main.blend"), b"original").unwrap();
    let library = Library::open(temp.path().join("library")).unwrap();
    let asset = library
        .import_folder(
            &source,
            "Asset",
            ExportConfig::default(),
            &OperationContext::default(),
        )
        .unwrap();
    (temp, library, asset)
}

#[test]
fn renames_persist_without_changing_content_and_shared_versions_keep_their_identity() {
    let (temp, library, asset) = fixture();
    let original = &asset.variants[0];
    let ctx = OperationContext::default();
    let fork = library
        .create_variant(&original.head_snapshot_id, "Fork", &ctx)
        .unwrap();
    let before = library.get_snapshot(&original.head_snapshot_id).unwrap();
    library
        .rename_snapshot(&before.id, "  Shared original  ")
        .unwrap();
    library.rename_variant(&fork.id, "  Blue  ").unwrap();
    assert!(library.rename_variant(&fork.id, "Principale").is_err());
    assert!(library.rename_snapshot(&before.id, "\n").is_err());
    let after = library.get_snapshot(&before.id).unwrap();
    assert_eq!(after.message, "Shared original");
    assert_eq!(before.entries, after.entries);
    assert_eq!(before.parent_id, after.parent_id);
    assert_eq!(
        library
            .list_snapshots(&fork.id, 0, 50)
            .unwrap()
            .items
            .iter()
            .find(|snapshot| snapshot.id == before.id)
            .unwrap()
            .message,
        "Shared original"
    );
    drop(library);
    let reopened = Library::open(temp.path().join("library")).unwrap();
    let renamed = reopened.get_variant(&fork.id).unwrap().variant;
    assert_eq!(renamed.name, "Blue");
    assert_eq!(renamed.workspace_path, fork.workspace_path);
    assert_eq!(renamed.head_snapshot_id, fork.head_snapshot_id);
    assert_eq!(
        reopened.get_snapshot(&before.id).unwrap().message,
        "Shared original"
    );
}

#[test]
fn deleting_parent_keeps_fork_ancestry_and_releases_only_exclusive_history() {
    let (temp, library, asset) = fixture();
    let ctx = OperationContext::default();
    let original = &asset.variants[0];
    let fork = library
        .create_variant(&original.head_snapshot_id, "Fork", &ctx)
        .unwrap();
    fs::write(
        Path::new(&original.workspace_path).join("main.blend"),
        b"exclusive",
    )
    .unwrap();
    let exclusive = library
        .create_snapshot(&original.id, "Private", &ctx)
        .unwrap();
    let plan = library.plan_variant_deletion(&original.id, &ctx).unwrap();
    assert_eq!(plan.snapshot_count, 1);
    assert_eq!(plan.retained_snapshot_count, 1);
    let report = library
        .apply_variant_deletion(&original.id, &plan.plan_token, &ctx)
        .unwrap();
    assert_eq!(report.deleted_variant_ids, vec![original.id.clone()]);
    assert!(report.deleted_asset_ids.is_empty());
    assert!(!report.cleanup_pending, "{:?}", report.warnings);
    assert!(!Path::new(&original.workspace_path).exists());
    assert!(library.get_variant(&original.id).is_err());
    assert!(library.workspace_path(&original.id).is_err());
    assert!(library.list_snapshots(&original.id, 0, 50).is_err());
    assert_eq!(
        library.get_snapshot(&exclusive.id).unwrap().storage.kind,
        "deleted"
    );
    assert_eq!(
        library
            .get_snapshot(&original.head_snapshot_id)
            .unwrap()
            .storage
            .kind,
        "full"
    );
    let detail = library.get_asset(&asset.asset.id).unwrap();
    assert_eq!(detail.variants.len(), 1);
    assert_eq!(detail.asset.variant_count, 1);
    assert_eq!(detail.asset.working_bytes, fork.working_bytes);
    let output = temp.path().join("copy");
    library
        .materialize(&original.head_snapshot_id, &output, &ctx)
        .unwrap();
    assert_eq!(fs::read(output.join("main.blend")).unwrap(), b"original");
    library
        .rename_snapshot(&original.head_snapshot_id, "Retained ancestor")
        .unwrap();
    let replacement = library
        .create_variant(&original.head_snapshot_id, "Principale", &ctx)
        .unwrap();
    assert_eq!(replacement.name, "Principale");
    drop(library);
    let reopened = Library::open(temp.path().join("library")).unwrap();
    assert_eq!(
        reopened.get_asset(&asset.asset.id).unwrap().variants.len(),
        2
    );
    assert!(reopened.info().unwrap().recovery_notices.is_empty());
    assert_eq!(
        reopened.list_assets(AssetFilter::default()).unwrap().items[0].variant_count,
        2
    );
}

#[test]
fn last_variant_and_stale_confirmation_cannot_be_deleted() {
    let (_temp, library, asset) = fixture();
    let ctx = OperationContext::default();
    let main = &asset.variants[0];
    assert!(library.plan_variant_deletion(&main.id, &ctx).is_err());
    let fork = library
        .create_variant(&main.head_snapshot_id, "Fork", &ctx)
        .unwrap();
    let plan = library.plan_variant_deletion(&fork.id, &ctx).unwrap();
    library.rename_variant(&fork.id, "Changed").unwrap();
    assert!(
        library
            .apply_variant_deletion(&fork.id, &plan.plan_token, &ctx)
            .is_err()
    );
    assert!(Path::new(&fork.workspace_path).exists());
    let plan = library.plan_variant_deletion(&fork.id, &ctx).unwrap();
    fs::write(Path::new(&fork.workspace_path).join("new.txt"), b"new").unwrap();
    assert!(
        library
            .apply_variant_deletion(&fork.id, &plan.plan_token, &ctx)
            .is_err()
    );
    assert_eq!(
        library.get_asset(&asset.asset.id).unwrap().variants.len(),
        2
    );
}

#[test]
fn unexpected_cross_asset_ancestry_blocks_variant_deletion() {
    let (temp, library, asset) = fixture();
    let ctx = OperationContext::default();
    let main = &asset.variants[0];
    library
        .create_variant(&main.head_snapshot_id, "Fork", &ctx)
        .unwrap();
    let other = library
        .import_folder(
            &temp.path().join("source"),
            "Other",
            ExportConfig::default(),
            &ctx,
        )
        .unwrap();
    let db = rusqlite::Connection::open(library.root().join("library.sqlite3")).unwrap();
    db.execute(
        "UPDATE snapshots SET parent_id=?1 WHERE id=?2",
        [&main.head_snapshot_id, &other.variants[0].head_snapshot_id],
    )
    .unwrap();
    assert!(library.plan_variant_deletion(&main.id, &ctx).is_err());
    assert!(library.get_variant(&main.id).is_ok());
    assert!(Path::new(&main.workspace_path).exists());
}

#[test]
fn deleting_last_reference_releases_previously_shared_deleted_owner_data() {
    let (_temp, library, asset) = fixture();
    let ctx = OperationContext::default();
    let main = &asset.variants[0];
    let independent = library
        .create_variant(&main.head_snapshot_id, "Independent", &ctx)
        .unwrap();
    fs::write(
        Path::new(&main.workspace_path).join("main.blend"),
        b"private branch",
    )
    .unwrap();
    let next = library.create_snapshot(&main.id, "New", &ctx).unwrap();
    let fork = library.create_variant(&next.id, "Fork", &ctx).unwrap();
    let plan = library.plan_variant_deletion(&main.id, &ctx).unwrap();
    library
        .apply_variant_deletion(&main.id, &plan.plan_token, &ctx)
        .unwrap();
    assert_eq!(library.get_snapshot(&next.id).unwrap().storage.kind, "full");
    let plan = library.plan_variant_deletion(&fork.id, &ctx).unwrap();
    library
        .apply_variant_deletion(&fork.id, &plan.plan_token, &ctx)
        .unwrap();
    assert_eq!(
        library.get_snapshot(&next.id).unwrap().storage.kind,
        "deleted"
    );
    assert!(library.get_snapshot(&next.id).unwrap().entries.is_empty());
    assert_eq!(
        library
            .get_snapshot(&main.head_snapshot_id)
            .unwrap()
            .storage
            .kind,
        "full"
    );
    assert_eq!(
        library.get_asset(&asset.asset.id).unwrap().variants[0].id,
        independent.id
    );
}

#[test]
fn deletion_removes_its_preferences_and_preserves_survivor_choices_and_shared_objects() {
    let (temp, library, asset) = fixture();
    let ctx = OperationContext::default();
    let main = &asset.variants[0];
    let fork = library
        .create_variant(&main.head_snapshot_id, "Fork", &ctx)
        .unwrap();
    for variant in [main, &fork] {
        library
            .set_file_selection(
                &variant.id,
                None,
                FileSelectionKind::Preview,
                Some("main.blend"),
            )
            .unwrap();
        library
            .set_file_selection(
                &variant.id,
                Some(&main.head_snapshot_id),
                FileSelectionKind::Export,
                Some("main.blend"),
            )
            .unwrap();
    }
    let plan = library.plan_variant_deletion(&main.id, &ctx).unwrap();
    library
        .apply_variant_deletion(&main.id, &plan.plan_token, &ctx)
        .unwrap();
    assert_eq!(
        library
            .get_file_selections(&fork.id, None)
            .unwrap()
            .preview_file
            .as_deref(),
        Some("main.blend")
    );
    assert_eq!(
        library
            .get_file_selections(&fork.id, Some(&main.head_snapshot_id))
            .unwrap()
            .export_file
            .as_deref(),
        Some("main.blend")
    );
    let db = rusqlite::Connection::open(library.root().join("library.sqlite3")).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM file_selections WHERE variant_id=?1",
            [&main.id],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    drop(db);
    // The whole-asset archive workflow also cleans retained owner rows and choices.
    library
        .update_asset(&asset.asset.id, "Asset", None, vec![], vec![], true)
        .unwrap();
    let plan = library
        .plan_archive_deletion(Some(&asset.asset.id), &ctx)
        .unwrap();
    library
        .apply_archive_deletion(Some(&asset.asset.id), &plan.plan_token, &ctx)
        .unwrap();
    drop(library);
    let reopened = Library::open(temp.path().join("library")).unwrap();
    assert_eq!(reopened.info().unwrap().asset_count, 0);
}

#[test]
fn archive_confirmation_counts_visible_variants_and_retained_versions_only() {
    let (_temp, library, asset) = fixture();
    let ctx = OperationContext::default();
    let main = &asset.variants[0];
    let fork = library
        .create_variant(&main.head_snapshot_id, "Fork", &ctx)
        .unwrap();
    fs::write(
        Path::new(&main.workspace_path).join("main.blend"),
        b"private",
    )
    .unwrap();
    let private = library
        .create_snapshot(&main.id, "Exclusive", &ctx)
        .unwrap();
    let plan = library.plan_variant_deletion(&main.id, &ctx).unwrap();
    library
        .apply_variant_deletion(&main.id, &plan.plan_token, &ctx)
        .unwrap();
    assert_eq!(
        library.get_snapshot(&private.id).unwrap().storage.kind,
        "deleted"
    );
    library
        .update_asset(&asset.asset.id, "Asset", None, vec![], vec![], true)
        .unwrap();
    let plan = library
        .plan_archive_deletion(Some(&asset.asset.id), &ctx)
        .unwrap();
    assert_eq!(plan.variant_count, 1);
    assert_eq!(plan.snapshot_count, 2);
    // Hidden immutable owners remain internal cleanup targets.
    assert_eq!(plan.variant_ids.len(), 2);
    assert!(plan.variant_ids.contains(&main.id));
    assert!(plan.variant_ids.contains(&fork.id));
    library
        .apply_archive_deletion(Some(&asset.asset.id), &plan.plan_token, &ctx)
        .unwrap();
    assert_eq!(library.info().unwrap().asset_count, 0);
    let db = rusqlite::Connection::open(library.root().join("library.sqlite3")).unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM variants", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn cancellation_before_catalogue_publication_preserves_the_variant() {
    let (_temp, library, asset) = fixture();
    let ctx = OperationContext::default();
    let main = &asset.variants[0];
    let fork = library
        .create_variant(&main.head_snapshot_id, "Fork", &ctx)
        .unwrap();
    let plan = library.plan_variant_deletion(&fork.id, &ctx).unwrap();
    let cancelled = ctx.cancelled.clone();
    let ctx = OperationContext {
        progress: Some(Arc::new(move |event| {
            if event.phase == "Suppression : dossiers préservés" {
                cancelled.store(true, Ordering::Relaxed);
            }
        })),
        ..ctx
    };
    assert!(
        library
            .apply_variant_deletion(&fork.id, &plan.plan_token, &ctx)
            .is_err()
    );
    assert!(library.get_variant(&fork.id).is_ok());
    assert!(Path::new(&fork.workspace_path).exists());
    assert!(library.info().unwrap().recovery_notices.is_empty());
    assert_eq!(
        fs::read_dir(library.root().join("staging"))
            .unwrap()
            .count(),
        0
    );
}

#[cfg(windows)]
#[test]
fn locked_variant_cleanup_is_deferred_and_recovers_on_reopen() {
    use std::os::windows::fs::OpenOptionsExt;
    let (temp, library, asset) = fixture();
    let ctx = OperationContext::default();
    let main = &asset.variants[0];
    let fork = library
        .create_variant(&main.head_snapshot_id, "Fork", &ctx)
        .unwrap();
    let handle = fs::OpenOptions::new()
        .read(true)
        .share_mode(3)
        .custom_flags(0x02000000)
        .open(&fork.workspace_path)
        .unwrap();
    let plan = library.plan_variant_deletion(&fork.id, &ctx).unwrap();
    let report = library
        .apply_variant_deletion(&fork.id, &plan.plan_token, &ctx)
        .unwrap();
    assert!(report.cleanup_pending);
    assert!(library.get_variant(&fork.id).is_err());
    assert!(Path::new(&fork.workspace_path).exists());
    library.rename_variant(&main.id, "Still usable").unwrap();
    drop(handle);
    drop(library);
    let reopened = Library::open(temp.path().join("library")).unwrap();
    assert!(reopened.info().unwrap().recovery_notices.is_empty());
    assert!(!Path::new(&fork.workspace_path).exists());
    assert_eq!(
        reopened.get_variant(&main.id).unwrap().variant.name,
        "Still usable"
    );
}

#[test]
#[ignore = "subprocess helper for disposable variant deletion crash fixtures"]
fn variant_crash_helper() {
    let Ok(root) = std::env::var("EDGE_LOCK_VARIANT_CRASH_ROOT") else {
        return;
    };
    let library = Library::open(root).unwrap();
    let id = std::env::var("EDGE_LOCK_VARIANT_CRASH_ID").unwrap();
    let phase = std::env::var("EDGE_LOCK_VARIANT_CRASH_PHASE").unwrap();
    let plan = library
        .plan_variant_deletion(&id, &OperationContext::default())
        .unwrap();
    let ctx = OperationContext {
        progress: Some(Arc::new(move |event| {
            if event.phase == phase {
                std::process::exit(94);
            }
        })),
        ..Default::default()
    };
    library
        .apply_variant_deletion(&id, &plan.plan_token, &ctx)
        .unwrap();
    panic!("Crash boundary not reached");
}

#[test]
fn variant_deletion_recovers_each_publication_boundary_after_process_exit() {
    for (phase, committed) in [
        ("Suppression : journal publié", false),
        ("Suppression : dossiers préservés", false),
        ("Suppression : catalogue publié", true),
    ] {
        let (temp, library, asset) = fixture();
        let main = &asset.variants[0];
        let fork = library
            .create_variant(&main.head_snapshot_id, "Fork", &OperationContext::default())
            .unwrap();
        drop(library);
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "variant_crash_helper",
                "--nocapture",
            ])
            .env("EDGE_LOCK_VARIANT_CRASH_ROOT", temp.path().join("library"))
            .env("EDGE_LOCK_VARIANT_CRASH_ID", &fork.id)
            .env("EDGE_LOCK_VARIANT_CRASH_PHASE", phase)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(94),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let reopened = Library::open(temp.path().join("library")).unwrap();
        assert_eq!(
            reopened.get_variant(&fork.id).is_err(),
            committed,
            "{phase}"
        );
        assert_eq!(
            Path::new(&fork.workspace_path).exists(),
            !committed,
            "{phase}"
        );
        assert!(reopened.get_variant(&main.id).is_ok());
        assert!(reopened.info().unwrap().recovery_notices.is_empty());
    }
}
