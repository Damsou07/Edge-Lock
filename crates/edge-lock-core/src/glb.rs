//! Bounded, offline-only GLB inspection. Binary buffers are streamed, never loaded wholesale.
use crate::{CoreError, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

pub const MAX_PREVIEW_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_TEXTURE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_JSON_BYTES: u64 = 16 * 1024 * 1024;
const MAX_IMAGE_HEADER_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GeometryStats {
    pub mesh_objects: u64,
    pub vertices: u64,
    pub faces: u64,
    pub triangles: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlbStats {
    pub geometry: GeometryStats,
    pub animations: Vec<String>,
    pub bytes: u64,
}

#[derive(Clone, Copy)]
struct View {
    offset: u64,
    length: u64,
    stride: Option<u64>,
}
struct Accessor {
    count: u64,
    element_bytes: u64,
    component: u64,
    kind: String,
}
struct Document {
    json: Value,
    bytes: u64,
    bin_offset: u64,
    views: Vec<View>,
    accessors: Vec<Accessor>,
}

fn invalid(message: impl Into<String>) -> CoreError {
    CoreError::Invalid(message.into())
}
fn array<'a>(value: &'a Value, name: &str) -> Result<&'a [Value]> {
    match value.get(name) {
        None => Ok(&[]),
        Some(Value::Array(items)) if items.len() <= 200_000 => Ok(items),
        _ => Err(invalid(format!(
            "GLB : tableau {name} invalide ou trop grand"
        ))),
    }
}
fn number(value: &Value, name: &str) -> Result<u64> {
    value
        .get(name)
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid(format!("GLB : {name} doit être un entier positif")))
}
fn optional_number(value: &Value, name: &str, default: u64) -> Result<u64> {
    if value.get(name).is_none() {
        Ok(default)
    } else {
        number(value, name)
    }
}
fn index(value: &Value, limit: usize, label: &str) -> Result<usize> {
    value
        .as_u64()
        .filter(|v| *v < limit as u64)
        .map(|v| v as usize)
        .ok_or_else(|| invalid(format!("GLB : référence {label} invalide")))
}
fn checked_add(a: u64, b: u64) -> Result<u64> {
    a.checked_add(b)
        .ok_or_else(|| invalid("GLB : dépassement de capacité"))
}
fn checked_mul(a: u64, b: u64) -> Result<u64> {
    a.checked_mul(b)
        .ok_or_else(|| invalid("GLB : dépassement de capacité"))
}
fn read_u32(file: &mut File) -> Result<u32> {
    let mut bytes = [0; 4];
    file.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}
fn view_range(view: View, offset: u64, count: u64, size: u64, stride: u64) -> Result<()> {
    if stride < size {
        return Err(invalid("GLB : stride inférieur à la taille d'un élément"));
    }
    let occupied = if count == 0 {
        0
    } else {
        checked_add(checked_mul(count - 1, stride)?, size)?
    };
    if checked_add(offset, occupied)? > view.length {
        return Err(invalid("GLB : accessor hors du buffer"));
    }
    Ok(())
}

