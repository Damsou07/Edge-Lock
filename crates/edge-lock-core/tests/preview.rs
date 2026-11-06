use edge_lock_core::{
    ExportConfig, Library, OperationContext,
    preview::{PreviewOutcome, PreviewRequest, prepare_preview},
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

fn triangle(path: &Path) {
    let mut json = serde_json::to_vec(&serde_json::json!({
        "asset":{"version":"2.0"},"buffers":[{"byteLength":36}],
        "bufferViews":[{"buffer":0,"byteLength":36}],
        "accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"}],
        "meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}],
        "nodes":[{"mesh":0}],"scenes":[{"nodes":[0]}]
    }))
    .unwrap();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let mut data = Vec::new();
    data.extend_from_slice(b"glTF");
    data.extend_from_slice(&2u32.to_le_bytes());
    data.extend_from_slice(&((12 + 8 + json.len() + 8 + 36) as u32).to_le_bytes());
    data.extend_from_slice(&(json.len() as u32).to_le_bytes());
    data.extend_from_slice(&0x4e4f534au32.to_le_bytes());
    data.extend_from_slice(&json);
    data.extend_from_slice(&36u32.to_le_bytes());
    data.extend_from_slice(&0x004e4942u32.to_le_bytes());
    data.extend_from_slice(&[0; 36]);
    fs::write(path, data).unwrap();
}

#[test]
fn sibling_glb_wins_without_blender_and_cache_survives_irrelevant_changes() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("main.blend"), b"original blend").unwrap();
    triangle(&source.join("Kitsune_GameReady.glb"));
    let library = Library::open(dir.path().join("library")).unwrap();
    let ctx = OperationContext::default();
    let asset = library
        .import_folder(&source, "Kitsune", ExportConfig::default(), &ctx)
        .unwrap();
    let variant = &asset.variants[0];
    library.set_geometry(&variant.id, 123).unwrap();
    let request = || PreviewRequest {
        variant_id: &variant.id,
        snapshot_id: None,
        preview_file: None,
        blender_path: None,
    };
    let PreviewOutcome::Ready(first) = prepare_preview(&library, request(), &ctx).unwrap() else {
        panic!("expected preview")
    };
    assert_eq!(first.source_file, "Kitsune_GameReady.glb");
    assert_eq!(
        first.path.file_stem().unwrap().to_str().unwrap(),
        first.artifact_key
    );
    assert!(!first.updates_geometry);
    assert_eq!(first.inspection.geometry.edges, Some(3));
    let modified = fs::metadata(&first.path).unwrap().modified().unwrap();
    fs::write(
        Path::new(&variant.workspace_path).join("report.txt"),
        b"new report",
    )
    .unwrap();
    let PreviewOutcome::Ready(second) = prepare_preview(&library, request(), &ctx).unwrap() else {
        panic!("expected preview")
    };
    assert_eq!(first.path, second.path);
    assert_eq!(first.artifact_key, second.artifact_key);
    assert_eq!(
        fs::metadata(&second.path).unwrap().modified().unwrap(),
        modified
    );
    assert_eq!(
        library.get_variant(&variant.id).unwrap().variant.triangles,
        Some(123)
    );
    assert_eq!(
        fs::read(source.join("main.blend")).unwrap(),
        b"original blend"
    );
    assert_eq!(
        fs::read(source.join("Kitsune_GameReady.glb")).unwrap(),
        fs::read(&second.path).unwrap()
    );
    fs::write(&second.path, b"corrupt cache").unwrap();
    let PreviewOutcome::Ready(repaired) = prepare_preview(&library, request(), &ctx).unwrap()
    else {
        panic!("expected preview")
    };
    assert_eq!(
        fs::read(&repaired.path).unwrap(),
        fs::read(source.join("Kitsune_GameReady.glb")).unwrap()
    );
}

