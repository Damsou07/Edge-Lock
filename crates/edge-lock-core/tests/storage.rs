use edge_lock_core::{AssetFilter, ExportConfig, Library, OperationContext};
use std::{fs, path::Path};

fn source(root: &Path) {
    fs::create_dir_all(root.join("textures/empty")).unwrap();
    fs::write(root.join("model.blend"), [0, 1, 2, 255]).unwrap();
    fs::write(root.join("textures/café.png"), b"texture").unwrap();
}

#[test]
fn principal_uses_asset_name_and_supports_redetection_and_versioned_manual_choice() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("source");
    source(&input);
    fs::write(input.join("a.blend"), [0, 1, 2, 255]).unwrap();
    for name in ["model.blend", "a.blend"] {
        fs::File::options()
            .write(true)
            .open(input.join(name))
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(std::time::SystemTime::UNIX_EPOCH))
            .unwrap();
    }
    let library = Library::open(temp.path().join("library")).unwrap();
    let ctx = OperationContext::default();
    let asset = library
        .import_folder(&input, "Model", ExportConfig::default(), &ctx)
        .unwrap();
    let variant = &asset.variants[0];
    assert_eq!(variant.config.primary_file.as_deref(), Some("model.blend"));
    let work = Path::new(&variant.workspace_path);
    fs::write(work.join("a.blend"), vec![2; 1000]).unwrap();
    library
        .update_config(
            &variant.id,
            ExportConfig {
                primary_file: None,
                ..variant.config.clone()
            },
        )
        .unwrap();
    let automatic = library.get_variant(&variant.id).unwrap().variant.config;
    assert_eq!(automatic.primary_file.as_deref(), Some("a.blend"));
    library
        .create_snapshot(&variant.id, "Automatic detection", &ctx)
        .unwrap();
    library
        .update_config(
            &variant.id,
            ExportConfig {
                primary_file: Some("model.blend".into()),
                ..automatic
            },
        )
        .unwrap();
    assert!(library.changes(&variant.id, &ctx).unwrap().config_changed);
    let manual = library
        .create_snapshot(&variant.id, "Manual principal", &ctx)
        .unwrap();
    assert_eq!(manual.config.primary_file.as_deref(), Some("model.blend"));
    fs::remove_file(work.join("model.blend")).unwrap();
    let capture = library
        .capture_working(&variant.id, &temp.path().join("capture"), &ctx)
        .unwrap();
    assert_eq!(capture.config.primary_file.as_deref(), Some("a.blend"));
}

#[test]
fn imported_snapshots_round_trip_without_mutating_the_source() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("source");
    source(&input);
    let library = Library::open(temp.path().join("library")).unwrap();
    let ctx = OperationContext::default();
    let asset = library
        .import_folder(&input, "Rock", ExportConfig::default(), &ctx)
        .unwrap();
    let variant = &asset.variants[0];
    let snapshot = library.get_snapshot(&variant.head_snapshot_id).unwrap();
    assert_eq!(snapshot.logical_bytes, 11);
    assert!(
        snapshot
            .entries
            .iter()
            .any(|entry| entry.path == "textures/empty" && entry.kind == "directory")
    );
    let output = temp.path().join("restored");
    library.materialize(&snapshot.id, &output, &ctx).unwrap();
    assert_eq!(
        fs::read(output.join("model.blend")).unwrap(),
        [0, 1, 2, 255]
    );
    assert!(output.join("textures/empty").is_dir());
    fs::write(
        Path::new(&variant.workspace_path).join("model.blend"),
        b"changed",
    )
    .unwrap();
    assert_eq!(fs::read(input.join("model.blend")).unwrap(), [0, 1, 2, 255]);
    assert_eq!(
        fs::read(output.join("model.blend")).unwrap(),
        [0, 1, 2, 255]
    );
}