fn read_document(path: &Path) -> Result<Document> {
    let mut file = File::open(path)?;
    let bytes = file.metadata()?.len();
    if bytes < 20 || bytes > u32::MAX as u64 {
        return Err(invalid("GLB : taille invalide"));
    }
    if read_u32(&mut file)? != 0x46546c67 || read_u32(&mut file)? != 2 {
        return Err(invalid("Format attendu : GLB 2.0"));
    }
    if read_u32(&mut file)? as u64 != bytes {
        return Err(invalid("GLB tronqué ou taille d'en-tête incohérente"));
    }
    let json_length = read_u32(&mut file)? as u64;
    if read_u32(&mut file)? != 0x4e4f534a
        || json_length == 0
        || json_length > MAX_JSON_BYTES
        || !json_length.is_multiple_of(4)
        || checked_add(20, json_length)? > bytes
    {
        return Err(invalid("GLB : bloc JSON invalide ou supérieur à 16 Mio"));
    }
    let mut json_bytes = vec![0; json_length as usize];
    file.read_exact(&mut json_bytes)?;
    let json: Value = serde_json::from_slice(&json_bytes)?;
    if json.pointer("/asset/version").and_then(Value::as_str) != Some("2.0") {
        return Err(invalid("GLB : version glTF non prise en charge"));
    }
    if let Some(minimum) = json.pointer("/asset/minVersion").and_then(Value::as_str)
        && minimum != "2.0"
    {
        return Err(invalid("GLB : version minimale glTF non prise en charge"));
    }
    let mut offset = 20 + json_length;
    let mut bin_offset = 0;
    let mut bin_length = 0;
    while offset < bytes {
        if bytes - offset < 8 {
            return Err(invalid("GLB : bloc tronqué"));
        }
        let length = read_u32(&mut file)? as u64;
        let kind = read_u32(&mut file)?;
        let end = checked_add(checked_add(offset, 8)?, length)?;
        if !length.is_multiple_of(4) || end > bytes {
            return Err(invalid("GLB : bloc hors limites"));
        }
        if kind == 0x004e4942 {
            if bin_offset != 0 || offset != 20 + json_length {
                return Err(invalid("GLB : bloc binaire dupliqué ou mal placé"));
            }
            bin_offset = offset + 8;
            bin_length = length;
        } else if kind == 0x4e4f534a {
            return Err(invalid("GLB : bloc JSON dupliqué"));
        }
        file.seek(SeekFrom::Start(end))?;
        offset = end;
    }
    let buffers = array(&json, "buffers")?;
    if buffers.len() > 1 {
        return Err(invalid("GLB : un seul buffer incorporé est pris en charge"));
    }
    let buffer_length = if let Some(buffer) = buffers.first() {
        if buffer.get("uri").is_some() {
            return Err(invalid("GLB : buffers externes interdits"));
        }
        let length = number(buffer, "byteLength")?;
        if length > bin_length || bin_length - length > 3 {
            return Err(invalid("GLB : longueur du buffer incohérente"));
        }
        length
    } else {
        if bin_length != 0 {
            return Err(invalid("GLB : buffer binaire sans déclaration"));
        }
        0
    };
    let mut views = Vec::new();
    for view in array(&json, "bufferViews")? {
        if number(view, "buffer")? != 0 || buffers.is_empty() {
            return Err(invalid("GLB : référence buffer invalide"));
        }
        let offset = optional_number(view, "byteOffset", 0)?;
        let length = number(view, "byteLength")?;
        if length == 0 || checked_add(offset, length)? > buffer_length {
            return Err(invalid("GLB : bufferView hors limites"));
        }
        let stride = view
            .get("byteStride")
            .map(|_| number(view, "byteStride"))
            .transpose()?;
        if stride.is_some_and(|v| !(4..=252).contains(&v) || !v.is_multiple_of(4)) {
            return Err(invalid("GLB : byteStride invalide"));
        }
        views.push(View {
            offset,
            length,
            stride,
        });
    }
    let mut accessors = Vec::new();
    for accessor in array(&json, "accessors")? {
        let component = number(accessor, "componentType")?;
        let component_bytes = match component {
            5120 | 5121 => 1,
            5122 | 5123 => 2,
            5125 | 5126 => 4,
            _ => return Err(invalid("GLB : composant inconnu")),
        };
        let kind = accessor
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("GLB : type accessor manquant"))?;
        let element_bytes = match kind {
            "SCALAR" => component_bytes,
            "VEC2" => 2 * component_bytes,
            "VEC3" => 3 * component_bytes,
            "VEC4" => 4 * component_bytes,
            "MAT2" => (2_u64 * component_bytes).div_ceil(4) * 4 * 2,
            "MAT3" => (3_u64 * component_bytes).div_ceil(4) * 4 * 3,
            "MAT4" => 16 * component_bytes,
            _ => return Err(invalid("GLB : type accessor inconnu")),
        };
        let count = number(accessor, "count")?;
        if count == 0 || count > 100_000_000 {
            return Err(invalid("GLB : nombre d'éléments excessif"));
        }
        let offset = optional_number(accessor, "byteOffset", 0)?;
        if !offset.is_multiple_of(component_bytes) {
            return Err(invalid("GLB : alignement accessor invalide"));
        }
        if let Some(v) = accessor.get("bufferView") {
            let view = views[index(v, views.len(), "accessor.bufferView")?];
            view_range(
                view,
                offset,
                count,
                element_bytes,
                view.stride.unwrap_or(element_bytes),
            )?;
        } else if offset != 0 {
            return Err(invalid("GLB : offset sans bufferView"));
        }
        if let Some(sparse) = accessor.get("sparse") {
            let sparse_count = number(sparse, "count")?;
            if sparse_count == 0 || sparse_count > count {
                return Err(invalid("GLB : sparse accessor invalide"));
            }
            for field in ["indices", "values"] {
                let data = sparse
                    .get(field)
                    .ok_or_else(|| invalid("GLB : données sparse absentes"))?;
                let view = views[index(&data["bufferView"], views.len(), "sparse.bufferView")?];
                let size = if field == "indices" {
                    match number(data, "componentType")? {
                        5121 => 1,
                        5123 => 2,
                        5125 => 4,
                        _ => return Err(invalid("GLB : indices sparse invalides")),
                    }
                } else {
                    element_bytes
                };
                view_range(
                    view,
                    optional_number(data, "byteOffset", 0)?,
                    sparse_count,
                    size,
                    size,
                )?;
            }
        }
        accessors.push(Accessor {
            count,
            element_bytes,
            component,
            kind: kind.to_owned(),
        });
    }
    // Image URIs are the most common route by which a viewer accidentally accesses the network.
    for image in array(&json, "images")? {
        if let Some(uri) = image.get("uri") {
            let uri = uri
                .as_str()
                .ok_or_else(|| invalid("GLB : URI image invalide"))?;
            if ![
                "data:image/png;base64,",
                "data:image/jpeg;base64,",
                "data:image/webp;base64,",
            ]
            .iter()
            .any(|prefix| uri.starts_with(prefix))
            {
                return Err(invalid(
                    "GLB : images externes ou format URI non pris en charge",
                ));
            }
            if image.get("bufferView").is_some() {
                return Err(invalid("GLB : image ambiguë"));
            }
        } else {
            index(&image["bufferView"], views.len(), "image.bufferView")?;
            if image.get("mimeType").and_then(Value::as_str).is_none() {
                return Err(invalid("GLB : MIME image manquant"));
            }
        }
    }
    Ok(Document {
        json,
        bytes,
        bin_offset,
        views,
        accessors,
    })
}

