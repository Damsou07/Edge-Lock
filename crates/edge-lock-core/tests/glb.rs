use edge_lock_core::glb::{GeometryStats, guard_preview, inspect_glb, validate_glb};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

fn write_glb(path: &Path, document: Value, binary: &[u8]) {
    let mut json = serde_json::to_vec(&document).unwrap();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let mut bin = binary.to_vec();
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let length = 12 + 8 + json.len() + if bin.is_empty() { 0 } else { 8 + bin.len() };
    let mut data = Vec::new();
    data.extend_from_slice(b"glTF");
    data.extend_from_slice(&2u32.to_le_bytes());
    data.extend_from_slice(&(length as u32).to_le_bytes());
    data.extend_from_slice(&(json.len() as u32).to_le_bytes());
    data.extend_from_slice(&0x4e4f534au32.to_le_bytes());
    data.extend_from_slice(&json);
    if !bin.is_empty() {
        data.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        data.extend_from_slice(&0x004e4942u32.to_le_bytes());
        data.extend_from_slice(&bin);
    }
    fs::write(path, data).unwrap();
}

fn triangle() -> Value {
    json!({"asset":{"version":"2.0"},"buffers":[{"byteLength":36}],
        "bufferViews":[{"buffer":0,"byteLength":36}],
        "accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"}],
        "meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}],
        "nodes":[{"mesh":0},{"mesh":0}],"scenes":[{"nodes":[0,1]}],"scene":0})
}

#[test]
fn counts_instanced_geometry_and_clips() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model.glb");
    let mut document = triangle();
    document["animations"] = json!([{"name":"Idle","channels":[],"samplers":[]}]);
    write_glb(&path, document, &[0; 36]);
    let stats = inspect_glb(&path).unwrap();
    assert_eq!(stats.geometry.mesh_objects, 2);
    assert_eq!(stats.geometry.vertices, 6);
    assert_eq!(stats.geometry.triangles, 2);
    assert_eq!(stats.geometry.edges, Some(6));
    assert_eq!(stats.animations, ["Idle"]);
    assert!(guard_preview(&path).is_ok());
}

fn indexed_geometry(mode: u64, indices: &[u32], component: u64) -> (Value, Vec<u8>) {
    let mut binary = vec![0; 4 * 12];
    for value in indices {
        match component {
            5121 => binary.push(*value as u8),
            5123 => binary.extend_from_slice(&(*value as u16).to_le_bytes()),
            5125 => binary.extend_from_slice(&value.to_le_bytes()),
            _ => unreachable!(),
        }
    }
    let document = json!({"asset":{"version":"2.0"},"buffers":[{"byteLength":binary.len()}],
        "bufferViews":[{"buffer":0,"byteLength":48},{"buffer":0,"byteOffset":48,"byteLength":binary.len()-48}],
        "accessors":[{"bufferView":0,"componentType":5126,"count":4,"type":"VEC3"},
            {"bufferView":1,"componentType":component,"count":indices.len(),"type":"SCALAR"}],
        "meshes":[{"primitives":[{"attributes":{"POSITION":0},"indices":1,"mode":mode}]}],
        "nodes":[{"mesh":0}],"scenes":[{"nodes":[0]}]});
    (document, binary)
}

#[test]
fn counts_unique_edges_for_all_primitive_modes_and_index_widths() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model.glb");
    for component in [5121, 5123, 5125] {
        for (mode, indices, expected) in [
            (0, vec![0, 1, 2, 3], 0),
            (1, vec![0, 1, 2, 3], 2),
            (2, vec![0, 1, 2, 3], 4),
            (3, vec![0, 1, 2, 3], 3),
            (4, vec![0, 1, 2, 0, 2, 3], 5),
            (5, vec![0, 1, 2, 3], 5),
            (6, vec![0, 1, 2, 3], 5),
        ] {
            let (document, binary) = indexed_geometry(mode, &indices, component);
            write_glb(&path, document, &binary);
            assert_eq!(
                inspect_glb(&path).unwrap().geometry.edges,
                Some(expected),
                "mode {mode}, component {component}"
            );
        }
    }
}

#[test]
fn counts_nonindexed_topology_and_only_active_scene_instances() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model.glb");
    for (mode, expected) in [(0, 0), (1, 3), (2, 6), (3, 5), (4, 6), (5, 9), (6, 9)] {
        let mut document = triangle();
        document["accessors"][0]
            .as_object_mut()
            .unwrap()
            .remove("bufferView");
        document["accessors"][0]["count"] = json!(6);
        document["meshes"][0]["primitives"][0]["mode"] = json!(mode);
        document["scenes"] = json!([{"nodes":[0]},{"nodes":[0,1]}]);
        write_glb(&path, document, &[0; 36]);
        assert_eq!(
            inspect_glb(&path).unwrap().geometry.edges,
            Some(expected),
            "mode {mode}"
        );
    }
}

