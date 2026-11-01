use edge_lock_core::glb::{guard_preview, inspect_glb};
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
    assert_eq!(stats.animations, ["Idle"]);
    assert!(guard_preview(&path).is_ok());
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
fn refuses_texture_bomb_before_decoding_image() {
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
        guard_preview(&path).is_err(),
        "preview must refuse decoded texture memory >512MiB"
    );
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