#[test]
fn refresh_uses_the_last_verified_preview_of_the_same_source() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("main.blend"), b"blend").unwrap();
    triangle(&source.join("one.glb"));
    triangle(&source.join("two.glb"));
    let library = Library::open(dir.path().join("library")).unwrap();
    let ctx = OperationContext::default();
    let asset = library
        .import_folder(&source, "Asset", ExportConfig::default(), &ctx)
        .unwrap();
    let variant = &asset.variants[0];
    let workspace = Path::new(&variant.workspace_path);
    let prepare = |source, context: &OperationContext| {
        prepare_preview(
            &library,
            PreviewRequest {
                variant_id: &variant.id,
                snapshot_id: None,
                preview_file: Some(source),
                blender_path: None,
            },
            context,
        )
    };
    let PreviewOutcome::Ready(first) = prepare("one.glb", &ctx).unwrap() else {
        panic!("expected preview")
    };
    let index_before = tree_hashes(&library.root().join("cache/previews"));
    fs::write(workspace.join("one.glb"), b"unfinished GLB").unwrap();
    fs::write(workspace.join("two.glb"), b"unfinished second source").unwrap();
    let PreviewOutcome::Ready(previous) = prepare("one.glb", &ctx).unwrap() else {
        panic!("expected previous preview")
    };
    assert_eq!(previous.path, first.path);
    assert_eq!(previous.artifact_key, first.artifact_key);
    assert_eq!(previous.source_file, "one.glb");
    assert!(previous.stale);
    assert!(!previous.updates_geometry);
    assert_eq!(
        tree_hashes(&library.root().join("cache/previews")),
        index_before
    );
    assert_eq!(
        fs::read(workspace.join("one.glb")).unwrap(),
        b"unfinished GLB"
    );
    assert!(prepare("two.glb", &ctx).is_err());

    let cancelled = OperationContext::default();
    cancelled.cancelled.store(true, Ordering::Relaxed);
    assert!(matches!(
        prepare("one.glb", &cancelled),
        Err(edge_lock_core::CoreError::Cancelled)
    ));
    fs::write(&first.path, b"corrupt cached GLB").unwrap();
    assert!(prepare("one.glb", &ctx).is_err());
}

#[cfg(windows)]
#[test]
fn locked_temporary_directory_does_not_block_a_cache_hit() {
    use std::os::windows::fs::OpenOptionsExt;

    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("main.blend"), b"blend").unwrap();
    triangle(&source.join("main.glb"));
    let library = Library::open(dir.path().join("library")).unwrap();
    let ctx = OperationContext::default();
    let asset = library
        .import_folder(&source, "Asset", ExportConfig::default(), &ctx)
        .unwrap();
    let prepare = || {
        prepare_preview(
            &library,
            PreviewRequest {
                variant_id: &asset.variants[0].id,
                snapshot_id: None,
                preview_file: None,
                blender_path: None,
            },
            &ctx,
        )
    };
    let PreviewOutcome::Ready(first) = prepare().unwrap() else {
        panic!("expected preview")
    };
    let locked = library.root().join("cache/previews/.preview-tmp-locked");
    fs::create_dir(&locked).unwrap();
    let marker = locked.join(".edge-lock-preview-temporary");
    fs::write(&marker, b"edge-lock-preview-v2").unwrap();
    let handle = fs::OpenOptions::new()
        .read(true)
        .share_mode(3)
        .custom_flags(0x02000000)
        .open(&locked)
        .unwrap();
    let PreviewOutcome::Ready(cached) = prepare().unwrap() else {
        panic!("expected cached preview")
    };
    assert_eq!(cached.artifact_key, first.artifact_key);
    assert!(locked.is_dir());
    drop(handle);
    prepare().unwrap();
    assert!(!locked.exists());
}

#[test]
fn ambiguous_glbs_require_explicit_asset_relative_choice() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("main.blend"), b"blend").unwrap();
    triangle(&source.join("one.glb"));
    triangle(&source.join("two.glb"));
    let library = Library::open(dir.path().join("library")).unwrap();
    let ctx = OperationContext::default();
    let asset = library
        .import_folder(&source, "choices", ExportConfig::default(), &ctx)
        .unwrap();
    let id = &asset.variants[0].id;
    let request = |preview_file| PreviewRequest {
        variant_id: id,
        snapshot_id: None,
        preview_file,
        blender_path: None,
    };
    assert!(
        matches!(prepare_preview(&library,request(None),&ctx).unwrap(),PreviewOutcome::Choose(files) if files==["one.glb","two.glb"])
    );
    assert!(matches!(
        prepare_preview(&library, request(Some("two.glb")), &ctx).unwrap(),
        PreviewOutcome::Ready(_)
    ));
    assert!(prepare_preview(&library, request(Some("../outside.glb")), &ctx).is_err());
    fs::remove_file(Path::new(&asset.variants[0].workspace_path).join("two.glb")).unwrap();
    triangle(&Path::new(&asset.variants[0].workspace_path).join("main.glb"));
    let PreviewOutcome::Ready(preview) = prepare_preview(&library, request(None), &ctx).unwrap()
    else {
        panic!("matching name should win")
    };
    assert_eq!(preview.source_file, "main.glb");
}