#[test]
fn deduplicates_shared_position_topology_and_counts_mesh_instances() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model.glb");
    let (mut document, binary) = indexed_geometry(4, &[0, 1, 2, 0, 2, 3], 5123);
    let primitive = document["meshes"][0]["primitives"][0].clone();
    document["meshes"][0]["primitives"]
        .as_array_mut()
        .unwrap()
        .push(primitive);
    document["nodes"] = json!([{"mesh":0},{"mesh":0}]);
    document["scenes"][0]["nodes"] = json!([0, 1]);
    write_glb(&path, document.clone(), &binary);
    assert_eq!(inspect_glb(&path).unwrap().geometry.edges, Some(10));
    let position = document["accessors"][0].clone();
    document["accessors"].as_array_mut().unwrap().push(position);
    document["meshes"][0]["primitives"][1]["attributes"]["POSITION"] = json!(2);
    write_glb(&path, document, &binary);
    assert_eq!(inspect_glb(&path).unwrap().geometry.edges, Some(20));
}

#[test]
fn reads_accessor_offsets_strides_and_sparse_overrides_of_base_indices() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model.glb");
    let (mut document, _) = indexed_geometry(4, &[0, 1, 2, 0, 2, 0], 5123);
    let mut binary = vec![0; 50];
    for value in [0u16, 1, 2, 0, 2, 0] {
        binary.extend_from_slice(&value.to_le_bytes());
        binary.extend_from_slice(&[255, 255]);
    }
    let sparse_offset = binary.len();
    binary.extend_from_slice(&[5, 0, 3, 0]);
    document["buffers"][0]["byteLength"] = json!(binary.len());
    document["bufferViews"][1] =
        json!({"buffer":0,"byteOffset":48,"byteLength":sparse_offset-48,"byteStride":4});
    document["bufferViews"].as_array_mut().unwrap().extend([
        json!({"buffer":0,"byteOffset":sparse_offset,"byteLength":1}),
        json!({"buffer":0,"byteOffset":sparse_offset+2,"byteLength":2}),
    ]);
    document["accessors"][1]["byteOffset"] = json!(2);
    document["accessors"][1]["sparse"] = json!({"count":1,"indices":{"bufferView":2,"componentType":5121},"values":{"bufferView":3}});
    write_glb(&path, document, &binary);
    assert_eq!(inspect_glb(&path).unwrap().geometry.edges, Some(5));
}

#[test]
fn reads_sparse_indices_without_a_base_buffer_and_ignores_degenerate_edges() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model.glb");
    let (mut document, _) = indexed_geometry(4, &[0, 1, 2, 0, 2, 3], 5123);
    let mut binary = vec![1, 2, 4, 5];
    for value in [1u16, 2, 2, 3] {
        binary.extend_from_slice(&value.to_le_bytes());
    }
    document["buffers"][0]["byteLength"] = json!(binary.len());
    document["bufferViews"] =
        json!([{"buffer":0,"byteLength":4},{"buffer":0,"byteOffset":4,"byteLength":8}]);
    document["accessors"][0]
        .as_object_mut()
        .unwrap()
        .remove("bufferView");
    document["accessors"][1] = json!({"componentType":5123,"count":6,"type":"SCALAR",
        "sparse":{"count":4,"indices":{"bufferView":0,"componentType":5121},"values":{"bufferView":1}}});
    write_glb(&path, document, &binary);
    assert_eq!(inspect_glb(&path).unwrap().geometry.edges, Some(5));
    let (document, binary) = indexed_geometry(4, &[0, 0, 1], 5121);
    write_glb(&path, document, &binary);
    assert_eq!(inspect_glb(&path).unwrap().geometry.edges, Some(1));
}

#[test]
fn rejects_out_of_range_and_unordered_sparse_indices() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model.glb");
    let (document, binary) = indexed_geometry(4, &[0, 1, 4], 5121);
    write_glb(&path, document, &binary);
    assert!(inspect_glb(&path).is_err());
    let (mut document, mut binary) = indexed_geometry(4, &[0, 1, 2], 5121);
    binary.extend_from_slice(&[1, 0, 2, 3]);
    document["buffers"][0]["byteLength"] = json!(binary.len());
    document["bufferViews"].as_array_mut().unwrap().extend([
        json!({"buffer":0,"byteOffset":51,"byteLength":2}),
        json!({"buffer":0,"byteOffset":53,"byteLength":2}),
    ]);
    document["accessors"][1]["sparse"] = json!({"count":2,"indices":{"bufferView":2,"componentType":5121},"values":{"bufferView":3}});
    write_glb(&path, document, &binary);
    assert!(inspect_glb(&path).is_err());
}