#[test]
fn changes_versions_and_variants_are_isolated_and_deduplicated() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("source");
    source(&input);
    let library = Library::open(temp.path().join("library")).unwrap();
    let ctx = OperationContext::default();
    let asset = library
        .import_folder(&input, "Rock", ExportConfig::default(), &ctx)
        .unwrap();
    let variant = &asset.variants[0];
    let original = library.get_snapshot(&variant.head_snapshot_id).unwrap();
    let work = Path::new(&variant.workspace_path);
    fs::write(work.join("model.blend"), b"changed").unwrap();
    fs::remove_file(work.join("textures/café.png")).unwrap();
    fs::write(work.join("new.txt"), b"new").unwrap();
    let changes = library.changes(&variant.id, &ctx).unwrap();
    assert_eq!(changes.added, ["new.txt"]);
    assert_eq!(changes.modified, ["model.blend"]);
    assert_eq!(changes.deleted, ["textures/café.png"]);
    let next = library
        .create_snapshot(&variant.id, "Update", &ctx)
        .unwrap();
    assert_eq!(next.parent_id.as_deref(), Some(original.id.as_str()));
    assert!(
        library
            .create_snapshot(&variant.id, "No change", &ctx)
            .is_err()
    );
    let fork = library.create_variant(&original.id, "Blue", &ctx).unwrap();
    assert_eq!(
        fs::read(Path::new(&fork.workspace_path).join("model.blend")).unwrap(),
        [0, 1, 2, 255]
    );
    assert_ne!(fork.workspace_path, variant.workspace_path);
    assert!(library.changes(&fork.id, &ctx).unwrap().modified.is_empty());
    assert_eq!(
        library.list_assets(AssetFilter::default()).unwrap().total,
        1
    );
}

#[test]
fn a_derived_glb_change_alone_never_creates_a_version() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("source");
    source(&input);
    fs::write(input.join("model.glb"), b"old derived output").unwrap();
    let library = Library::open(temp.path().join("library")).unwrap();
    let ctx = OperationContext::default();
    let asset = library
        .import_folder(
            &input,
            "Derived",
            ExportConfig {
                generated_glb_file: Some("model.glb".into()),
                ..Default::default()
            },
            &ctx,
        )
        .unwrap();
    let variant = &asset.variants[0];
    let work = Path::new(&variant.workspace_path);
    fs::write(work.join("model.glb"), b"regenerated output").unwrap();
    let changes = library.changes(&variant.id, &ctx).unwrap();
    assert!(changes.modified.is_empty());
    assert!(
        library
            .create_snapshot(&variant.id, "Only GLB", &ctx)
            .is_err()
    );
    fs::write(work.join("model.blend"), b"source edit").unwrap();
    let snapshot = library
        .create_snapshot(&variant.id, "Source edit", &ctx)
        .unwrap();
    assert!(
        snapshot
            .entries
            .iter()
            .any(|entry| entry.path == "model.glb")
    );
}

#[test]
fn refuses_unrelated_directories_nested_imports_and_overwriting_outputs() {
    let temp = tempfile::tempdir().unwrap();
    let other = temp.path().join("personal");
    fs::create_dir(&other).unwrap();
    fs::write(other.join("keep.txt"), b"keep").unwrap();
    assert!(Library::open(&other).is_err());
    let root = temp.path().join("library");
    let library = Library::open(&root).unwrap();
    assert!(Library::open(&root).is_err());
    let ctx = OperationContext::default();
    assert!(
        library
            .import_folder(&root, "Bad", ExportConfig::default(), &ctx)
            .is_err()
    );
    assert!(
        library
            .import_folder(temp.path(), "Bad", ExportConfig::default(), &ctx)
            .is_err()
    );
    let source_dir = temp.path().join("source");
    source(&source_dir);
    let asset = library
        .import_folder(&source_dir, "Rock", ExportConfig::default(), &ctx)
        .unwrap();
    assert!(
        library
            .materialize(&asset.variants[0].head_snapshot_id, &other, &ctx)
            .is_err()
    );
    assert_eq!(fs::read(other.join("keep.txt")).unwrap(), b"keep");
}

