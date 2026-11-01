//! Real Blender contract tests; only generated disposable assets are opened.
//! EDGE_LOCK_TEST_BLENDER=<executable> cargo test -p edge-lock-core --test blender_integration -- --ignored
use edge_lock_core::{
    CoreError, ExportConfig, OperationContext,
    blender::BlenderRunner,
    glb::{guard_preview, inspect_glb},
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::Ordering,
    time::Duration,
};

fn executable() -> PathBuf {
    std::env::var_os("EDGE_LOCK_TEST_BLENDER")
        .map(PathBuf::from)
        .expect("Set EDGE_LOCK_TEST_BLENDER to an installed Blender executable")
}

fn test_directory() -> tempfile::TempDir {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/blender-tests");
    fs::create_dir_all(&directory).unwrap();
    tempfile::tempdir_in(directory).unwrap()
}

#[test]
fn rejects_ambiguous_or_escaping_primary_file_before_launch() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("first.blend"), b"fixture").unwrap();
    fs::write(directory.path().join("second.blend"), b"fixture").unwrap();
    let runner = BlenderRunner::new(PathBuf::from("nonexistent-blender-executable"));
    let context = OperationContext::default();
    assert!(matches!(
        runner.inspect(directory.path(), &ExportConfig::default(), &context),
        Err(CoreError::Invalid(_))
    ));
    let config = ExportConfig {
        primary_file: Some("../outside.blend".into()),
        ..Default::default()
    };
    assert!(matches!(
        runner.inspect(directory.path(), &config, &context),
        Err(CoreError::Invalid(_))
    ));
    context.cancelled.store(true, Ordering::Relaxed);
    assert!(matches!(
        runner.inspect(directory.path(), &config, &context),
        Err(CoreError::Cancelled)
    ));
}