#[test]
fn edge_budget_and_legacy_metadata_do_not_prevent_preview() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model.glb");
    let mut document = triangle();
    document["accessors"][0]
        .as_object_mut()
        .unwrap()
        .remove("bufferView");
    document["accessors"][0]["count"] = json!(6_000_000);
    write_glb(&path, document, &[0; 36]);
    assert_eq!(inspect_glb(&path).unwrap().geometry.edges, None);
    assert_eq!(guard_preview(&path).unwrap().geometry.edges, None);
    validate_glb(&path).unwrap();
    let legacy: GeometryStats =
        serde_json::from_value(json!({"meshObjects":1,"vertices":8,"faces":6,"triangles":12}))
            .unwrap();
    assert_eq!(legacy.edges, None);
}

#[test]
fn rejects_external_images_and_buffers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model.glb");
    for uri in [
        "https://example.test/texture.png",
        "../private.png",
        "file:///C:/secret.png",
    ] {
        let mut document = triangle();
        document["images"] = json!([{"uri":uri}]);
        write_glb(&path, document, &[0; 36]);
        assert!(inspect_glb(&path).is_err(), "accepted {uri}");
    }
    let mut document = triangle();
    document["buffers"][0]["uri"] = json!("outside.bin");
    write_glb(&path, document, &[0; 36]);
    assert!(inspect_glb(&path).is_err());
}

#[test]
fn preview_rejects_embedded_svg_images_without_decoding_them() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model.glb");
    let mut document = triangle();
    document["images"] = json!([{"bufferView":0,"mimeType":"image/svg+xml"}]);
    write_glb(&path, document, &[0; 36]);
    assert!(guard_preview(&path).is_err());
}

#[test]
fn rejects_truncation_out_of_bounds_and_cycles() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model.glb");
    write_glb(&path, triangle(), &[0; 36]);
    let mut bytes = fs::read(&path).unwrap();
    bytes.pop();
    fs::write(&path, bytes).unwrap();
    assert!(inspect_glb(&path).is_err());
    let mut document = triangle();
    document["accessors"][0]["count"] = json!(u64::MAX);
    write_glb(&path, document, &[0; 36]);
    assert!(inspect_glb(&path).is_err());
    let mut document = triangle();
    document["nodes"][0]["children"] = json!([1]);
    document["nodes"][1]["children"] = json!([0]);
    write_glb(&path, document, &[0; 36]);
    assert!(inspect_glb(&path).is_err());
}

#[test]
fn preview_accepts_large_texture_metadata_without_decoding_image() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model.glb");
    let mut binary = vec![0; 36];
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    png.extend_from_slice(&20_000u32.to_be_bytes());
    png.extend_from_slice(&20_000u32.to_be_bytes());
    binary.extend_from_slice(&png);
    let mut document = triangle();
    document["buffers"][0]["byteLength"] = json!(binary.len());
    document["bufferViews"]
        .as_array_mut()
        .unwrap()
        .push(json!({"buffer":0,"byteOffset":36,"byteLength":png.len()}));
    document["images"] = json!([{"bufferView":1,"mimeType":"image/png"}]);
    write_glb(&path, document, &binary);
    assert!(
        inspect_glb(&path).is_ok(),
        "large textures remain exportable"
    );
    assert!(
        guard_preview(&path).is_ok(),
        "preview must not impose a decoded texture quota"
    );
}

#[test]
fn preview_accepts_large_geometry_and_json_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model.glb");
    let mut document = triangle();
    document["accessors"][0]
        .as_object_mut()
        .unwrap()
        .remove("bufferView");
    document["accessors"][0]["count"] = json!(48_000_000);
    document["extras"] = json!({"notes":"x".repeat(16 * 1024 * 1024)});
    write_glb(&path, document, &[0; 36]);
    assert!(guard_preview(&path).is_ok());
}

#[test]
fn preview_accepts_a_glb_file_above_256_mib_without_loading_its_buffer() {
    use std::io::{Seek, SeekFrom, Write};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("large.glb");
    let binary_length = 257 * 1024 * 1024u32;
    let mut document = triangle();
    document["buffers"][0]["byteLength"] = json!(binary_length);
    write_glb(&path, document, &[0; 36]);
    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    let original_length = file.metadata().unwrap().len();
    let total_length = original_length - 36 + u64::from(binary_length);
    file.set_len(total_length).unwrap();
    file.seek(SeekFrom::Start(8)).unwrap();
    file.write_all(&(total_length as u32).to_le_bytes())
        .unwrap();
    file.seek(SeekFrom::Start(original_length - 44)).unwrap();
    file.write_all(&binary_length.to_le_bytes()).unwrap();
    drop(file);
    assert!(guard_preview(&path).is_ok());
}

#[test]
fn malformed_unicode_data_uri_is_an_error_not_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model.glb");
    let mut document = triangle();
    // Place a multibyte character across the bounded header's byte boundary.
    let encoded = format!("{}éAAAA", "A".repeat((1024 * 1024 / 3) * 4 - 1));
    document["images"] = json!([{"uri":format!("data:image/png;base64,{encoded}")}]);
    write_glb(&path, document, &[0; 36]);
    assert!(guard_preview(&path).is_err());
}