#[test]
fn taxonomy_metadata_and_reopen_preserve_history() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("library");
    let library = Library::open(&root).unwrap();
    let ctx = OperationContext::default();
    let asset = library
        .create_empty("Chair", ExportConfig::default(), &ctx)
        .unwrap();
    let parent = library.save_category(None, "Props", None).unwrap();
    let child = library
        .save_category(None, "Furniture", Some(&parent.id))
        .unwrap();
    assert!(
        library
            .save_category(Some(&parent.id), "Props", Some(&child.id))
            .is_err()
    );
    let game = library.save_game(None, "Demo").unwrap();
    library
        .update_asset(
            &asset.asset.id,
            "Chair 02",
            Some(&child.id),
            vec!["wood".into(), "wood".into()],
            vec![game.id.clone()],
            false,
        )
        .unwrap();
    drop(library);
    let library = Library::open(&root).unwrap();
    let found = library.get_asset(&asset.asset.id).unwrap();
    assert_eq!(found.asset.name, "Chair 02");
    assert_eq!(found.asset.tags, ["wood"]);
    assert_eq!(found.asset.game_ids, [game.id]);
    assert_eq!(found.variants.len(), 1);
    assert_eq!(library.taxonomy().unwrap().categories.len(), 2);
}

#[test]
fn cancelled_import_never_publishes_an_asset() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("source");
    source(&input);
    let library = Library::open(temp.path().join("library")).unwrap();
    let ctx = OperationContext::default();
    ctx.cancelled
        .store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(
        library
            .import_folder(&input, "Cancelled", ExportConfig::default(), &ctx)
            .is_err()
    );
    assert_eq!(library.info().unwrap().asset_count, 0);
    assert!(input.join("model.blend").exists());
}

fn object_files(path: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for prefix in fs::read_dir(path).unwrap() {
        for file in fs::read_dir(prefix.unwrap().path()).unwrap() {
            files.push(file.unwrap().path());
        }
    }
    files.sort();
    files
}

#[test]
fn immutable_history_is_deduplicated_and_working_capture_stays_ephemeral() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("source");
    source(&input);
    let library = Library::open(temp.path().join("library")).unwrap();
    let ctx = OperationContext::default();
    let first = library
        .import_folder(&input, "Rock", ExportConfig::default(), &ctx)
        .unwrap();
    library
        .import_folder(&input, "Second", ExportConfig::default(), &ctx)
        .unwrap();
    assert_eq!(object_files(&library.root().join("objects")).len(), 2);
    let variant = &first.variants[0];
    fs::write(
        Path::new(&variant.workspace_path).join("model.blend"),
        b"ephemeral-change",
    )
    .unwrap();
    let output = temp.path().join("capture");
    library.capture_working(&variant.id, &output, &ctx).unwrap();
    assert_eq!(
        fs::read(output.join("model.blend")).unwrap(),
        b"ephemeral-change"
    );
    assert_eq!(object_files(&library.root().join("objects")).len(), 2);
    assert_eq!(library.get_variant(&variant.id).unwrap().snapshots.len(), 1);
    let snapshot = library.create_snapshot(&variant.id, "Save", &ctx).unwrap();
    assert_eq!(object_files(&library.root().join("objects")).len(), 3);
    assert!(
        library
            .get_variant(&variant.id)
            .unwrap()
            .snapshots
            .iter()
            .all(|snapshot| snapshot.entries.is_empty())
    );
    assert!(
        !library
            .get_snapshot(&snapshot.id)
            .unwrap()
            .entries
            .is_empty()
    );
}

#[test]
fn corrupt_objects_never_publish_a_partial_restore() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("source");
    source(&input);
    let library = Library::open(temp.path().join("library")).unwrap();
    let ctx = OperationContext::default();
    let asset = library
        .import_folder(&input, "Rock", ExportConfig::default(), &ctx)
        .unwrap();
    let objects = object_files(&library.root().join("objects"));
    fs::write(&objects[0], b"corrupt").unwrap();
    let output = temp.path().join("restore");
    assert!(
        library
            .materialize(&asset.variants[0].head_snapshot_id, &output, &ctx)
            .is_err()
    );
    assert!(!output.exists());
    assert!(
        Path::new(&asset.variants[0].workspace_path)
            .join("model.blend")
            .is_file()
    );
}

