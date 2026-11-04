use edge_lock_core::{ExportConfig, FileSelectionKind, Library, OperationContext};
use std::{fs, path::Path};

#[test]
fn selections_survive_reopen_and_are_independent_from_versioned_configuration() {
    let temporary = tempfile::tempdir().unwrap();
    let input = temporary.path().join("input");
    fs::create_dir(&input).unwrap();
    fs::write(input.join("main.blend"), b"blend").unwrap();
    fs::write(input.join("export.glb"), b"glb").unwrap();
    let root = temporary.path().join("library");
    let library = Library::open(&root).unwrap();
    let ctx = OperationContext::default();
    let asset = library
        .import_folder(&input, "Asset", ExportConfig::default(), &ctx)
        .unwrap();
    let variant = &asset.variants[0];
    let before = library.get_snapshot(&variant.head_snapshot_id).unwrap();
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
            None,
            FileSelectionKind::Export,
            Some("export.glb"),
        )
        .unwrap();
    library
        .set_file_selection(
            &variant.id,
            Some(&variant.head_snapshot_id),
            FileSelectionKind::Preview,
            Some("export.glb"),
        )
        .unwrap();
    let changes = library.changes(&variant.id, &ctx).unwrap();
    assert!(!changes.config_changed);
    assert!(changes.added.is_empty() && changes.modified.is_empty() && changes.deleted.is_empty());
    assert_eq!(
        library.get_snapshot(&before.id).unwrap().entries,
        before.entries
    );
    assert_eq!(
        library.get_snapshot(&before.id).unwrap().config,
        before.config
    );
    let variant_id = variant.id.clone();
    let snapshot_id = variant.head_snapshot_id.clone();
    drop(library);
    let library = Library::open(&root).unwrap();
    let current = library.get_file_selections(&variant_id, None).unwrap();
    assert_eq!(current.preview_file.as_deref(), Some("main.blend"));
    assert_eq!(current.export_file.as_deref(), Some("export.glb"));
    let historical = library
        .get_file_selections(&variant_id, Some(&snapshot_id))
        .unwrap();
    assert_eq!(historical.preview_file.as_deref(), Some("export.glb"));
    assert_eq!(historical.export_file, None);
    library
        .set_file_selection(&variant_id, None, FileSelectionKind::Export, None)
        .unwrap();
    assert_eq!(
        library
            .get_file_selections(&variant_id, None)
            .unwrap()
            .export_file,
        None
    );
    fs::remove_file(
        Path::new(
            &library
                .get_variant(&variant_id)
                .unwrap()
                .variant
                .workspace_path,
        )
        .join("main.blend"),
    )
    .unwrap();
    assert_eq!(
        library
            .get_file_selections(&variant_id, None)
            .unwrap()
            .preview_file
            .as_deref(),
        Some("main.blend")
    );
}

#[test]
fn inherited_versions_have_independent_choices_and_unrelated_scopes_are_rejected() {
    let temporary = tempfile::tempdir().unwrap();
    let input = temporary.path().join("input");
    fs::create_dir(&input).unwrap();
    fs::write(input.join("main.blend"), b"blend").unwrap();
    let library = Library::open(temporary.path().join("library")).unwrap();
    let ctx = OperationContext::default();
    let asset = library
        .import_folder(&input, "Asset", ExportConfig::default(), &ctx)
        .unwrap();
    let original = &asset.variants[0];
    let fork = library
        .create_variant(&original.head_snapshot_id, "Fork", &ctx)
        .unwrap();
    library
        .set_file_selection(
            &fork.id,
            Some(&original.head_snapshot_id),
            FileSelectionKind::Preview,
            Some("main.blend"),
        )
        .unwrap();
    assert_eq!(
        library
            .get_file_selections(&original.id, Some(&original.head_snapshot_id))
            .unwrap()
            .preview_file,
        None
    );
    assert!(
        library
            .get_file_selections(&original.id, Some(&fork.head_snapshot_id))
            .is_err()
    );
    assert!(
        library
            .set_file_selection(
                &original.id,
                None,
                FileSelectionKind::Preview,
                Some("../main.blend")
            )
            .is_err()
    );
    assert!(
        library
            .set_file_selection(
                &original.id,
                None,
                FileSelectionKind::Preview,
                Some("missing.blend")
            )
            .is_err()
    );
    assert!(
        library
            .set_file_selection(
                &original.id,
                None,
                FileSelectionKind::Preview,
                Some("main.blend1")
            )
            .is_err()
    );
}

#[test]
fn schema_five_upgrade_preserves_existing_files_history_and_configuration() {
    let temporary = tempfile::tempdir().unwrap();
    let input = temporary.path().join("input");
    fs::create_dir(&input).unwrap();
    fs::write(input.join("main.blend"), b"existing model").unwrap();
    let root = temporary.path().join("library");
    let library = Library::open(&root).unwrap();
    let asset = library
        .import_folder(
            &input,
            "Existing",
            ExportConfig::default(),
            &OperationContext::default(),
        )
        .unwrap();
    let variant = asset.variants[0].clone();
    let snapshot = library.get_snapshot(&variant.head_snapshot_id).unwrap();
    drop(library);
    {
        let db = rusqlite::Connection::open(root.join("library.sqlite3")).unwrap();
        db.execute_batch("DROP TABLE file_selections; ALTER TABLE variants DROP COLUMN deleted_name; PRAGMA user_version=5;").unwrap();
    }
    let library = Library::open(&root).unwrap();
    assert_eq!(
        library.get_variant(&variant.id).unwrap().variant.config,
        variant.config
    );
    let after = library.get_snapshot(&snapshot.id).unwrap();
    assert_eq!(after.entries, snapshot.entries);
    assert_eq!(after.config, snapshot.config);
    assert_eq!(
        fs::read(Path::new(&variant.workspace_path).join("main.blend")).unwrap(),
        b"existing model"
    );
    assert_eq!(
        library
            .get_file_selections(&variant.id, None)
            .unwrap()
            .preview_file,
        None
    );
    library
        .set_file_selection(
            &variant.id,
            None,
            FileSelectionKind::Preview,
            Some("main.blend"),
        )
        .unwrap();
    let backups = fs::read_dir(root.join("backups"))
        .unwrap()
        .filter_map(|entry| entry.ok())
        .collect::<Vec<_>>();
    assert!(backups.iter().any(|entry| {
        entry
            .file_name()
            .to_string_lossy()
            .starts_with("catalog-schema-5-")
    }));
    let db = rusqlite::Connection::open(root.join("library.sqlite3")).unwrap();
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        6
    );
}