fn statistics(document: &Document) -> Result<GlbStats> {
    let meshes = array(&document.json, "meshes")?;
    let mut counts = Vec::new();
    for mesh in meshes {
        let mut vertices = 0;
        let mut triangles = 0;
        let primitives = array(mesh, "primitives")?;
        if primitives.is_empty() {
            return Err(invalid("GLB : mesh vide"));
        }
        for primitive in primitives {
            let attributes = primitive
                .get("attributes")
                .and_then(Value::as_object)
                .ok_or_else(|| invalid("GLB : attributs absents"))?;
            let position = &document.accessors[index(
                attributes.get("POSITION").unwrap_or(&Value::Null),
                document.accessors.len(),
                "POSITION",
            )?];
            if position.kind != "VEC3" {
                return Err(invalid("GLB : POSITION doit être VEC3"));
            }
            for attribute in attributes.values() {
                let accessor =
                    &document.accessors[index(attribute, document.accessors.len(), "attribut")?];
                if accessor.count != position.count {
                    return Err(invalid("GLB : attributs de longueurs différentes"));
                }
            }
            for target in array(primitive, "targets")? {
                let target = target
                    .as_object()
                    .ok_or_else(|| invalid("GLB : morph target invalide"))?;
                for value in target.values() {
                    index(value, document.accessors.len(), "morph target")?;
                }
            }
            let count = if let Some(indices) = primitive.get("indices") {
                let accessor =
                    &document.accessors[index(indices, document.accessors.len(), "indices")?];
                if accessor.kind != "SCALAR" || ![5121, 5123, 5125].contains(&accessor.component) {
                    return Err(invalid("GLB : indices invalides"));
                }
                accessor.count
            } else {
                position.count
            };
            let mode = optional_number(primitive, "mode", 4)?;
            let primitive_triangles = match mode {
                0..=3 => 0,
                4 => count / 3,
                5 | 6 => count.saturating_sub(2),
                _ => return Err(invalid("GLB : mode géométrique inconnu")),
            };
            vertices = checked_add(vertices, position.count)?;
            triangles = checked_add(triangles, primitive_triangles)?;
        }
        counts.push((vertices, triangles));
    }
    let nodes = array(&document.json, "nodes")?;
    let mut parents = vec![0u8; nodes.len()];
    for node in nodes {
        if let Some(mesh) = node.get("mesh") {
            index(mesh, meshes.len(), "node.mesh")?;
        }
        for child in array(node, "children")? {
            let child = index(child, nodes.len(), "enfant")?;
            parents[child] = parents[child]
                .checked_add(1)
                .ok_or_else(|| invalid("GLB : graphe invalide"))?;
            if parents[child] > 1 {
                return Err(invalid("GLB : un nœud possède plusieurs parents"));
            }
        }
    }
    // A topological traversal also detects cycles outside the selected scene.
    let mut remaining = parents.clone();
    let mut queue: Vec<usize> = remaining
        .iter()
        .enumerate()
        .filter_map(|(i, p)| (*p == 0).then_some(i))
        .collect();
    let mut visited = 0;
    while visited < queue.len() {
        for child in array(&nodes[queue[visited]], "children")? {
            let child = index(child, nodes.len(), "enfant")?;
            remaining[child] -= 1;
            if remaining[child] == 0 {
                queue.push(child);
            }
        }
        visited += 1;
    }
    if visited != nodes.len() {
        return Err(invalid("GLB : cycle dans la hiérarchie"));
    }
    let scenes = array(&document.json, "scenes")?;
    for scene in scenes {
        for root in array(scene, "nodes")? {
            index(root, nodes.len(), "scene.nodes")?;
        }
    }
    let scene_index = optional_number(&document.json, "scene", 0)? as usize;
    let mut stack = if scenes.is_empty() {
        parents
            .iter()
            .enumerate()
            .filter_map(|(i, p)| (*p == 0).then_some(i))
            .collect::<Vec<_>>()
    } else {
        let scene = scenes
            .get(scene_index)
            .ok_or_else(|| invalid("GLB : scène par défaut absente"))?;
        array(scene, "nodes")?
            .iter()
            .map(|v| index(v, nodes.len(), "scene.nodes"))
            .collect::<Result<Vec<_>>>()?
    };
    let mut geometry = GeometryStats::default();
    let mut seen = vec![false; nodes.len()];
    while let Some(i) = stack.pop() {
        if seen[i] {
            return Err(invalid("GLB : nœud répété dans la scène"));
        }
        seen[i] = true;
        if let Some(mesh) = nodes[i].get("mesh") {
            let (vertices, triangles) = counts[index(mesh, counts.len(), "mesh")?];
            geometry.mesh_objects = checked_add(geometry.mesh_objects, 1)?;
            geometry.vertices = checked_add(geometry.vertices, vertices)?;
            geometry.triangles = checked_add(geometry.triangles, triangles)?;
        }
        for child in array(&nodes[i], "children")? {
            stack.push(index(child, nodes.len(), "enfant")?);
        }
    }
    geometry.faces = geometry.triangles;
    for skin in array(&document.json, "skins")? {
        for joint in array(skin, "joints")? {
            index(joint, nodes.len(), "skin.joints")?;
        }
        if let Some(bind) = skin.get("inverseBindMatrices") {
            index(bind, document.accessors.len(), "inverseBindMatrices")?;
        }
    }
    let mut animations = Vec::new();
    for (i, animation) in array(&document.json, "animations")?.iter().enumerate() {
        let samplers = array(animation, "samplers")?;
        for sampler in samplers {
            index(
                &sampler["input"],
                document.accessors.len(),
                "animation.input",
            )?;
            index(
                &sampler["output"],
                document.accessors.len(),
                "animation.output",
            )?;
        }
        for channel in array(animation, "channels")? {
            index(&channel["sampler"], samplers.len(), "animation.sampler")?;
            if let Some(node) = channel.pointer("/target/node") {
                index(node, nodes.len(), "animation.node")?;
            }
        }
        animations.push(
            animation
                .get("name")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("Animation {}", i + 1)),
        );
    }
    Ok(GlbStats {
        geometry,
        animations,
        bytes: document.bytes,
    })
}