fn fixture(root: &Path, external_texture: bool, embedded_script: bool) -> PathBuf {
    let workspace = root.join("asset échantillon");
    fs::create_dir_all(workspace.join("textures")).unwrap();
    let request = root.join("fixture.json");
    fs::write(
        &request,
        serde_json::to_vec(&serde_json::json!({
            "workspace":workspace,"external":external_texture,"embeddedScript":embedded_script,
            "outside":root.join("external.png"),"marker":root.join("executed.txt")
        }))
        .unwrap(),
    )
    .unwrap();
    let script = root.join("fixture.py");
    fs::write(&script,r#"
import bpy, json, sys
from pathlib import Path
r=json.loads(Path(sys.argv[-1]).read_text(encoding='utf-8'))
root=Path(r['workspace'])
bpy.ops.wm.read_factory_settings(use_empty=True)
scene=bpy.context.scene
asset=bpy.data.collections.new('Asset'); scene.collection.children.link(asset)
nested=bpy.data.collections.new('Nested'); asset.children.link(nested)
other=bpy.data.collections.new('Other'); scene.collection.children.link(other)
bpy.ops.mesh.primitive_cube_add()
cube=bpy.context.object; cube.name='TexturedCube'
for col in list(cube.users_collection): col.objects.unlink(cube)
nested.objects.link(cube)
cube.shape_key_add(name='Basis')
key=cube.shape_key_add(name='Smile'); key.data[0].co.z+=0.3
key.value=0; key.keyframe_insert('value',frame=1)
key.value=1; key.keyframe_insert('value',frame=10)
image=bpy.data.images.new('Albedo',width=2,height=2)
image.generated_color=(0.2,0.6,0.9,1)
texture=Path(r['outside']) if r['external'] else root/'textures'/'albedo.png'
image.filepath_raw=str(texture); image.file_format='PNG'; image.save()
image.filepath=str(texture) if r['external'] else '//textures/albedo.png'
material=bpy.data.materials.new('PBR'); material.use_nodes=True
node=material.node_tree.nodes.new('ShaderNodeTexImage'); node.image=image
material.node_tree.links.new(node.outputs['Color'], material.node_tree.nodes['Principled BSDF'].inputs['Base Color'])
cube.data.materials.append(material)
data=bpy.data.armatures.new('Skeleton'); rig=bpy.data.objects.new('Rig',data); asset.objects.link(rig)
bpy.context.view_layer.objects.active=rig; rig.select_set(True); cube.select_set(False)
bpy.ops.object.mode_set(mode='EDIT')
bone=data.edit_bones.new('Root'); bone.head=(0,0,0); bone.tail=(0,0,1)
bpy.ops.object.mode_set(mode='OBJECT')
mod=cube.modifiers.new('Rig','ARMATURE'); mod.object=rig; cube.parent=rig
group=cube.vertex_groups.new(name='Root'); group.add(list(range(len(cube.data.vertices))),1.0,'REPLACE')
bone=rig.pose.bones['Root']; bone.rotation_mode='XYZ'
bone.rotation_euler=(0,0,0); bone.keyframe_insert('rotation_euler',frame=1)
bone.rotation_euler=(0,0,0.5); bone.keyframe_insert('rotation_euler',frame=10)
bpy.ops.mesh.primitive_cube_add(location=(5,0,0))
extra=bpy.context.object
for col in list(extra.users_collection): col.objects.unlink(extra)
other.objects.link(extra)
if r['embeddedScript']:
    text=bpy.data.texts.new('untrusted.py')
    text.write('from pathlib import Path\nPath('+repr(r['marker'])+').write_text("executed")')
    text.use_module=True
scene.frame_set(1)
bpy.context.preferences.filepaths.save_version=0
bpy.context.preferences.filepaths.file_preview_type='NONE'
bpy.ops.wm.save_as_mainfile(filepath=str(root/'main.blend'))
"#).unwrap();
    let mut command = Command::new(executable());
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

#[test]
#[ignore = "requires explicitly configured Blender; uses disposable fixture only"]
fn exports_nested_collection_preserving_rig_shapes_animation_and_source() {
    let root = test_directory();
    let workspace = fixture(root.path(), false, false);
    let runner = BlenderRunner::new(executable());
    assert!(runner.probe().unwrap().version.starts_with('5'));
    let context = OperationContext::default();
    let config = ExportConfig {
        collection: Some("Asset".into()),
        ..Default::default()
    };
    let original = fs::read(workspace.join("main.blend")).unwrap();
    let inspection = runner.inspect(&workspace, &config, &context).unwrap();
    assert_eq!(inspection.geometry.mesh_objects, 1);
    assert_eq!(inspection.geometry.vertices, 8);
    assert_eq!(inspection.geometry.faces, 6);
    assert_eq!(inspection.geometry.triangles, 12);
    assert!(inspection.collections.contains(&"Asset".into()));
    assert!(
        inspection
            .dependencies
            .iter()
            .all(|d| d.status == "internal" || d.status == "packed"),
        "{:?}",
        inspection.dependencies
    );
    assert!(
        inspection
            .dependencies
            .iter()
            .filter(|item| item.status == "internal")
            .all(|item| !Path::new(&item.path).is_absolute())
    );
    let output = root.path().join("result.glb");
    runner
        .export_glb(&workspace, &config, &output, &context)
        .unwrap();
    let stats = inspect_glb(&output).unwrap();
    assert_eq!(stats.geometry.mesh_objects, 1);
    assert_eq!(stats.geometry.triangles, 12);
    assert!(!stats.animations.is_empty());
    assert!(guard_preview(&output).is_ok());
    let data = fs::read(&output).unwrap();
    let json_len = u32::from_le_bytes(data[12..16].try_into().unwrap()) as usize;
    let document: serde_json::Value = serde_json::from_slice(&data[20..20 + json_len]).unwrap();
    assert!(!document["skins"].as_array().unwrap().is_empty());
    assert!(
        !document["meshes"][0]["primitives"][0]["targets"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(document["images"][0]["bufferView"].is_u64());
    assert_eq!(fs::read(workspace.join("main.blend")).unwrap(), original);
    let modifiers = ExportConfig {
        apply_modifiers: true,
        ..config
    };
    assert!(
        runner
            .export_glb(
                &workspace,
                &modifiers,
                &root.path().join("bad.glb"),
                &context
            )
            .is_err()
    );
    assert!(!root.path().join("bad.glb").exists());
}

#[test]
#[ignore = "requires explicitly configured Blender; uses disposable fixture only"]
fn reports_external_texture_and_never_publishes_historical_export() {
    let root = test_directory();
    let workspace = fixture(root.path(), true, false);
    let runner = BlenderRunner::new(executable());
    let config = ExportConfig::default();
    let context = OperationContext::default();
    let inspection = runner.inspect(&workspace, &config, &context).unwrap();
    assert!(
        inspection
            .dependencies
            .iter()
            .any(|d| d.status == "external")
    );
    let output = root.path().join("forbidden.glb");
    assert!(
        runner
            .export_glb(&workspace, &config, &output, &context)
            .is_err()
    );
    assert!(!output.exists());
}

#[test]
#[ignore = "requires explicitly configured Blender; uses disposable fixture only"]
fn isolates_embedded_scripts_and_handles_timeout_and_cancellation() {
    let root = test_directory();
    let workspace = fixture(root.path(), false, true);
    let runner = BlenderRunner::new(executable());
    let config = ExportConfig::default();
    let context = OperationContext::default();
    runner.inspect(&workspace, &config, &context).unwrap();
    assert!(!root.path().join("executed.txt").exists());
    assert!(
        runner
            .clone()
            .with_timeout(Duration::from_millis(1))
            .inspect(&workspace, &config, &context)
            .is_err()
    );
    context.cancelled.store(true, Ordering::Relaxed);
    assert!(matches!(
        runner.inspect(&workspace, &config, &context),
        Err(CoreError::Cancelled)
    ));
}

#[test]
#[ignore = "requires explicitly configured Blender; uses disposable fixture only"]
fn creates_real_empty_project_and_refuses_overwrite() {
    let root = test_directory();
    let workspace = root.path().join("new project");
    let runner = BlenderRunner::new(executable());
    let context = OperationContext::default();
    runner.create_project(&workspace, &context).unwrap();
    assert!(workspace.join("main.blend").is_file());
    assert_eq!(
        runner
            .inspect(&workspace, &ExportConfig::default(), &context)
            .unwrap()
            .geometry
            .mesh_objects,
        0
    );
    assert!(runner.create_project(&workspace, &context).is_err());
}
