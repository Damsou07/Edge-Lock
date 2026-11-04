use edge_lock_core::{ExportConfig, Library, OperationContext};
use std::{fs, path::Path};

#[test]
fn workspace_scan_persists_a_missing_primary_fallback_without_rewriting_history() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("main.blend"), b"main").unwrap();
    fs::write(
        source.join("alternative.blend"),
        b"larger alternative source",
    )
    .unwrap();
    fs::write(source.join("main.glb"), b"user managed output").unwrap();
    let library_path = root.path().join("library");
    let library = Library::open(&library_path).unwrap();
    let ctx = OperationContext::default();
    let asset = library
        .import_folder(
            &source,
            "Fallback",
            ExportConfig {
                primary_file: Some("main.blend".into()),
                generated_glb_file: Some("main.glb".into()),
                ..ExportConfig::default()
            },
            &ctx,
        )
        .unwrap();
    let variant = &asset.variants[0];
    library.check_workspace(&variant.id, &ctx).unwrap();
    assert_eq!(
        library
            .get_variant(&variant.id)
            .unwrap()
            .variant
            .config
            .primary_file
            .as_deref(),
        Some("main.blend")
    );
    fs::remove_file(Path::new(&variant.workspace_path).join("main.blend")).unwrap();
    let checked = library.check_workspace(&variant.id, &ctx).unwrap();
    assert!(checked.changes.config_changed);
    let resolved = library.get_variant(&variant.id).unwrap().variant.config;
    assert_eq!(resolved.primary_file.as_deref(), Some("alternative.blend"));
    assert!(resolved.generated_glb_file.is_none());
    assert_eq!(
        fs::read(Path::new(&variant.workspace_path).join("main.glb")).unwrap(),
        b"user managed output"
    );
    drop(library);
    let reopened = Library::open(library_path).unwrap();
    assert_eq!(
        reopened.get_variant(&variant.id).unwrap().variant.config,
        resolved
    );
    let historical = reopened.get_snapshot(&variant.head_snapshot_id).unwrap();
    assert_eq!(historical.config, variant.config);
    assert!(
        historical
            .entries
            .iter()
            .any(|entry| entry.path == "main.blend")
    );
}

#[test]
fn workspace_scan_tracks_former_managed_glbs_and_same_size_edits_without_blender() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("main.blend"), b"saved blend").unwrap();
    fs::write(source.join("main.glb"), b"old glb").unwrap();
    let library = Library::open(root.path().join("library")).unwrap();
    let ctx = OperationContext::default();
    let asset = library
        .import_folder(
            &source,
            "Legacy",
            ExportConfig {
                primary_file: Some("main.blend".into()),
                generated_glb_file: Some("main.glb".into()),
                ..ExportConfig::default()
            },
            &ctx,
        )
        .unwrap();
    let variant = &asset.variants[0];
    let work = Path::new(&variant.workspace_path);
    let first = library.check_workspace(&variant.id, &ctx).unwrap();
    assert!(!first.changes.has_source_changes());
    assert_eq!(
        first.fingerprint,
        library
            .check_workspace(&variant.id, &ctx)
            .unwrap()
            .fingerprint
    );
    fs::write(work.join("main.glb"), b"new glb").unwrap();
    let edited = library.check_workspace(&variant.id, &ctx).unwrap();
    assert_ne!(first.fingerprint, edited.fingerprint);
    assert_eq!(edited.changes.modified, ["main.glb"]);
    library
        .create_snapshot(&variant.id, "User GLB", &ctx)
        .unwrap();
    assert!(
        !library
            .changes(&variant.id, &ctx)
            .unwrap()
            .has_source_changes()
    );
    fs::remove_file(work.join("main.glb")).unwrap();
    assert_eq!(
        library.changes(&variant.id, &ctx).unwrap().deleted,
        ["main.glb"]
    );
    assert_eq!(fs::read(work.join("main.blend")).unwrap(), b"saved blend");
    assert_eq!(fs::read(source.join("main.glb")).unwrap(), b"old glb");
}
