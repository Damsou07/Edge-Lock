//! Real Blender contract tests; only generated disposable assets are opened.
//! EDGE_LOCK_TEST_BLENDER=<executable> cargo test -p edge-lock-core --test blender_integration -- --ignored
use edge_lock_core::{
    CoreError, ExportConfig, Library, OperationContext,
    blender::BlenderRunner,
    glb::{guard_preview, inspect_glb},
    sync,
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
    assert!(matches!(
        runner.generate_glb(
            directory.path(),
            directory.path(),
            &ExportConfig {
                primary_file: Some("first.blend".into()),
                ..Default::default()
            },
            &directory.path().join("canonical.glb"),
            &context,
        ),
        Err(CoreError::External(_))
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

#[test]
fn preview_logical_workspace_can_be_missing_but_must_be_absolute() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("main.blend"), b"fixture").unwrap();
    let runner = BlenderRunner::new(PathBuf::from("nonexistent-blender-executable"));
    let config = ExportConfig::default();
    let context = OperationContext::default();
    assert!(matches!(
        runner.inspect_preview(directory.path(), Path::new("relative"), &config, &context),
        Err(CoreError::Invalid(_))
    ));
    assert!(matches!(
        runner.generate_glb(
            directory.path(),
            Path::new("relative"),
            &config,
            &directory.path().join("canonical.glb"),
            &context,
        ),
        Err(CoreError::Invalid(_))
    ));
    assert!(matches!(
        runner.inspect_preview(
            directory.path(),
            &directory.path().join("missing-original"),
            &config,
            &context
        ),
        Err(CoreError::External(_))
    ));
}

fn fixture(root: &Path, external_texture: bool, embedded_script: bool) -> PathBuf {
    fixture_with_shapes(root, external_texture, embedded_script, false)
}

fn fixture_with_shapes(
    root: &Path,
    external_texture: bool,
    embedded_script: bool,
    shape_keys: bool,
) -> PathBuf {
    let workspace = root.join("asset échantillon");
    fs::create_dir_all(workspace.join("textures")).unwrap();
    let request = root.join("fixture.json");
    fs::write(
        &request,
        serde_json::to_vec(&serde_json::json!({
            "workspace":workspace,"external":external_texture,"embeddedScript":embedded_script,
            "shapeKeys":shape_keys,
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
if r['shapeKeys']:
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

fn edit_fixture(root: &Path, workspace: &Path, script: &str) {
    let script_path = root.join("edit-fixture.py");
    fs::write(&script_path, script).unwrap();
    let mut command = Command::new(executable());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command
        .current_dir(root)
        .args([
            "--background",
            "--factory-startup",
            "--disable-autoexec",
            "--offline-mode",
            "--python-exit-code",
            "1",
            "--python",
        ])
        .arg(script_path)
        .arg("--")
        .arg(workspace)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "fixture edit failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn glb_document(path: &Path) -> (serde_json::Value, Vec<u8>) {
    let bytes = fs::read(path).unwrap();
    let json_length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let document = serde_json::from_slice(&bytes[20..20 + json_length]).unwrap();
    (document, bytes[28 + json_length..].to_vec())
}

fn float_accessor(document: &serde_json::Value, binary: &[u8], index: usize) -> Vec<f32> {
    let accessor = &document["accessors"][index];
    assert_eq!(accessor["componentType"], 5126);
    assert_eq!(accessor["type"], "SCALAR");
    let view = &document["bufferViews"][accessor["bufferView"].as_u64().unwrap() as usize];
    let offset = view["byteOffset"].as_u64().unwrap_or(0) as usize
        + accessor["byteOffset"].as_u64().unwrap_or(0) as usize;
    let stride = view["byteStride"].as_u64().unwrap_or(4) as usize;
    (0..accessor["count"].as_u64().unwrap() as usize)
        .map(|i| {
            f32::from_le_bytes(
                binary[offset + i * stride..offset + i * stride + 4]
                    .try_into()
                    .unwrap(),
            )
        })
        .collect()
}

fn assert_animated_skin_and_morphs(path: &Path, node_name: &str) {
    let (document, binary) = glb_document(path);
    let (node_index, node) = document["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .find(|(_, node)| node["name"] == node_name)
        .unwrap();
    let skin = node["skin"]
        .as_u64()
        .expect("morph mesh must retain its skin") as usize;
    assert!(
        !document["skins"][skin]["joints"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let mesh = &document["meshes"][node["mesh"].as_u64().unwrap() as usize];
    assert_eq!(
        mesh["extras"]["targetNames"],
        serde_json::json!(["Smile", "Driven"])
    );
    for primitive in mesh["primitives"].as_array().unwrap() {
        assert!(primitive["attributes"]["JOINTS_0"].is_u64());
        assert!(primitive["attributes"]["WEIGHTS_0"].is_u64());
        assert!(primitive["attributes"]["TEXCOORD_0"].is_u64());
        assert_eq!(primitive["targets"].as_array().unwrap().len(), 2);
    }
    let mut driven_changes = false;
    let mut has_bone_animation = false;
    for animation in document["animations"].as_array().unwrap() {
        for channel in animation["channels"].as_array().unwrap() {
            has_bone_animation |= channel["target"]["path"] == "rotation";
            if channel["target"]["node"] != node_index || channel["target"]["path"] != "weights" {
                continue;
            }
            let sampler = &animation["samplers"][channel["sampler"].as_u64().unwrap() as usize];
            let weights = float_accessor(
                &document,
                &binary,
                sampler["output"].as_u64().unwrap() as usize,
            );
            let driven: Vec<_> = weights
                .as_chunks::<2>()
                .0
                .iter()
                .map(|sample| sample[1])
                .collect();
            driven_changes |= driven
                .windows(2)
                .any(|pair| (pair[0] - pair[1]).abs() > 0.001);
        }
    }
    assert!(has_bone_animation, "skeleton animation must survive export");
    assert!(
        driven_changes,
        "bone-driven morph weights must be sampled, not frozen"
    );
}

#[test]
#[ignore = "requires explicitly configured Blender; uses disposable fixture only"]
fn previews_external_and_missing_textures_without_modifying_sources() {
    let root = test_directory();
    let workspace = fixture(root.path(), true, false);
    let original = fs::read(workspace.join("main.blend")).unwrap();
    let runner = BlenderRunner::new(executable());
    let context = OperationContext::default();
    let config = ExportConfig::default();
    let inspection = runner
        .inspect_preview(&workspace, &workspace, &config, &context)
        .unwrap();
    assert!(inspection.dependencies.iter().any(|d| {
        d.kind == "image"
            && d.render_relevant
            && d.source_path
                .as_deref()
                .is_some_and(|p| p.ends_with("external.png"))
    }));
    let output = root.path().join("external-preview.glb");
    runner
        .preview_glb(&workspace, &workspace, &config, &output, &context)
        .unwrap();
    assert!(guard_preview(&output).is_ok());
    let canonical = root.path().join("external-canonical.glb");
    runner
        .generate_glb(&workspace, &workspace, &config, &canonical, &context)
        .unwrap();
    assert!(guard_preview(&canonical).is_ok());
    let canonical_before = fs::read(&canonical).unwrap();
    fs::remove_file(root.path().join("external.png")).unwrap();
    let rejected = root.path().join("missing-canonical.glb");
    let error = runner
        .generate_glb(&workspace, &workspace, &config, &rejected, &context)
        .unwrap_err();
    assert!(error.to_string().contains("manquantes"));
    assert!(!rejected.exists());
    assert_eq!(fs::read(&canonical).unwrap(), canonical_before);
    let output = root.path().join("missing-preview.glb");
    let inspection = runner
        .preview_glb(&workspace, &workspace, &config, &output, &context)
        .unwrap();
    assert!(
        inspection
            .dependencies
            .iter()
            .any(|d| d.kind == "image" && d.render_relevant && d.status == "missing")
    );
    assert!(inspection.warnings.iter().any(|w| w.contains("dégrad")));
    assert!(guard_preview(&output).is_ok());
    assert_eq!(fs::read(workspace.join("main.blend")).unwrap(), original);
}

#[test]
#[ignore = "requires explicitly configured Blender; uses disposable fixture only"]
fn previews_ignore_unused_font_and_missing_brush_library() {
    let root = test_directory();
    let workspace = fixture(root.path(), false, false);
    edit_fixture(
        root.path(),
        &workspace,
        r#"
import bpy, sys
from pathlib import Path
root=Path(sys.argv[-1]); bpy.ops.wm.open_mainfile(filepath=str(root/'main.blend'), use_scripts=False)
font=next((p for p in (Path('C:/Windows/Fonts/msyhbd.ttc'),Path('C:/Windows/Fonts/arial.ttf'),Path('/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf')) if p.is_file()),None)
assert font is not None, 'No fixture font found'
unused=bpy.data.fonts.load(str(font)); unused.use_fake_user=True
brush=bpy.data.brushes.new('Unused Brush', mode='SCULPT')
library=root.parent/'essentials_brushes-gp_draw.blend'
bpy.data.libraries.write(str(library), {brush}, fake_user=True)
bpy.data.brushes.remove(brush)
with bpy.data.libraries.load(str(library),link=True) as (source,destination): destination.brushes=source.brushes
for brush in destination.brushes: brush.use_fake_user=True
bpy.context.scene.tool_settings.sculpt.eraser_brush=destination.brushes[0]
bpy.ops.wm.save_as_mainfile(filepath=str(root/'main.blend'),check_existing=False)
library.unlink()
"#,
    );
    let original = fs::read(workspace.join("main.blend")).unwrap();
    let runner = BlenderRunner::new(executable());
    let output = root.path().join("secondary-preview.glb");
    let inspection = runner
        .preview_glb(
            &workspace,
            &workspace,
            &ExportConfig::default(),
            &output,
            &OperationContext::default(),
        )
        .unwrap();
    assert!(
        inspection
            .dependencies
            .iter()
            .any(|d| d.kind == "font" && !d.render_relevant)
    );
    assert!(
        inspection
            .dependencies
            .iter()
            .any(|d| d.kind == "library" && d.status == "missing" && !d.render_relevant),
        "{:?}",
        inspection.dependencies
    );
    assert!(inspection.warnings.is_empty(), "{:?}", inspection.warnings);
    assert!(guard_preview(&output).is_ok());
    let canonical = root.path().join("secondary-canonical.glb");
    runner
        .generate_glb(
            &workspace,
            &workspace,
            &ExportConfig::default(),
            &canonical,
            &OperationContext::default(),
        )
        .unwrap();
    assert!(guard_preview(&canonical).is_ok());
    assert_eq!(fs::read(workspace.join("main.blend")).unwrap(), original);
}

#[test]
#[ignore = "requires explicitly configured Blender; uses disposable fixture only"]
fn preview_rebases_external_relative_texture_from_a_staged_copy() {
    let root = test_directory();
    let workspace = fixture(root.path(), true, false);
    edit_fixture(
        root.path(),
        &workspace,
        r#"
import bpy, sys
from pathlib import Path
root=Path(sys.argv[-1]); bpy.ops.wm.open_mainfile(filepath=str(root/'main.blend'),use_scripts=False)
bpy.data.images['Albedo'].filepath='//../external.png'
bpy.ops.wm.save_as_mainfile(filepath=str(root/'main.blend'),check_existing=False)
"#,
    );
    let staged = root.path().join("temporary/deep/source");
    fs::create_dir_all(&staged).unwrap();
    fs::copy(workspace.join("main.blend"), staged.join("main.blend")).unwrap();
    let runner = BlenderRunner::new(executable());
    let output = root.path().join("relative-preview.glb");
    let inspection = runner
        .preview_glb(
            &staged,
            &workspace,
            &ExportConfig::default(),
            &output,
            &OperationContext::default(),
        )
        .unwrap();
    let image = inspection
        .dependencies
        .iter()
        .find(|d| d.kind == "image" && d.render_relevant)
        .unwrap();
    assert_eq!(image.status, "external");
    assert_eq!(
        fs::canonicalize(image.source_path.as_ref().unwrap()).unwrap(),
        fs::canonicalize(root.path().join("external.png")).unwrap()
    );
    assert!(guard_preview(&output).is_ok());
    runner
        .generate_glb(
            &staged,
            &workspace,
            &ExportConfig::default(),
            &root.path().join("relative-canonical.glb"),
            &OperationContext::default(),
        )
        .unwrap();
}

#[test]
#[ignore = "requires explicitly configured Blender; uses disposable fixture only"]
fn preview_reloads_nested_external_relative_linked_geometry() {
    let root = test_directory();
    let workspace = fixture(root.path(), true, false);
    edit_fixture(
        root.path(),
        &workspace,
        r#"
import bpy, sys
from pathlib import Path
root=Path(sys.argv[-1]); external=root.parent/'external'; (external/'nested').mkdir(parents=True)
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.mesh.primitive_cube_add()
cube=bpy.context.object; cube.name='LinkedCube'
geometry=bpy.data.collections.new('Geometry'); bpy.context.scene.collection.children.link(geometry)
for collection in list(cube.users_collection): collection.objects.unlink(cube)
geometry.objects.link(cube)
image=bpy.data.images.load(str(root.parent/'external.png')); image.filepath='//../../external.png'
material=bpy.data.materials.new('Linked Material'); material.use_nodes=True
node=material.node_tree.nodes.new('ShaderNodeTexImage'); node.image=image
material.node_tree.links.new(node.outputs['Color'],material.node_tree.nodes['Principled BSDF'].inputs['Base Color'])
cube.data.materials.append(material)
bpy.context.preferences.filepaths.save_version=0
bpy.ops.wm.save_as_mainfile(filepath=str(external/'nested/geometry.blend'),check_existing=False)
bpy.ops.wm.read_factory_settings(use_empty=True)
parent=bpy.data.collections.new('Parent'); bpy.context.scene.collection.children.link(parent)
with bpy.data.libraries.load(str(external/'nested/geometry.blend'),link=True) as (source,destination): destination.collections=source.collections
for collection in destination.collections: parent.children.link(collection)
bpy.data.libraries[0].filepath='//nested/geometry.blend'
bpy.ops.wm.save_as_mainfile(filepath=str(external/'parent.blend'),check_existing=False)
bpy.ops.wm.open_mainfile(filepath=str(root/'main.blend'),use_scripts=False)
with bpy.data.libraries.load(str(external/'parent.blend'),link=True) as (source,destination): destination.collections=['Parent']
bpy.context.scene.collection.children.link(destination.collections[0])
next(lib for lib in bpy.data.libraries if Path(lib.filepath).name=='parent.blend').filepath='//../external/parent.blend'
bpy.ops.wm.save_as_mainfile(filepath=str(root/'main.blend'),check_existing=False)
"#,
    );
    let original = fs::read(workspace.join("main.blend")).unwrap();
    let staged = root.path().join("temporary/deep/source");
    fs::create_dir_all(&staged).unwrap();
    fs::copy(workspace.join("main.blend"), staged.join("main.blend")).unwrap();
    let output = root.path().join("linked-preview.glb");
    let inspection = BlenderRunner::new(executable())
        .preview_glb(
            &staged,
            &workspace,
            &ExportConfig::default(),
            &output,
            &OperationContext::default(),
        )
        .unwrap();
    assert_eq!(inspection.geometry.mesh_objects, 3);
    for filename in ["parent.blend", "geometry.blend"] {
        assert!(
            inspection.dependencies.iter().any(|d| d.kind == "library"
                && d.render_relevant
                && d.status == "external"
                && d.source_path
                    .as_deref()
                    .is_some_and(|p| p.ends_with(filename))),
            "{:?}",
            inspection.dependencies
        );
    }
    assert!(guard_preview(&output).is_ok());
    BlenderRunner::new(executable())
        .generate_glb(
            &staged,
            &workspace,
            &ExportConfig::default(),
            &root.path().join("linked-canonical.glb"),
            &OperationContext::default(),
        )
        .unwrap();
    assert_eq!(fs::read(workspace.join("main.blend")).unwrap(), original);
}

#[test]
#[ignore = "requires explicitly configured Blender; uses disposable fixture only"]
fn exports_nested_collection_preserving_rig_animation_and_source() {
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
    assert_eq!(inspection.geometry.edges, Some(12));
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
    assert!(document["images"][0]["bufferView"].is_u64());
    let primitive = &document["meshes"][0]["primitives"][0];
    let material = primitive["material"].as_u64().unwrap() as usize;
    let texture =
        document["materials"][material]["pbrMetallicRoughness"]["baseColorTexture"]["index"]
            .as_u64()
            .expect("the Blender image must remain connected to the exported material")
            as usize;
    let image = document["textures"][texture]["source"].as_u64().unwrap() as usize;
    assert!(document["images"][image]["bufferView"].is_u64());
    let uv = primitive["attributes"]["TEXCOORD_0"]
        .as_u64()
        .expect("textured geometry must preserve UV coordinates") as usize;
    assert_eq!(document["accessors"][uv]["type"], "VEC2");
    assert_eq!(fs::read(workspace.join("main.blend")).unwrap(), original);
}

#[test]
#[ignore = "requires explicitly configured Blender; uses disposable fixture only"]
fn applies_mirror_even_when_legacy_configuration_disables_modifiers() {
    let root = test_directory();
    let workspace = fixture(root.path(), false, false);
    edit_fixture(
        root.path(),
        &workspace,
        r#"
import bpy, sys
from pathlib import Path
root=Path(sys.argv[-1]); bpy.ops.wm.open_mainfile(filepath=str(root/'main.blend'),use_scripts=False)
cube=bpy.data.objects['TexturedCube']; mirror=cube.modifiers.new('Mirror','MIRROR'); mirror.use_mirror_merge=False
bpy.ops.wm.save_as_mainfile(filepath=str(root/'main.blend'),check_existing=False)
"#,
    );
    let original = fs::read(workspace.join("main.blend")).unwrap();
    let config = ExportConfig {
        collection: Some("Asset".into()),
        apply_modifiers: false,
        ..Default::default()
    };
    let output = root.path().join("mirrored.glb");
    BlenderRunner::new(executable())
        .export_glb(&workspace, &config, &output, &OperationContext::default())
        .unwrap();
    assert_eq!(inspect_glb(&output).unwrap().geometry.triangles, 24);
    assert_eq!(fs::read(workspace.join("main.blend")).unwrap(), original);
}

#[test]
#[ignore = "requires explicitly configured Blender; uses disposable fixture only"]
fn armature_morphs_keep_drivers_animation_and_sources_across_all_export_paths() {
    let root = test_directory();
    let workspace = fixture_with_shapes(root.path(), false, false, true);
    edit_fixture(
        root.path(),
        &workspace,
        r#"
import bpy, sys
from pathlib import Path
root=Path(sys.argv[-1]); bpy.ops.wm.open_mainfile(filepath=str(root/'main.blend'),use_scripts=False)
cube=bpy.data.objects['TexturedCube']; rig=bpy.data.objects['Rig']
key=cube.shape_key_add(name='Driven'); key.data[1].co.x+=0.4
driver=key.driver_add('value').driver
variable=driver.variables.new(); variable.name='rotation'; variable.type='SINGLE_PROP'
variable.targets[0].id=rig; variable.targets[0].data_path='pose.bones["Root"].rotation_euler.z'
driver.expression='rotation'
muted=cube.shape_key_add(name='Muted'); muted.data[2].co.y+=0.5; muted.mute=True
bpy.context.scene.frame_end=10; bpy.context.scene.frame_set(1)
bpy.ops.wm.save_as_mainfile(filepath=str(root/'main.blend'),check_existing=False)
"#,
    );
    let original = fs::read(workspace.join("main.blend")).unwrap();
    let runner = BlenderRunner::new(executable());
    let context = OperationContext::default();
    let config = ExportConfig::default();
    for operation in ["export", "generate", "preview"] {
        let output = root.path().join(format!("{operation}.glb"));
        let result = match operation {
            "export" => runner.export_glb(&workspace, &config, &output, &context),
            "generate" => runner.generate_glb(&workspace, &workspace, &config, &output, &context),
            _ => runner.preview_glb(&workspace, &workspace, &config, &output, &context),
        };
        result.unwrap_or_else(|error| {
            panic!("{operation} rejected compatible Armature/morphs: {error}")
        });
        assert_animated_skin_and_morphs(&output, "TexturedCube");
        assert!(guard_preview(&output).is_ok());
        assert_eq!(fs::read(workspace.join("main.blend")).unwrap(), original);
    }

    let library = Library::open(root.path().join("library")).unwrap();
    let imported =
        sync::import_project(&library, &workspace, "Animated face", &runner, &context).unwrap();
    let variant = &imported.variants[0];
    let working = Path::new(&variant.workspace_path);
    let canonical = working.join("main.glb");
    assert_animated_skin_and_morphs(&canonical, "TexturedCube");
    let last_glb = fs::read(&canonical).unwrap();
    let snapshot = library.get_snapshot(&variant.head_snapshot_id).unwrap();
    edit_fixture(
        root.path(),
        working,
        r#"
import bpy, sys
from pathlib import Path
root=Path(sys.argv[-1]); bpy.ops.wm.open_mainfile(filepath=str(root/'main.blend'),use_scripts=False)
bpy.data.objects['TexturedCube'].modifiers.new('Unsupported Mirror','MIRROR')
bpy.ops.wm.save_as_mainfile(filepath=str(root/'main.blend'),check_existing=False)
"#,
    );
    let edited = fs::read(working.join("main.blend")).unwrap();
    let error = sync::synchronize_variant(&library, &variant.id, Some(&runner), &context)
        .err()
        .unwrap();
    assert!(error.to_string().contains("TexturedCube"), "{error}");
    assert!(error.to_string().contains("Mirror"), "{error}");
    assert_eq!(fs::read(&canonical).unwrap(), last_glb);
    assert_eq!(fs::read(working.join("main.blend")).unwrap(), edited);
    assert_eq!(
        library.get_snapshot(&snapshot.id).unwrap().entries,
        snapshot.entries
    );
    assert_eq!(fs::read(workspace.join("main.blend")).unwrap(), original);
}

#[test]
#[ignore = "requires explicitly configured Blender; uses disposable fixture only"]
fn armature_morph_export_keeps_other_modifiers_in_nested_shared_and_instanced_meshes() {
    for morph_first in [false, true] {
        let root = test_directory();
        let workspace = fixture_with_shapes(root.path(), false, false, true);
        let setup = format!(
            r#"
import bpy, sys
from pathlib import Path
root=Path(sys.argv[-1]); bpy.ops.wm.open_mainfile(filepath=str(root/'main.blend'),use_scripts=False)
cube=bpy.data.objects['TexturedCube']; rig=bpy.data.objects['Rig']
cube.name='A_Morph' if {morph_first} else 'Z_Morph'
plain=next(obj for obj in bpy.context.scene.objects if obj.type=='MESH' and obj!=cube)
plain.name='Z_Mirror' if {morph_first} else 'A_Mirror'
plain.modifiers.new('Mirror','MIRROR').use_mirror_merge=False
child=plain.copy(); child.data=plain.data.copy(); child.name='NestedMirror'
bpy.context.scene.collection.objects.link(child); child.parent=cube
shared=cube.copy(); shared.name='SharedMorph'; shared.parent=rig
bpy.data.collections['Nested'].objects.link(shared)
instance=bpy.data.objects.new('Instance',None); instance.instance_type='COLLECTION'; instance.instance_collection=bpy.data.collections['Asset']
bpy.data.collections['Other'].objects.link(instance); instance.location.x=10
bpy.context.scene.frame_end=10; bpy.context.scene.frame_set(1)
bpy.ops.wm.save_as_mainfile(filepath=str(root/'main.blend'),check_existing=False)
"#,
            morph_first = if morph_first { "True" } else { "False" }
        );
        edit_fixture(root.path(), &workspace, &setup);
        let original = fs::read(workspace.join("main.blend")).unwrap();
        let output = root.path().join("mixed.glb");
        BlenderRunner::new(executable())
            .generate_glb(
                &workspace,
                &workspace,
                &ExportConfig::default(),
                &output,
                &OperationContext::default(),
            )
            .unwrap();
        let (document, _) = glb_document(&output);
        let nodes = document["nodes"].as_array().unwrap();
        let mut morph_nodes = 0;
        let mut mirrored_nodes = 0;
        for node in nodes {
            let Some(mesh_index) = node["mesh"].as_u64() else {
                continue;
            };
            let name = node["name"].as_str().unwrap_or("");
            let primitives = document["meshes"][mesh_index as usize]["primitives"]
                .as_array()
                .unwrap();
            if name.contains("Morph") {
                morph_nodes += 1;
                assert!(node["skin"].is_u64(), "{name} lost its skin");
                assert!(
                    primitives.iter().all(|primitive| primitive["targets"]
                        .as_array()
                        .is_some_and(|targets| targets.len() == 1)),
                    "{name} lost its morph"
                );
            }
            if name.contains("Mirror") {
                mirrored_nodes += 1;
                let triangles: u64 = primitives
                    .iter()
                    .map(|primitive| {
                        let indices = primitive["indices"].as_u64().unwrap() as usize;
                        document["accessors"][indices]["count"].as_u64().unwrap() / 3
                    })
                    .sum();
                assert_eq!(
                    triangles, 24,
                    "{name} must keep Mirror even near a morph mesh"
                );
            }
        }
        assert!(
            morph_nodes >= 4,
            "original/shared morphs and their instances must export"
        );
        assert!(
            mirrored_nodes >= 3,
            "root/nested mirrors and their instances must export"
        );
        assert_eq!(fs::read(workspace.join("main.blend")).unwrap(), original);
    }
}

#[test]
#[ignore = "requires explicitly configured Blender; uses disposable fixture only"]
fn previews_evaluated_solidify_with_morphs_and_keeps_strict_exports() {
    for instanced_only in [false, true] {
        let root = test_directory();
        let workspace = fixture_with_shapes(root.path(), false, false, true);
        let runner = BlenderRunner::new(executable());
        let context = OperationContext::default();
        let config = ExportConfig::default();
        let previous_glb = workspace.join("main.glb");
        runner
            .generate_glb(&workspace, &workspace, &config, &previous_glb, &context)
            .unwrap();
        let previous_bytes = fs::read(&previous_glb).unwrap();
        let setup = format!(
            r#"
import bpy, sys
from pathlib import Path
root=Path(sys.argv[-1]); bpy.ops.wm.open_mainfile(filepath=str(root/'main.blend'),use_scripts=False)
cube=bpy.data.objects['TexturedCube']
cube.data.shape_keys.key_blocks['Smile'].data[0].co.z+=2.0
child=cube.copy(); child.name='PreservedMorph'; child.parent=cube; child.modifiers.clear()
cube.users_collection[0].objects.link(child)
solidify=cube.modifiers.new('Épaisseur','SOLIDIFY'); solidify.thickness=0.2
cube.modifiers.move(1,0)
if {instanced_only}:
    source=bpy.data.collections.new('InstanceSource')
    for obj in (cube,child):
        for col in list(obj.users_collection): col.objects.unlink(obj)
        source.objects.link(obj)
    cube.parent=None
    instance=bpy.data.objects.new('Instance',None); instance.instance_type='COLLECTION'; instance.instance_collection=source
    bpy.context.scene.collection.objects.link(instance)
    assert cube.name not in bpy.context.scene.objects
bpy.context.scene.frame_set(10)
bpy.ops.wm.save_as_mainfile(filepath=str(root/'main.blend'),check_existing=False)
"#,
            instanced_only = if instanced_only { "True" } else { "False" }
        );
        edit_fixture(root.path(), &workspace, &setup);
        let original = fs::read(workspace.join("main.blend")).unwrap();
        let preview = root.path().join("preview.glb");
        let inspection = runner
            .preview_glb(&workspace, &workspace, &config, &preview, &context)
            .unwrap();
        assert!(
            inspection.warnings.iter().any(|warning| {
                warning.contains("TexturedCube") && warning.contains("Épaisseur")
            })
        );
        assert!(guard_preview(&preview).is_ok());
        let (document, _) = glb_document(&preview);
        let nodes = document["nodes"].as_array().unwrap();
        let (degraded_index, degraded) = nodes
            .iter()
            .enumerate()
            .find(|(_, node)| node["name"] == "TexturedCube")
            .unwrap();
        assert!(degraded["skin"].is_u64());
        let mesh = &document["meshes"][degraded["mesh"].as_u64().unwrap() as usize];
        assert!(mesh["weights"].is_null());
        assert!(mesh["extras"]["targetNames"].is_null());
        let mut triangles = 0;
        let mut max_y = f64::NEG_INFINITY;
        for primitive in mesh["primitives"].as_array().unwrap() {
            assert!(primitive["targets"].is_null());
            assert!(primitive["attributes"]["JOINTS_0"].is_u64());
            assert!(primitive["attributes"]["WEIGHTS_0"].is_u64());
            let positions = primitive["attributes"]["POSITION"].as_u64().unwrap() as usize;
            let indices = primitive["indices"].as_u64().unwrap() as usize;
            max_y = max_y.max(document["accessors"][positions]["max"][1].as_f64().unwrap());
            triangles += document["accessors"][indices]["count"].as_u64().unwrap() / 3;
        }
        assert_eq!(triangles, 24, "Solidify must preserve both surfaces");
        assert!(max_y > 1.1, "saved morph pose was lost: {max_y}");
        let (preserved_index, preserved) = nodes
            .iter()
            .enumerate()
            .find(|(_, node)| node["name"] == "PreservedMorph")
            .unwrap();
        let preserved_mesh = &document["meshes"][preserved["mesh"].as_u64().unwrap() as usize];
        assert_eq!(
            preserved_mesh["extras"]["targetNames"],
            serde_json::json!(["Smile"])
        );
        let mut has_morph_animation = false;
        let mut has_bone_animation = false;
        for animation in document["animations"].as_array().unwrap() {
            let channels = animation["channels"].as_array().unwrap();
            assert!(!channels.is_empty());
            let mut used_samplers = std::collections::BTreeSet::new();
            for channel in channels {
                used_samplers.insert(channel["sampler"].as_u64().unwrap());
                has_bone_animation |= channel["target"]["path"] == "rotation";
                if channel["target"]["path"] == "weights" {
                    assert_ne!(channel["target"]["node"], degraded_index);
                    has_morph_animation |= channel["target"]["node"] == preserved_index;
                }
            }
            assert_eq!(
                used_samplers.len(),
                animation["samplers"].as_array().unwrap().len()
            );
        }
        assert!(has_morph_animation && has_bone_animation);

        let exported = root.path().join("export.glb");
        let generated = root.path().join("generate.glb");
        for error in [
            runner
                .export_glb(&workspace, &config, &exported, &context)
                .unwrap_err(),
            runner
                .generate_glb(&workspace, &workspace, &config, &generated, &context)
                .unwrap_err(),
        ] {
            assert!(error.to_string().contains("TexturedCube"), "{error}");
            assert!(error.to_string().contains("Épaisseur"), "{error}");
        }
        assert!(!exported.exists() && !generated.exists());
        assert_eq!(fs::read(workspace.join("main.blend")).unwrap(), original);
        assert_eq!(fs::read(previous_glb).unwrap(), previous_bytes);
    }
}

#[test]
#[ignore = "requires explicitly configured Blender; uses disposable fixture only"]
fn rejects_morph_stacks_outside_the_supported_single_active_armature_case() {
    for alteration in [
        "cube.modifiers['Rig'].show_viewport=False",
        "modifier=cube.modifiers.new('Disabled Mirror','MIRROR'); modifier.show_viewport=False; modifier.show_render=False",
        "cube.data.shape_keys.use_relative=False",
    ] {
        let root = test_directory();
        let workspace = fixture_with_shapes(root.path(), false, false, true);
        let setup = format!(
            r#"
import bpy, sys
from pathlib import Path
root=Path(sys.argv[-1]); bpy.ops.wm.open_mainfile(filepath=str(root/'main.blend'),use_scripts=False)
cube=bpy.data.objects['TexturedCube']
{alteration}
bpy.ops.wm.save_as_mainfile(filepath=str(root/'main.blend'),check_existing=False)
"#
        );
        edit_fixture(root.path(), &workspace, &setup);
        let original = fs::read(workspace.join("main.blend")).unwrap();
        let output = root.path().join("unsupported.glb");
        let error = BlenderRunner::new(executable())
            .generate_glb(
                &workspace,
                &workspace,
                &ExportConfig::default(),
                &output,
                &OperationContext::default(),
            )
            .unwrap_err();
        assert!(error.to_string().contains("TexturedCube"), "{error}");
        assert!(!output.exists());
        assert_eq!(fs::read(workspace.join("main.blend")).unwrap(), original);
    }
}

#[test]
#[ignore = "requires explicitly configured Blender; uses disposable fixture only"]
fn muted_morphs_keep_native_skin_without_requiring_targets() {
    let root = test_directory();
    let workspace = fixture_with_shapes(root.path(), false, false, true);
    edit_fixture(
        root.path(),
        &workspace,
        r#"
import bpy, sys
from pathlib import Path
root=Path(sys.argv[-1]); bpy.ops.wm.open_mainfile(filepath=str(root/'main.blend'),use_scripts=False)
cube=bpy.data.objects['TexturedCube']; cube.data.shape_keys.key_blocks['Smile'].mute=True
ignored=cube.shape_key_add(name='Self relative'); ignored.relative_key=ignored
for name,skinned in [('EmptyMorph',False),('EmptySkinnedMorph',True)]:
    empty=bpy.data.objects.new(name,bpy.data.meshes.new(name)); bpy.context.scene.collection.objects.link(empty)
    empty.shape_key_add(name='Basis'); empty.shape_key_add(name='Empty expression')
    if skinned:
        empty.modifiers.new('Rig','ARMATURE').object=bpy.data.objects['Rig']
bpy.context.scene.frame_end=10
bpy.ops.wm.save_as_mainfile(filepath=str(root/'main.blend'),check_existing=False)
"#,
    );
    let original = fs::read(workspace.join("main.blend")).unwrap();
    let output = root.path().join("muted.glb");
    BlenderRunner::new(executable())
        .generate_glb(
            &workspace,
            &workspace,
            &ExportConfig::default(),
            &output,
            &OperationContext::default(),
        )
        .unwrap();
    let (document, _) = glb_document(&output);
    for name in ["EmptyMorph", "EmptySkinnedMorph"] {
        assert!(
            document["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .all(|node| { node["name"] != name || node["mesh"].is_null() })
        );
    }
    let node = document["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["name"] == "TexturedCube")
        .unwrap();
    assert!(node["skin"].is_u64());
    let mesh = &document["meshes"][node["mesh"].as_u64().unwrap() as usize];
    assert!(
        mesh["primitives"]
            .as_array()
            .unwrap()
            .iter()
            .all(|primitive| {
                primitive["targets"]
                    .as_array()
                    .is_none_or(|targets| targets.is_empty())
                    && primitive["attributes"]["JOINTS_0"].is_u64()
                    && primitive["attributes"]["WEIGHTS_0"].is_u64()
            })
    );
    assert_eq!(fs::read(workspace.join("main.blend")).unwrap(), original);
}

#[test]
#[ignore = "requires explicitly configured Blender; uses disposable fixture only"]
fn morph_export_rejects_missing_callbacks_and_lost_targets() {
    let root = test_directory();
    let workspace = fixture_with_shapes(root.path(), false, false, true);
    let original = fs::read(workspace.join("main.blend")).unwrap();
    let worker = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("blender/edge_lock.py");
    let setup = format!(
        r#"
import bpy, runpy, sys
from pathlib import Path
root=Path(sys.argv[-1])
for broken_callbacks in (True,False):
    worker=runpy.run_path({worker})
    extension=worker['MorphSafeExport']
    if broken_callbacks:
        delattr(extension,'gather_node_mesh_hook'); delattr(extension,'gather_node_hook')
    else:
        original_hook=extension.gather_node_hook
        def lose_targets(self,node,obj,settings):
            if obj and obj.type=='MESH' and obj.data.shape_keys and node.mesh:
                node.mesh.primitives[0].targets=[]
            original_hook(self,node,obj,settings)
        extension.gather_node_hook=lose_targets
    output=root.parent/('missing-callbacks.glb' if broken_callbacks else 'lost-targets.glb')
    try:
        worker['run']({{'protocolVersion':1,'operation':'generate','workspace':str(root),'blendFile':str(root/'main.blend'),'destination':str(output)}})
    except ValueError as error:
        assert 'GLB non publié' in str(error),str(error)
        assert 'Callbacks' in str(error) if broken_callbacks else 'morphs' in str(error),str(error)
    else:
        raise AssertionError('A degraded export must be rejected even when Blender reports FINISHED')
    finally:
        output.unlink(missing_ok=True)
"#,
        worker = serde_json::to_string(&worker).unwrap()
    );
    edit_fixture(root.path(), &workspace, &setup);
    assert_eq!(fs::read(workspace.join("main.blend")).unwrap(), original);
}

#[test]
#[ignore = "requires explicitly configured Blender; uses disposable fixture only"]
fn generates_complete_active_scene_with_modifiers_and_keeps_source_intact() {
    let root = test_directory();
    let workspace = fixture(root.path(), false, false);
    edit_fixture(
        root.path(),
        &workspace,
        r#"
import bpy, sys
from pathlib import Path
root=Path(sys.argv[-1]); bpy.ops.wm.open_mainfile(filepath=str(root/'main.blend'),use_scripts=False)
mirror=bpy.data.objects['TexturedCube'].modifiers.new('Mirror','MIRROR'); mirror.use_mirror_merge=False
bpy.ops.wm.save_as_mainfile(filepath=str(root/'main.blend'),check_existing=False)
"#,
    );
    let original = fs::read(workspace.join("main.blend")).unwrap();
    let staged = root.path().join("staged");
    fs::create_dir(&staged).unwrap();
    fs::copy(workspace.join("main.blend"), staged.join("main.blend")).unwrap();
    fs::create_dir(staged.join("textures")).unwrap();
    fs::copy(
        workspace.join("textures/albedo.png"),
        staged.join("textures/albedo.png"),
    )
    .unwrap();
    let output = root.path().join("canonical.glb");
    BlenderRunner::new(executable())
        .generate_glb(
            &staged,
            &workspace,
            &ExportConfig {
                scene: Some("obsolete scene selection".into()),
                collection: Some("Asset".into()),
                apply_modifiers: false,
                ..Default::default()
            },
            &output,
            &OperationContext::default(),
        )
        .unwrap();
    let stats = inspect_glb(&output).unwrap();
    assert_eq!(stats.geometry.mesh_objects, 2);
    assert_eq!(stats.geometry.triangles, 36);
    assert_eq!(fs::read(workspace.join("main.blend")).unwrap(), original);
    assert_eq!(fs::read(staged.join("main.blend")).unwrap(), original);
}

#[test]
#[ignore = "requires explicitly configured Blender; uses disposable fixture only"]
fn canonical_generation_preserves_shape_keys_without_modifiers() {
    let root = test_directory();
    let workspace = fixture_with_shapes(root.path(), false, false, true);
    edit_fixture(
        root.path(),
        &workspace,
        r#"
import bpy, sys
from pathlib import Path
root=Path(sys.argv[-1]); bpy.ops.wm.open_mainfile(filepath=str(root/'main.blend'),use_scripts=False)
cube=bpy.data.objects['TexturedCube']; cube.modifiers.clear(); cube.parent=None
bpy.ops.wm.save_as_mainfile(filepath=str(root/'main.blend'),check_existing=False)
"#,
    );
    let original = fs::read(workspace.join("main.blend")).unwrap();
    let output = root.path().join("morph.glb");
    BlenderRunner::new(executable())
        .generate_glb(
            &workspace,
            &workspace,
            &ExportConfig::default(),
            &output,
            &OperationContext::default(),
        )
        .unwrap();
    let bytes = fs::read(&output).unwrap();
    let json_length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let document: serde_json::Value = serde_json::from_slice(&bytes[20..20 + json_length]).unwrap();
    assert!(document["meshes"].as_array().unwrap().iter().any(|mesh| {
        mesh["primitives"]
            .as_array()
            .unwrap()
            .iter()
            .any(|primitive| {
                primitive["targets"]
                    .as_array()
                    .is_some_and(|targets| !targets.is_empty())
            })
    }));
    assert!(!inspect_glb(&output).unwrap().animations.is_empty());
    assert_eq!(fs::read(workspace.join("main.blend")).unwrap(), original);
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
    let original = fs::read(workspace.join("main.blend")).unwrap();
    let output = root.path().join("empty.glb");
    runner
        .generate_glb(
            &workspace,
            &workspace,
            &ExportConfig::default(),
            &output,
            &context,
        )
        .unwrap();
    assert_eq!(inspect_glb(&output).unwrap().geometry.mesh_objects, 0);
    assert_eq!(fs::read(workspace.join("main.blend")).unwrap(), original);
}