#[test]
fn current_preview_choice_is_remembered_and_explicit_choice_has_priority() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    triangle(&source.join("one.glb"));
    triangle(&source.join("two.glb"));
    let library = Library::open(root.path().join("library")).unwrap();
    let ctx = OperationContext::default();
    let asset = library
        .import_folder(&source, "Choices", ExportConfig::default(), &ctx)
        .unwrap();
    let variant = &asset.variants[0];
    let config = variant.config.clone();
    let preview = |selected| {
        prepare_preview(
            &library,
            PreviewRequest {
                variant_id: &variant.id,
                snapshot_id: None,
                preview_file: selected,
                blender_path: None,
            },
            &ctx,
        )
        .unwrap()
    };
    let PreviewOutcome::Ready(chosen) = preview(Some("two.glb")) else {
        panic!("explicit choice")
    };
    let PreviewOutcome::Ready(reused) = preview(None) else {
        panic!("remembered choice must resolve the ambiguity")
    };
    assert_eq!(chosen.path, reused.path);
    assert_eq!(reused.source_file, "two.glb");
    assert!(
        matches!(preview(Some("one.glb")), PreviewOutcome::Ready(artifact) if artifact.source_file == "one.glb")
    );
    assert!(
        matches!(preview(None), PreviewOutcome::Ready(artifact) if artifact.source_file == "one.glb")
    );
    assert_eq!(
        library.get_variant(&variant.id).unwrap().variant.config,
        config
    );
    fs::remove_file(Path::new(&variant.workspace_path).join("one.glb")).unwrap();
    assert!(
        matches!(preview(None), PreviewOutcome::Ready(artifact) if artifact.source_file == "two.glb")
    );
    assert_eq!(
        fs::read(source.join("one.glb")).unwrap(),
        fs::read(source.join("two.glb")).unwrap()
    );
}

#[test]
fn explicit_blend_preview_uses_the_selected_source_without_changing_export_config() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("main.blend"), b"main blend").unwrap();
    fs::write(source.join("alternative.blend"), b"alternative blend").unwrap();
    triangle(&source.join("main.glb"));
    let library = Library::open(root.path().join("library")).unwrap();
    let ctx = OperationContext::default();
    let asset = library
        .import_folder(&source, "Choices", ExportConfig::default(), &ctx)
        .unwrap();
    let variant = &asset.variants[0];
    let error = match prepare_preview(
        &library,
        PreviewRequest {
            variant_id: &variant.id,
            snapshot_id: None,
            preview_file: Some("alternative.blend"),
            blender_path: None,
        },
        &ctx,
    ) {
        Err(error) => error,
        Ok(_) => panic!("a Blender source requires Blender"),
    };
    assert!(error.to_string().contains("Configurez Blender"), "{error}");
    assert_eq!(
        library.get_variant(&variant.id).unwrap().variant.config,
        variant.config
    );
}