/// Inspect a self-contained GLB without allocating its binary buffers or decoding textures.
pub fn inspect_glb(path: &Path) -> Result<GlbStats> {
    statistics(&read_document(path)?)
}

/// Additional resource limits for the embedded viewer. Export itself has no preview size cap.
pub fn guard_preview(path: &Path) -> Result<GlbStats> {
    if std::fs::metadata(path)?.len() > MAX_PREVIEW_BYTES {
        return Err(invalid(
            "Aperçu limité à 256 Mio ; le fichier reste exportable",
        ));
    }
    let document = read_document(path)?;
    let stats = statistics(&document)?;
    let expanded = document.accessors.iter().try_fold(0, |sum, a| {
        checked_add(sum, checked_mul(a.count, a.element_bytes)?)
    })?;
    if expanded > MAX_TEXTURE_BYTES {
        return Err(invalid("Géométrie trop volumineuse pour l'aperçu"));
    }
    let mut file = File::open(path)?;
    let mut texture_bytes = 0;
    for image in array(&document.json, "images")? {
        let bytes = if let Some(uri) = image.get("uri").and_then(Value::as_str) {
            let (_, encoded) = uri
                .split_once(',')
                .ok_or_else(|| invalid("URI image invalide"))?;
            let header_length = encoded.len().min((MAX_IMAGE_HEADER_BYTES as usize / 3) * 4);
            base64::engine::general_purpose::STANDARD
                .decode(&encoded.as_bytes()[..header_length])
                .map_err(|_| invalid("GLB : image base64 invalide"))?
        } else {
            let view = document.views[index(&image["bufferView"], document.views.len(), "image")?];
            file.seek(SeekFrom::Start(checked_add(
                document.bin_offset,
                view.offset,
            )?))?;
            let mut bytes = vec![0; view.length.min(MAX_IMAGE_HEADER_BYTES) as usize];
            file.read_exact(&mut bytes)?;
            bytes
        };
        let (width, height) = image_dimensions(&bytes)?;
        if width == 0 || height == 0 {
            return Err(invalid("GLB : dimensions d'image nulles"));
        }
        texture_bytes = checked_add(texture_bytes, checked_mul(checked_mul(width, height)?, 4)?)?;
        if texture_bytes > MAX_TEXTURE_BYTES {
            return Err(invalid("Aperçu limité à 512 Mio de textures décompressées"));
        }
    }
    Ok(stats)
}

