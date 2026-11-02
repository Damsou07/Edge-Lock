//! End-to-end canonical project contract. Every Blender file is a generated disposable fixture.
use edge_lock_core::{Library, OperationContext, blender::BlenderRunner, glb, sync};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn executable() -> PathBuf {
    std::env::var_os("EDGE_LOCK_TEST_BLENDER")
        .map(PathBuf::from)
        .expect("Set EDGE_LOCK_TEST_BLENDER to an installed Blender executable")
}

fn run_fixture(root: &Path, blend: &Path, operation: &str) {
    let script = root.join("fixture.py");
    fs::write(&script, r#"
import bpy, sys
path, operation = sys.argv[sys.argv.index('--') + 1:]
if operation == 'create':
    bpy.ops.object.select_all(action='SELECT')
    bpy.ops.object.delete(use_global=False)
    bpy.ops.mesh.primitive_cube_add()
    obj = bpy.context.object
    for vertex in obj.data.vertices:
        vertex.co.x += 2
        vertex.co.y += 2
    mirror = obj.modifiers.new('Mirror', 'MIRROR')
    import os
    texture = os.path.join(os.path.dirname(path), 'textures', 'albedo.png')
    os.makedirs(os.path.dirname(texture), exist_ok=True)
    image = bpy.data.images.new('Albedo', width=2, height=2)
    image.filepath_raw = texture
    image.file_format = 'PNG'
    image.save()
    material = bpy.data.materials.new('Textured')
    material.use_nodes = True
    node = material.node_tree.nodes.new('ShaderNodeTexImage')
    node.image = image
    material.node_tree.links.new(node.outputs['Color'], material.node_tree.nodes.get('Principled BSDF').inputs['Base Color'])
    obj.data.materials.append(material)
else:
    bpy.ops.wm.open_mainfile(filepath=path)
    obj = next(obj for obj in bpy.data.objects if obj.type == 'MESH')
    mirror = obj.modifiers['Mirror']
    mirror.use_axis[1] = True
bpy.ops.wm.save_as_mainfile(filepath=path)
assert obj.modifiers.get('Mirror') is not None
"#).unwrap();
    for name in ["config", "scripts", "extensions"] {
        fs::create_dir_all(root.join(name)).unwrap();
    }
    let mut command = Command::new(executable());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
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
        .arg(blend)
        .arg(operation)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn hash(path: &Path) -> String {
    blake3::hash(&fs::read(path).unwrap()).to_hex().to_string()
}

#[test]
#[ignore = "requires explicitly configured Blender; uses disposable fixtures only"]
fn import_save_sync_snapshot_and_historical_fork_keep_sources_editable_and_glbs_current() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/blender-tests");
    fs::create_dir_all(&directory).unwrap();
    let root = tempfile::tempdir_in(directory).unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    let original = source.join("main.blend");
    run_fixture(root.path(), &original, "create");
    let original_hash = hash(&original);
    let library = Library::open(root.path().join("library")).unwrap();
    let runner = BlenderRunner::new(executable());
    let ctx = OperationContext::default();
    let asset = sync::import_project(&library, &source, "Mirrored", &runner, &ctx).unwrap();
    let variant = &asset.variants[0];
    let initial = library.get_snapshot(&variant.head_snapshot_id).unwrap();
    assert!(initial.entries.iter().any(|entry| entry.path == "main.glb"));
    let workspace = Path::new(&variant.workspace_path);
    let blend = workspace.join("main.blend");
    let glb = workspace.join("main.glb");
    assert_eq!(hash(&blend), original_hash);
    assert_eq!(hash(&original), original_hash);
    assert!(!source.join("main.glb").exists());
    assert_eq!(glb::inspect_glb(&glb).unwrap().geometry.triangles, 24);
    assert_eq!(variant.triangles, Some(24));
    let first = sync::synchronize_variant(&library, &variant.id, None, &ctx).unwrap();
    assert!(!first.source_changed);
    assert!(!library.root().join("backups/glb").exists());

    run_fixture(root.path(), &blend, "edit");
    let edited_hash = hash(&blend);
    assert_ne!(edited_hash, original_hash);
    assert!(
        library
            .create_snapshot_checked(&variant.id, "Stale output", &first.receipt, &ctx)
            .is_err()
    );
    let updated = sync::synchronize_variant(&library, &variant.id, Some(&runner), &ctx).unwrap();
    assert!(updated.source_changed);
    assert_ne!(first.artifact_key, updated.artifact_key);
    assert_eq!(glb::inspect_glb(&glb).unwrap().geometry.triangles, 48);
    assert_eq!(updated.inspection.geometry.triangles, 48);
    assert_eq!(
        glb::inspect_glb(&first.path).unwrap().geometry.triangles,
        24
    );
    assert_eq!(hash(&blend), edited_hash);
    assert!(!library.root().join("backups/glb").exists());
    let version = sync::create_snapshot(
        &library,
        &variant.id,
        "Second mirror axis",
        Some(&runner),
        &ctx,
    )
    .unwrap();
    assert_eq!(
        version
            .entries
            .iter()
            .find(|entry| entry.path == "main.blend")
            .unwrap()
            .hash
            .as_deref(),
        Some(edited_hash.as_str())
    );
    assert!(
        sync::create_snapshot(&library, &variant.id, "Unchanged", Some(&runner), &ctx).is_err()
    );

    fs::write(&glb, b"unversioned GLB data to preserve").unwrap();
    let replaced_hash = hash(&glb);
    let repaired = sync::synchronize_variant(&library, &variant.id, Some(&runner), &ctx).unwrap();
    assert!(!repaired.source_changed);
    assert_eq!(
        fs::read(
            library
                .root()
                .join("backups/glb")
                .join(format!("{replaced_hash}.glb"))
        )
        .unwrap(),
        b"unversioned GLB data to preserve"
    );
    assert!(
        sync::create_snapshot(
            &library,
            &variant.id,
            "Only regenerated GLB",
            Some(&runner),
            &ctx
        )
        .is_err()
    );
    assert_eq!(hash(&blend), edited_hash);

    let current_glb_hash = hash(&glb);
    let texture = workspace.join("textures/albedo.png");
    let texture_bytes = fs::read(&texture).unwrap();
    fs::remove_file(&texture).unwrap();
    assert!(sync::synchronize_variant(&library, &variant.id, Some(&runner), &ctx).is_err());
    assert_eq!(hash(&glb), current_glb_hash);
    assert_eq!(hash(&blend), edited_hash);
    fs::write(&texture, texture_bytes).unwrap();

    let fork = sync::create_variant(&library, &initial.id, "Old source", &runner, &ctx).unwrap();
    let fork_path = Path::new(&fork.workspace_path);
    assert_eq!(hash(&fork_path.join("main.blend")), original_hash);
    assert_eq!(
        glb::inspect_glb(&fork_path.join("main.glb"))
            .unwrap()
            .geometry
            .triangles,
        24
    );
    assert!(
        !sync::synchronize_variant(&library, &fork.id, None, &ctx)
            .unwrap()
            .source_changed
    );
    assert_eq!(
        library.get_snapshot(&initial.id).unwrap().entries,
        initial.entries
    );
    assert_eq!(hash(&original), original_hash);
}