#[test]
fn remembered_preview_choice_is_isolated_per_current_variant_and_snapshot_and_survives_reopen() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    triangle(&source.join("one.glb"));
    triangle(&source.join("two.glb"));
    let library = Library::open(root.path().join("library")).unwrap();
    let ctx = OperationContext::default();
    let asset = library
        .import_folder(&source, "Choices", ExportConfig::default(), &ctx)
        .unwrap();
    let variant = &asset.variants[0];
    let preview = |snapshot_id, preview_file| {
        prepare_preview(
            &library,
            PreviewRequest {
                variant_id: &variant.id,
                snapshot_id,
                preview_file,
                blender_path: None,
            },
            &ctx,
        )
        .unwrap()
    };
    preview(None, Some("two.glb"));
    assert!(matches!(
        preview(Some(&variant.head_snapshot_id), None),
        PreviewOutcome::Choose(_)
    ));
    preview(Some(&variant.head_snapshot_id), Some("one.glb"));
    assert!(
        matches!(preview(Some(&variant.head_snapshot_id), None), PreviewOutcome::Ready(artifact) if artifact.source_file == "one.glb")
    );
    fs::write(
        Path::new(&variant.workspace_path).join("note.txt"),
        b"new version",
    )
    .unwrap();
    let next = library
        .create_snapshot(&variant.id, "Next context", &ctx)
        .unwrap();
    assert!(matches!(
        preview(Some(&next.id), None),
        PreviewOutcome::Choose(_)
    ));
    preview(Some(&next.id), Some("two.glb"));
    assert!(
        matches!(preview(Some(&variant.head_snapshot_id), None), PreviewOutcome::Ready(artifact) if artifact.source_file == "one.glb")
    );
    assert!(
        matches!(preview(None,None),PreviewOutcome::Ready(artifact) if artifact.source_file == "two.glb")
    );
    library
        .update_config(
            &variant.id,
            ExportConfig {
                apply_modifiers: !variant.config.apply_modifiers,
                ..variant.config.clone()
            },
        )
        .unwrap();
    assert!(matches!(preview(None, None), PreviewOutcome::Choose(_)));
    assert!(matches!(
        preview(Some(&variant.head_snapshot_id), None),
        PreviewOutcome::Ready(artifact) if artifact.source_file == "one.glb"
    ));
    drop(library);
    let reopened = Library::open(root.path().join("library")).unwrap();
    assert!(
        matches!(prepare_preview(&reopened, PreviewRequest { variant_id: &variant.id, snapshot_id: Some(&variant.head_snapshot_id), preview_file: None, blender_path: None }, &ctx).unwrap(), PreviewOutcome::Ready(artifact) if artifact.source_file == "one.glb")
    );
    assert!(
        matches!(prepare_preview(&reopened, PreviewRequest { variant_id: &variant.id, snapshot_id: Some(&next.id), preview_file: None, blender_path: None }, &ctx).unwrap(), PreviewOutcome::Ready(artifact) if artifact.source_file == "two.glb")
    );
}

fn blender_executable() -> PathBuf {
    std::env::var_os("EDGE_LOCK_TEST_BLENDER")
        .map(PathBuf::from)
        .expect("Set EDGE_LOCK_TEST_BLENDER to an installed Blender executable")
}

fn blender_test_directory() -> tempfile::TempDir {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/preview-tests");
    fs::create_dir_all(&parent).unwrap();
    tempfile::tempdir_in(parent).unwrap()
}