fn image_dimensions(bytes: &[u8]) -> Result<(u64, u64)> {
    if bytes.len() >= 24 && &bytes[..8] == b"\x89PNG\r\n\x1a\n" && &bytes[12..16] == b"IHDR" {
        return Ok((
            u32::from_be_bytes(bytes[16..20].try_into().unwrap()) as u64,
            u32::from_be_bytes(bytes[20..24].try_into().unwrap()) as u64,
        ));
    }
    if bytes.len() >= 30 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        let read24 =
            |i: usize| bytes[i] as u64 | (bytes[i + 1] as u64) << 8 | (bytes[i + 2] as u64) << 16;
        if &bytes[12..16] == b"VP8X" {
            return Ok((read24(24) + 1, read24(27) + 1));
        }
        if &bytes[12..16] == b"VP8L" && bytes[20] == 0x2f {
            let bits = u32::from_le_bytes(bytes[21..25].try_into().unwrap());
            return Ok((
                (bits & 0x3fff) as u64 + 1,
                ((bits >> 14) & 0x3fff) as u64 + 1,
            ));
        }
        if &bytes[12..16] == b"VP8 " && &bytes[23..26] == b"\x9d\x01\x2a" {
            return Ok((
                (u16::from_le_bytes(bytes[26..28].try_into().unwrap()) & 0x3fff) as u64,
                (u16::from_le_bytes(bytes[28..30].try_into().unwrap()) & 0x3fff) as u64,
            ));
        }
    }
    if bytes.starts_with(b"\xff\xd8") {
        let mut offset = 2;
        while offset + 4 <= bytes.len() {
            if bytes[offset] != 0xff {
                break;
            }
            while offset < bytes.len() && bytes[offset] == 0xff {
                offset += 1;
            }
            if offset >= bytes.len() {
                break;
            }
            let marker = bytes[offset];
            offset += 1;
            if marker == 0xda || marker == 0xd9 {
                break;
            }
            if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
                continue;
            }
            if offset + 2 > bytes.len() {
                break;
            }
            let length = u16::from_be_bytes(bytes[offset..offset + 2].try_into().unwrap()) as usize;
            if length < 2 || offset + length > bytes.len() {
                break;
            }
            if [
                0xc0, 0xc1, 0xc2, 0xc3, 0xc5, 0xc6, 0xc7, 0xc9, 0xca, 0xcb, 0xcd, 0xce, 0xcf,
            ]
            .contains(&marker)
                && length >= 7
            {
                return Ok((
                    u16::from_be_bytes(bytes[offset + 5..offset + 7].try_into().unwrap()) as u64,
                    u16::from_be_bytes(bytes[offset + 3..offset + 5].try_into().unwrap()) as u64,
                ));
            }
            offset += length;
        }
    }
    Err(invalid(
        "Aperçu : image PNG/JPEG/WebP non reconnue ou en-tête trop grand",
    ))
}