#[test]
fn legacy_config_alone_does_not_enable_a_version_and_category_filter_includes_descendants() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("source");
    source(&input);
    let library = Library::open(temp.path().join("library")).unwrap();
    let ctx = OperationContext::default();
    let asset = library
        .import_folder(&input, "Rock", ExportConfig::default(), &ctx)
        .unwrap();
    let variant = &asset.variants[0];
    let config = ExportConfig {
        primary_file: Some("model.blend".into()),
        collection: Some("Export".into()),
        ..Default::default()
    };
    library.update_config(&variant.id, config.clone()).unwrap();
    assert!(!library.changes(&variant.id, &ctx).unwrap().config_changed);
    assert!(
        library
            .create_snapshot(&variant.id, "Only settings", &ctx)
            .is_err()
    );
    fs::write(
        Path::new(&variant.workspace_path).join("report.txt"),
        b"source change",
    )
    .unwrap();
    let version = library
        .create_snapshot(&variant.id, "Export settings", &ctx)
        .unwrap();
    assert_eq!(version.config, config);
    assert!(!library.changes(&variant.id, &ctx).unwrap().config_changed);
    let parent = library.save_category(None, "Props", None).unwrap();
    let child = library
        .save_category(None, "Rocks", Some(&parent.id))
        .unwrap();
    let game = library.save_game(None, "Adventure").unwrap();
    library
        .update_asset(
            &asset.asset.id,
            "Rock",
            Some(&child.id),
            vec!["stone".into()],
            vec![game.id.clone()],
            false,
        )
        .unwrap();
    let filter = AssetFilter {
        category_id: Some(parent.id),
        game_id: Some(game.id),
        tag: Some("stone".into()),
        search: "sto".into(),
        ..Default::default()
    };
    assert_eq!(library.list_assets(filter).unwrap().total, 1);
    library
        .update_asset(
            &asset.asset.id,
            "Rock",
            Some(&child.id),
            vec![],
            vec![],
            true,
        )
        .unwrap();
    assert_eq!(
        library.list_assets(AssetFilter::default()).unwrap().total,
        0
    );
    assert_eq!(
        library
            .list_assets(AssetFilter {
                archived: true,
                ..Default::default()
            })
            .unwrap()
            .total,
        1
    );
}

#[test]
fn interrupted_publication_preserves_workspace_and_is_reported_after_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("library");
    let library = Library::open(&root).unwrap();
    // Trigger cancellation after the final workspace-copy callback, before DB commit.
    let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let callback_flag = cancelled.clone();
    let ctx = OperationContext {
        cancelled,
        progress: Some(std::sync::Arc::new(move |event| {
            if event.phase == "materialize" && event.total == Some(event.completed) {
                callback_flag.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        })),
    };
    assert!(
        library
            .create_empty("Interrupted", ExportConfig::default(), &ctx)
            .is_err()
    );
    assert_eq!(library.info().unwrap().asset_count, 0);
    drop(library);
    let library = Library::open(&root).unwrap();
    let info = library.info().unwrap();
    assert_eq!(info.asset_count, 0);
    assert_eq!(info.recovery_notices.len(), 1);
    assert_eq!(fs::read_dir(root.join("workspaces")).unwrap().count(), 1);
}

#[test]
fn refuses_future_schemas_missing_catalogs_and_unsafe_primary_paths() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("library");
    let library = Library::open(&root).unwrap();
    let input = temp.path().join("source");
    source(&input);
    let invalid = ExportConfig {
        primary_file: Some("../outside.blend".into()),
        ..Default::default()
    };
    assert!(
        library
            .import_folder(&input, "Unsafe", invalid, &OperationContext::default())
            .is_err()
    );
    assert_eq!(library.info().unwrap().asset_count, 0);
    drop(library);
    {
        let db = rusqlite::Connection::open(root.join("library.sqlite3")).unwrap();
        db.execute_batch("PRAGMA user_version=999;").unwrap();
    }
    assert!(Library::open(&root).is_err());
    {
        let db = rusqlite::Connection::open(root.join("library.sqlite3")).unwrap();
        let version: i64 = db
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 999);
    }
    fs::rename(
        root.join("library.sqlite3"),
        root.join("saved-catalog.sqlite3"),
    )
    .unwrap();
    assert!(Library::open(&root).is_err());
    assert!(!root.join("library.sqlite3").exists());
}