/// Both projects and texture variants are generated in this test's disposable root.
fn blender_fixture(root: &Path, external: bool) -> PathBuf {
    let workspace = root.join("source échantillon");
    fs::create_dir_all(workspace.join("textures")).unwrap();
    let request = root.join("fixture.json");
    fs::write(
        &request,
        serde_json::to_vec(&serde_json::json!({
            "workspace": workspace, "external": external,
            "outside":root.join("external.png"),"updated":root.join("updated.png")
        }))
        .unwrap(),
    )
    .unwrap();
    let script = root.join("fixture.py");
    fs::write(&script, r#"
import bpy, json, sys
from pathlib import Path
r=json.loads(Path(sys.argv[-1]).read_text(encoding='utf-8'))
root=Path(r['workspace'])
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.mesh.primitive_cube_add()
cube=bpy.context.object
image=bpy.data.images.new('Albedo',width=2,height=2)
image.generated_color=(1,0,0,1)
texture=Path(r['outside']) if r['external'] else root/'textures'/'albedo.png'
image.filepath_raw=str(texture); image.file_format='PNG'; image.save()
image.filepath=str(texture) if r['external'] else '//textures/albedo.png'
updated=bpy.data.images.new('Updated',width=2,height=2)
updated.generated_color=(0,0,1,1)
updated.filepath_raw=r['updated']; updated.file_format='PNG'; updated.save()
bpy.data.images.remove(updated)
material=bpy.data.materials.new('Textured'); material.use_nodes=True
node=material.node_tree.nodes.new('ShaderNodeTexImage'); node.image=image
material.node_tree.links.new(node.outputs['Color'],material.node_tree.nodes['Principled BSDF'].inputs['Base Color'])
cube.data.materials.append(material)
bpy.context.preferences.filepaths.save_version=0
bpy.context.preferences.filepaths.file_preview_type='NONE'
bpy.ops.wm.save_as_mainfile(filepath=str(root/'main.blend'),relative_remap=False)
"#).unwrap();
    let mut command = Command::new(blender_executable());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    for name in ["config", "scripts", "extensions"] {
        fs::create_dir_all(root.join(name)).unwrap();
    }
    let output = command
        .current_dir(root)
        .env("BLENDER_USER_CONFIG", root.join("config"))
        .env("BLENDER_USER_SCRIPTS", root.join("scripts"))
        .env("BLENDER_USER_EXTENSIONS", root.join("extensions"))
        .args([
            "--background",
            "--factory-startup",
            "--disable-autoexec",
            "--offline-mode",
            "--python-exit-code",
            "1",
            "--python",
        ])
        .arg(script)
        .arg("--")
        .arg(request)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "fixture failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    workspace
}

fn tree_hashes(root: &Path) -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                stack.push(path);
            } else {
                files.insert(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    blake3::hash(&fs::read(path).unwrap()).to_hex().to_string(),
                );
            }
        }
    }
    files
}

fn generated_preview(
    library: &Library,
    variant_id: &str,
    snapshot_id: Option<&str>,
    ctx: &OperationContext,
) -> edge_lock_core::preview::PreviewArtifact {
    let executable = blender_executable();
    let outcome = prepare_preview(
        library,
        PreviewRequest {
            variant_id,
            snapshot_id,
            preview_file: None,
            blender_path: Some(&executable),
        },
        ctx,
    )
    .unwrap();
    let PreviewOutcome::Ready(artifact) = outcome else {
        panic!("expected a generated preview")
    };
    assert_eq!(
        artifact.source_kind,
        edge_lock_core::preview::PreviewKind::GeneratedGlb
    );
    *artifact
}

#[test]
#[ignore = "requires explicitly configured Blender; uses disposable fixtures only"]
fn generated_cache_tracks_external_textures_repairs_corruption_and_preserves_sources() {
    let root = blender_test_directory();
    let source = blender_fixture(root.path(), true);
    let library = Library::open(root.path().join("library")).unwrap();
    let phases = Arc::new(Mutex::new(Vec::new()));
    let progress = phases.clone();
    let ctx = OperationContext {
        progress: Some(Arc::new(move |event| {
            progress.lock().unwrap().push(event.phase)
        })),
        ..Default::default()
    };
    let asset = library
        .import_folder(&source, "External texture", ExportConfig::default(), &ctx)
        .unwrap();
    let variant = &asset.variants[0];
    library.set_geometry(&variant.id, 123).unwrap();
    let metadata = serde_json::to_value(library.get_asset(&asset.asset.id).unwrap()).unwrap();
    let snapshot =
        serde_json::to_value(library.get_snapshot(&variant.head_snapshot_id).unwrap()).unwrap();
    let source_before = tree_hashes(&source);
    let objects_before = tree_hashes(&library.root().join("objects"));
    let workspace = Path::new(&variant.workspace_path);
    let workspace_before = tree_hashes(workspace);
    let first = generated_preview(&library, &variant.id, None, &ctx);
    assert_eq!(first.inspection.geometry.edges, Some(12));
    assert!(
        first
            .inspection
            .dependencies
            .iter()
            .any(|dependency| dependency.render_relevant && dependency.status == "external"),
        "{:?}",
        first.inspection.dependencies
    );
    assert_eq!(tree_hashes(workspace), workspace_before);
    let modified = fs::metadata(&first.path).unwrap().modified().unwrap();
    fs::write(
        workspace.join("report.txt"),
        b"This report does not affect rendering",
    )
    .unwrap();
    let workspace_with_report = tree_hashes(workspace);
    phases.lock().unwrap().clear();
    let second = generated_preview(&library, &variant.id, None, &ctx);
    assert_eq!(first.path, second.path);
    assert_eq!(
        fs::metadata(&second.path).unwrap().modified().unwrap(),
        modified
    );
    assert!(
        phases
            .lock()
            .unwrap()
            .iter()
            .any(|phase| phase == "Aperçu en cache")
    );
    fs::copy(
        root.path().join("updated.png"),
        root.path().join("external.png"),
    )
    .unwrap();
    let changed = generated_preview(&library, &variant.id, None, &ctx);
    assert_ne!(changed.path, second.path);
    let expected = fs::read(&changed.path).unwrap();
    assert_ne!(expected, fs::read(&second.path).unwrap());
    fs::write(&changed.path, b"interrupted/corrupted cache file").unwrap();
    let repaired = generated_preview(&library, &variant.id, None, &ctx);
    assert_eq!(repaired.path, changed.path);
    assert_eq!(fs::read(&repaired.path).unwrap(), expected);
    assert_eq!(tree_hashes(workspace), workspace_with_report);
    assert_eq!(tree_hashes(&source), source_before);
    assert_eq!(tree_hashes(&library.root().join("objects")), objects_before);
    assert_eq!(
        serde_json::to_value(library.get_asset(&asset.asset.id).unwrap()).unwrap(),
        metadata
    );
    assert_eq!(
        serde_json::to_value(library.get_snapshot(&variant.head_snapshot_id).unwrap()).unwrap(),
        snapshot
    );
    assert!(
        !fs::read_dir(library.root().join("cache/previews"))
            .unwrap()
            .any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".preview-tmp-"))
    );

    let blend = workspace.join("main.blend");
    let saved = fs::read(&blend).unwrap();
    fs::write(&blend, b"unfinished Blender save").unwrap();
    let previous = generated_preview(&library, &variant.id, None, &ctx);
    assert!(previous.stale);
    assert!(!previous.updates_geometry);
    assert_eq!(previous.artifact_key, repaired.artifact_key);
    assert_eq!(fs::read(&previous.path).unwrap(), expected);

    let changed_config = ExportConfig {
        scene: Some("Other scene".into()),
        ..variant.config.clone()
    };
    library.update_config(&variant.id, changed_config).unwrap();
    assert!(
        prepare_preview(
            &library,
            PreviewRequest {
                variant_id: &variant.id,
                snapshot_id: None,
                preview_file: None,
                blender_path: Some(&blender_executable()),
            },
            &ctx,
        )
        .is_err(),
        "the previous preview belongs to a different scene configuration"
    );

    fs::write(&blend, saved).unwrap();
    library
        .update_config(&variant.id, variant.config.clone())
        .unwrap();
    let fresh = generated_preview(&library, &variant.id, None, &ctx);
    assert!(!fresh.stale);
    assert_eq!(fresh.artifact_key, repaired.artifact_key);
}