#[test]
fn files_changed_during_capture_do_not_publish_an_inconsistent_version() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("source");
    fs::create_dir(&input).unwrap();
    let changing_file = input.join("changing.blend");
    fs::write(&changing_file, vec![0u8; 32]).unwrap();
    let mutations = Arc::new(AtomicUsize::new(0));
    let callback_count = mutations.clone();
    let ctx = OperationContext {
        progress: Some(Arc::new(move |event| {
            if event.phase != "capture" {
                return;
            }
            let count = callback_count.fetch_add(1, Ordering::Relaxed) + 1;
            fs::write(&changing_file, vec![count as u8; 32]).unwrap();
            let file = fs::OpenOptions::new()
                .write(true)
                .open(&changing_file)
                .unwrap();
            file.set_times(fs::FileTimes::new().set_modified(
                std::time::UNIX_EPOCH + std::time::Duration::from_secs(count as u64),
            ))
            .unwrap();
        })),
        ..Default::default()
    };
    let library = Library::open(temp.path().join("library")).unwrap();
    assert!(
        library
            .import_folder(&input, "Changing", ExportConfig::default(), &ctx)
            .is_err()
    );
    assert_eq!(library.info().unwrap().asset_count, 0);
    assert_eq!(mutations.load(Ordering::Relaxed), 2);
}

#[test]
fn history_pagination_reaches_old_versions_and_fork_ancestors() {
    let temp = tempfile::tempdir().unwrap();
    let library = Library::open(temp.path().join("library")).unwrap();
    let ctx = OperationContext::default();
    let asset = library
        .create_empty("History", ExportConfig::default(), &ctx)
        .unwrap();
    let variant = &asset.variants[0];
    let initial_id = variant.head_snapshot_id.clone();
    let mut latest = None;
    for index in 0..110 {
        fs::write(
            Path::new(&variant.workspace_path).join("report.txt"),
            format!("version {index}"),
        )
        .unwrap();
        latest = Some(
            library
                .create_snapshot(&variant.id, &format!("v{index}"), &ctx)
                .unwrap(),
        );
    }
    assert_eq!(
        library.get_variant(&variant.id).unwrap().snapshots.len(),
        100
    );
    let last_page = library.list_snapshots(&variant.id, 100, 50).unwrap();
    assert_eq!(last_page.total, 111);
    assert_eq!(last_page.items.len(), 11);
    assert_eq!(last_page.items.last().unwrap().id, initial_id);
    let fork = library
        .create_variant(&latest.unwrap().id, "Fork", &ctx)
        .unwrap();
    let inherited = library.list_snapshots(&fork.id, 100, 50).unwrap();
    assert_eq!(inherited.total, 112);
    assert_eq!(inherited.items.len(), 12);
    assert_eq!(inherited.items.last().unwrap().id, initial_id);
    assert!(
        inherited
            .items
            .iter()
            .all(|snapshot| snapshot.entries.is_empty())
    );
    assert!(
        library
            .list_snapshots(&fork.id, 999, 50)
            .unwrap()
            .items
            .is_empty()
    );
}

#[test]
fn object_write_failure_preserves_the_previous_head_and_editable_files() {
    let temp = tempfile::tempdir().unwrap();
    let library = Library::open(temp.path().join("library")).unwrap();
    let ctx = OperationContext::default();
    let asset = library
        .create_empty("IO failure", ExportConfig::default(), &ctx)
        .unwrap();
    let variant = &asset.variants[0];
    let contents = b"bytes whose object directory is blocked";
    let hash = blake3::hash(contents).to_hex().to_string();
    let obstruction = library.root().join("objects").join(&hash[..2]);
    fs::write(&obstruction, b"disposable obstruction").unwrap();
    let working = Path::new(&variant.workspace_path).join("edited.txt");
    fs::write(&working, contents).unwrap();
    assert!(library.create_snapshot(&variant.id, "Fail", &ctx).is_err());
    let detail = library.get_variant(&variant.id).unwrap();
    assert_eq!(detail.variant.head_snapshot_id, variant.head_snapshot_id);
    assert_eq!(detail.snapshots.len(), 1);
    assert_eq!(fs::read(&working).unwrap(), contents);
    fs::remove_file(obstruction).unwrap();
    assert!(library.create_snapshot(&variant.id, "Retry", &ctx).is_ok());
}

#[cfg(windows)]
#[test]
fn locked_source_file_fails_without_publishing_or_changing_source() {
    use std::os::windows::fs::OpenOptionsExt;
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("source");
    source(&input);
    let library = Library::open(temp.path().join("library")).unwrap();
    let locked = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(input.join("model.blend"))
        .unwrap();
    assert!(
        library
            .import_folder(
                &input,
                "Locked",
                ExportConfig::default(),
                &OperationContext::default()
            )
            .is_err()
    );
    assert_eq!(library.info().unwrap().asset_count, 0);
    drop(locked);
    assert_eq!(fs::read(input.join("model.blend")).unwrap(), [0, 1, 2, 255]);
}

#[test]
fn existing_schema_zero_is_backed_up_and_migrated_without_losing_records() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("library");
    fs::create_dir(&root).unwrap();
    fs::write(root.join(".edge-lock-library"), b"edge-lock-library-v1\n").unwrap();
    for directory in ["objects", "staging", "workspaces"] {
        fs::create_dir(root.join(directory)).unwrap();
    }
    {
        let db = rusqlite::Connection::open(root.join("library.sqlite3")).unwrap();
        db.execute_batch("CREATE TABLE legacy_settings(key TEXT PRIMARY KEY,value TEXT); INSERT INTO legacy_settings VALUES('favorite','preserved'); PRAGMA user_version=0;").unwrap();
    }
    let library = Library::open(&root).unwrap();
    assert_eq!(library.info().unwrap().asset_count, 0);
    drop(library);
    let db = rusqlite::Connection::open(root.join("library.sqlite3")).unwrap();
    let preserved: String = db
        .query_row(
            "SELECT value FROM legacy_settings WHERE key='favorite'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let version: i64 = db
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(preserved, "preserved");
    assert_eq!(version, 5);
    let backup = fs::read_dir(root.join("backups"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let old = rusqlite::Connection::open(backup).unwrap();
    let old_version: i64 = old
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    let preserved: String = old
        .query_row(
            "SELECT value FROM legacy_settings WHERE key='favorite'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(old_version, 0);
    assert_eq!(preserved, "preserved");
}

#[test]
fn interrupted_initialization_resumes_only_its_known_owned_state() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("library");
    fs::create_dir(&root).unwrap();
    fs::write(
        root.join(".edge-lock-initializing"),
        b"edge-lock-library-v1\n",
    )
    .unwrap();
    fs::create_dir(root.join("objects")).unwrap();
    let library = Library::open(&root).unwrap();
    assert_eq!(library.info().unwrap().asset_count, 0);
    assert!(root.join(".edge-lock-library").is_file());
    assert!(!root.join(".edge-lock-initializing").exists());
    let unrelated = temp.path().join("unrelated");
    fs::create_dir(&unrelated).unwrap();
    fs::write(
        unrelated.join(".edge-lock-initializing"),
        b"edge-lock-library-v1\n",
    )
    .unwrap();
    fs::write(unrelated.join("precious.txt"), b"keep").unwrap();
    assert!(Library::open(&unrelated).is_err());
    assert_eq!(fs::read(unrelated.join("precious.txt")).unwrap(), b"keep");
}

#[test]
fn primary_selection_prefers_blend_and_resolves_multiple_sources_stably() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("source");
    source(&input);
    let library = Library::open(temp.path().join("library")).unwrap();
    let ctx = OperationContext::default();
    let first = library
        .import_folder(&input, "Unique blend", ExportConfig::default(), &ctx)
        .unwrap();
    assert_eq!(
        first.variants[0].config.primary_file.as_deref(),
        Some("model.blend")
    );
    #[cfg(windows)]
    {
        assert!(!first.variants[0].workspace_path.starts_with(r"\\?\"));
        assert!(!library.info().unwrap().root.starts_with(r"\\?\"));
    }
    fs::write(input.join("second.BLEND"), b"second").unwrap();
    fs::write(input.join("model.glb"), b"placeholder").unwrap();
    let ambiguous = library
        .import_folder(&input, "Ambiguous", ExportConfig::default(), &ctx)
        .unwrap();
    assert_eq!(
        ambiguous.variants[0].config.primary_file.as_deref(),
        Some("second.BLEND")
    );
    let explicit = ExportConfig {
        primary_file: Some("model.glb".into()),
        ..Default::default()
    };
    library
        .update_config(&ambiguous.variants[0].id, explicit)
        .unwrap();
    assert_eq!(
        library
            .get_variant(&ambiguous.variants[0].id)
            .unwrap()
            .variant
            .config
            .primary_file
            .as_deref(),
        Some("model.glb")
    );
    fs::remove_file(input.join("model.blend")).unwrap();
    fs::remove_file(input.join("second.BLEND")).unwrap();
    let glb = library
        .import_folder(&input, "Only GLB", ExportConfig::default(), &ctx)
        .unwrap();
    assert_eq!(
        glb.variants[0].config.primary_file.as_deref(),
        Some("model.glb")
    );
}

#[test]
fn a_new_primary_is_resolved_when_capturing_work_and_saved_in_the_next_version() {
    let temp = tempfile::tempdir().unwrap();
    let library = Library::open(temp.path().join("library")).unwrap();
    let ctx = OperationContext::default();
    let asset = library
        .create_empty("Created later", ExportConfig::default(), &ctx)
        .unwrap();
    let variant = &asset.variants[0];
    fs::write(
        Path::new(&variant.workspace_path).join("models/new.blend"),
        b"new",
    )
    .unwrap();
    let capture = library
        .capture_working(&variant.id, &temp.path().join("derived"), &ctx)
        .unwrap();
    assert_eq!(
        capture.config.primary_file.as_deref(),
        Some("models/new.blend")
    );
    assert_eq!(
        library.get_variant(&variant.id).unwrap().variant.config,
        capture.config
    );
    let snapshot = library
        .create_snapshot(&variant.id, "First model", &ctx)
        .unwrap();
    assert_eq!(snapshot.config, capture.config);
}

#[test]
fn variant_size_and_geometry_are_independent_and_config_changes_invalidate_only_the_target() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("source");
    source(&input);
    let library = Library::open(temp.path().join("library")).unwrap();
    let ctx = OperationContext::default();
    let asset = library
        .import_folder(&input, "Counts", ExportConfig::default(), &ctx)
        .unwrap();
    let main = &asset.variants[0];
    let fork = library
        .create_variant(&main.head_snapshot_id, "Second", &ctx)
        .unwrap();
    library.set_geometry(&main.id, 111).unwrap();
    library.set_geometry(&fork.id, 222).unwrap();
    assert_eq!(
        library.get_variant(&main.id).unwrap().variant.triangles,
        Some(111)
    );
    assert_eq!(
        library.get_variant(&fork.id).unwrap().variant.triangles,
        Some(222)
    );
    assert_eq!(main.working_bytes, 11);
    library
        .update_config(&main.id, main.config.clone())
        .unwrap();
    assert_eq!(
        library.get_variant(&main.id).unwrap().variant.triangles,
        Some(111)
    );
    let changed = ExportConfig {
        collection: Some("New collection".into()),
        ..main.config.clone()
    };
    library.update_config(&main.id, changed).unwrap();
    assert_eq!(
        library.get_variant(&main.id).unwrap().variant.triangles,
        None
    );
    assert_eq!(
        library.get_variant(&fork.id).unwrap().variant.triangles,
        Some(222)
    );
    fs::write(Path::new(&fork.workspace_path).join("added.txt"), b"longer").unwrap();
    library.changes(&fork.id, &ctx).unwrap();
    assert_eq!(
        library.get_variant(&fork.id).unwrap().variant.working_bytes,
        17
    );
    assert_eq!(
        library.get_variant(&main.id).unwrap().variant.working_bytes,
        11
    );
}