#[test]
#[ignore = "requires explicitly configured Blender; uses disposable fixtures only"]
fn historical_generated_preview_uses_immutable_internal_files_and_keeps_metadata() {
    let root = blender_test_directory();
    let source = blender_fixture(root.path(), false);
    let library = Library::open(root.path().join("library")).unwrap();
    let ctx = OperationContext::default();
    let asset = library
        .import_folder(&source, "Historical texture", ExportConfig::default(), &ctx)
        .unwrap();
    let variant = &asset.variants[0];
    let snapshot_id = &variant.head_snapshot_id;
    let saved_snapshot = serde_json::to_value(library.get_snapshot(snapshot_id).unwrap()).unwrap();
    let objects_before = tree_hashes(&library.root().join("objects"));
    let source_before = tree_hashes(&source);
    let workspace = Path::new(&variant.workspace_path);
    fs::write(
        workspace.join("main.blend"),
        b"current work intentionally differs from history",
    )
    .unwrap();
    fs::copy(
        root.path().join("updated.png"),
        workspace.join("textures/albedo.png"),
    )
    .unwrap();
    library
        .update_config(
            &variant.id,
            ExportConfig {
                scene: Some("Current scene absent from saved version".into()),
                ..variant.config.clone()
            },
        )
        .unwrap();
    library.set_geometry(&variant.id, 456).unwrap();
    let metadata = serde_json::to_value(library.get_asset(&asset.asset.id).unwrap()).unwrap();
    let workspace_before = tree_hashes(workspace);
    let first = generated_preview(&library, &variant.id, Some(snapshot_id), &ctx);
    assert_eq!(first.inspection.geometry.vertices, 8);
    assert_eq!(first.inspection.geometry.edges, Some(12));
    assert!(
        !first
            .inspection
            .dependencies
            .iter()
            .any(|dependency| dependency.render_relevant && dependency.status == "missing")
    );
    let modified = fs::metadata(&first.path).unwrap().modified().unwrap();
    fs::write(
        workspace.join("textures/albedo.png"),
        b"the live texture is now invalid",
    )
    .unwrap();
    let workspace_after_manual_edit = tree_hashes(workspace);
    let second = generated_preview(&library, &variant.id, Some(snapshot_id), &ctx);
    assert_eq!(first.path, second.path);
    assert_eq!(
        fs::metadata(&second.path).unwrap().modified().unwrap(),
        modified
    );
    assert_eq!(
        fs::read(&second.path).unwrap(),
        fs::read(&first.path).unwrap()
    );
    let detached = root.path().join("detached-workspace");
    fs::rename(workspace, &detached).unwrap();
    let historical = generated_preview(&library, &variant.id, Some(snapshot_id), &ctx);
    assert!(!historical.stale);
    assert_eq!(historical.artifact_key, first.artifact_key);
    fs::rename(&detached, workspace).unwrap();
    assert_eq!(tree_hashes(workspace), workspace_after_manual_edit);
    assert_eq!(tree_hashes(&source), source_before);
    assert_eq!(tree_hashes(&library.root().join("objects")), objects_before);
    assert_eq!(
        serde_json::to_value(library.get_asset(&asset.asset.id).unwrap()).unwrap(),
        metadata
    );
    assert_eq!(
        serde_json::to_value(library.get_snapshot(snapshot_id).unwrap()).unwrap(),
        saved_snapshot
    );
    // The only workspace differences are the deliberate texture edit above.
    assert_eq!(workspace_before.len(), workspace_after_manual_edit.len());
}

#[test]
#[ignore = "requires explicitly configured Blender; uses disposable fixtures only"]
fn deleted_configured_glb_falls_back_to_unique_blend_without_changing_config() {
    let root = blender_test_directory();
    let source = blender_fixture(root.path(), false);
    triangle(&source.join("original.glb"));
    let library = Library::open(root.path().join("library")).unwrap();
    let ctx = OperationContext::default();
    let asset = library
        .import_folder(
            &source,
            "Removed GLB",
            ExportConfig {
                primary_file: Some("original.glb".into()),
                ..Default::default()
            },
            &ctx,
        )
        .unwrap();
    let variant = &asset.variants[0];
    fs::remove_file(Path::new(&variant.workspace_path).join("original.glb")).unwrap();
    let before = tree_hashes(Path::new(&variant.workspace_path));
    let metadata = serde_json::to_value(library.get_asset(&asset.asset.id).unwrap()).unwrap();
    let artifact = generated_preview(&library, &variant.id, None, &ctx);
    assert_eq!(artifact.source_file, "main.blend");
    assert_eq!(artifact.inspection.geometry.edges, Some(12));
    assert_eq!(tree_hashes(Path::new(&variant.workspace_path)), before);
    assert_eq!(
        serde_json::to_value(library.get_asset(&asset.asset.id).unwrap()).unwrap(),
        metadata
    );
    assert_eq!(
        library
            .get_variant(&variant.id)
            .unwrap()
            .variant
            .config
            .primary_file
            .as_deref(),
        Some("original.glb")
    );
}

#[test]
fn cancelled_staging_removes_temporary_files_without_touching_sources() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    triangle(&source.join("original.glb"));
    let library = Library::open(root.path().join("library")).unwrap();
    let asset = library
        .import_folder(
            &source,
            "Cancelled preview",
            ExportConfig::default(),
            &OperationContext::default(),
        )
        .unwrap();
    let variant = &asset.variants[0];
    let source_before = tree_hashes(&source);
    let workspace_before = tree_hashes(Path::new(&variant.workspace_path));
    let objects_before = tree_hashes(&library.root().join("objects"));
    let metadata = serde_json::to_value(library.get_asset(&asset.asset.id).unwrap()).unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancel_on_staging = cancelled.clone();
    let ctx = OperationContext {
        cancelled,
        progress: Some(Arc::new(move |event| {
            if event.phase == "Préparation de l’aperçu" {
                cancel_on_staging.store(true, Ordering::Relaxed);
            }
        })),
    };
    let result = prepare_preview(
        &library,
        PreviewRequest {
            variant_id: &variant.id,
            snapshot_id: None,
            preview_file: None,
            blender_path: None,
        },
        &ctx,
    );
    assert!(matches!(result, Err(edge_lock_core::CoreError::Cancelled)));
    assert_eq!(tree_hashes(&source), source_before);
    assert_eq!(
        tree_hashes(Path::new(&variant.workspace_path)),
        workspace_before
    );
    assert_eq!(tree_hashes(&library.root().join("objects")), objects_before);
    assert_eq!(
        serde_json::to_value(library.get_asset(&asset.asset.id).unwrap()).unwrap(),
        metadata
    );
    assert_eq!(
        fs::read_dir(library.root().join("cache/previews"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
#[ignore = "requires explicitly configured Blender; uses disposable fixtures only"]
fn source_change_during_conversion_retries_before_publishing_stable_cache() {
    let root = blender_test_directory();
    let source = blender_fixture(root.path(), true);
    let library = Library::open(root.path().join("library")).unwrap();
    let asset = library
        .import_folder(
            &source,
            "Changing dependency",
            ExportConfig::default(),
            &OperationContext::default(),
        )
        .unwrap();
    let variant = &asset.variants[0];
    let source_before = tree_hashes(&source);
    let workspace_before = tree_hashes(Path::new(&variant.workspace_path));
    let objects_before = tree_hashes(&library.root().join("objects"));
    let metadata = serde_json::to_value(library.get_asset(&asset.asset.id).unwrap()).unwrap();
    let snapshot =
        serde_json::to_value(library.get_snapshot(&variant.head_snapshot_id).unwrap()).unwrap();
    let starts = Arc::new(AtomicUsize::new(0));
    let observed_starts = starts.clone();
    let texture = root.path().join("external.png");
    let replacement = root.path().join("updated.png");
    let ctx = OperationContext {
        progress: Some(Arc::new(move |event| {
            if event.phase == "blender"
                && event.completed == 0
                && observed_starts.fetch_add(1, Ordering::Relaxed) == 1
            {
                fs::copy(&replacement, &texture).unwrap();
            }
        })),
        ..Default::default()
    };
    let artifact = generated_preview(&library, &variant.id, None, &ctx);
    assert!(
        starts.load(Ordering::Relaxed) >= 4,
        "expected discovery/export for both the interrupted and stable attempts"
    );
    edge_lock_core::glb::guard_preview(&artifact.path).unwrap();
    assert_eq!(artifact.inspection.geometry.edges, Some(12));
    assert_eq!(tree_hashes(&source), source_before);
    assert_eq!(
        tree_hashes(Path::new(&variant.workspace_path)),
        workspace_before
    );
    assert_eq!(tree_hashes(&library.root().join("objects")), objects_before);
    assert_eq!(
        serde_json::to_value(library.get_asset(&asset.asset.id).unwrap()).unwrap(),
        metadata
    );
    assert_eq!(
        serde_json::to_value(library.get_snapshot(&variant.head_snapshot_id).unwrap()).unwrap(),
        snapshot
    );
    let entries: Vec<_> = fs::read_dir(library.root().join("cache/previews"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(
        entries
            .iter()
            .filter(|path| path.extension().is_some_and(|ext| ext == "glb"))
            .count(),
        1
    );
    assert!(!entries.iter().any(|path| {
        path.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(".preview-tmp-")
    }));
    let stable = generated_preview(&library, &variant.id, None, &ctx);
    assert_eq!(stable.path, artifact.path);
    assert_eq!(
        starts.load(Ordering::Relaxed),
        4,
        "stable cache must avoid further Blender launches"
    );
}
