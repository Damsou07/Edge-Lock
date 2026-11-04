//! Bounded, offline-only GLB inspection. Binary buffers are streamed, never loaded wholesale.
use crate::{CoreError, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs::File,
    io::{BufReader, Read, Seek, SeekFrom},
    path::Path,
};

const MAX_EDGE_WORKING_BYTES: u64 = 64 * 1024 * 1024;
const INDEX_READER_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GeometryStats {
    pub mesh_objects: u64,
    pub vertices: u64,
    #[serde(default)]
    pub edges: Option<u64>,
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

fn validate_image_base64(uri: &str) -> Result<()> {
    let encoded = uri
        .split_once(',')
        .ok_or_else(|| invalid("GLB : URI image invalide"))?
        .1;
    if encoded.is_empty() || !encoded.len().is_multiple_of(4) {
        return Err(invalid("GLB : image base64 invalide"));
    }
    // Validate the complete URI with fixed working memory, independently of image size.
    let mut chunks = encoded.as_bytes().chunks(16 * 1024).peekable();
    let mut decoded = [0; 12 * 1024];
    while let Some(chunk) = chunks.next() {
        if chunks.peek().is_some() && chunk.contains(&b'=') {
            return Err(invalid("GLB : image base64 invalide"));
        }
        base64::engine::general_purpose::STANDARD
            .decode_slice(chunk, &mut decoded)
            .map_err(|_| invalid("GLB : image base64 invalide"))?;
    }
    Ok(())
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
        || !json_length.is_multiple_of(4)
        || checked_add(20, json_length)? > bytes
    {
        return Err(invalid("GLB : bloc JSON invalide"));
    }
    let mut json_bytes = Vec::new();
    json_bytes
        .try_reserve_exact(json_length as usize)
        .map_err(|_| invalid("Mémoire insuffisante pour lire les métadonnées GLB"))?;
    json_bytes.resize(json_length as usize, 0);
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
            validate_image_base64(uri)?;
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

/// Validate structure without reading indices or computing optional topology statistics.
pub fn validate_glb(path: &Path) -> Result<()> {
    statistics(&read_document(path)?)?;
    Ok(())
}

/// Inspect a self-contained GLB, streaming indices within a bounded topology budget.
pub fn inspect_glb(path: &Path) -> Result<GlbStats> {
    let document = read_document(path)?;
    let mut stats = statistics(&document)?;
    stats.geometry.edges = count_scene_edges(path, &document)?;
    Ok(stats)
}

/// A scalar stream retains only a small read buffer, including interleaved accessors.
struct ScalarReader {
    file: BufReader<File>,
    component_bytes: usize,
    stride: u64,
    started: bool,
}

impl ScalarReader {
    fn new(
        path: &Path,
        document: &Document,
        data: &Value,
        component: u64,
        interleaved: bool,
    ) -> Result<Self> {
        let view = document.views[index(
            &data["bufferView"],
            document.views.len(),
            "indices.bufferView",
        )?];
        let component_bytes = match component {
            5121 => 1,
            5123 => 2,
            5125 => 4,
            _ => return Err(invalid("GLB : composant d'indice invalide")),
        };
        let offset = checked_add(
            checked_add(document.bin_offset, view.offset)?,
            optional_number(data, "byteOffset", 0)?,
        )?;
        let mut file = BufReader::with_capacity(INDEX_READER_BYTES, File::open(path)?);
        file.seek(SeekFrom::Start(offset))?;
        Ok(Self {
            file,
            component_bytes,
            stride: if interleaved {
                view.stride.unwrap_or(component_bytes as u64)
            } else {
                component_bytes as u64
            },
            started: false,
        })
    }

    fn next(&mut self) -> Result<u32> {
        if self.started {
            self.file
                .seek_relative((self.stride - self.component_bytes as u64) as i64)?;
        }
        self.started = true;
        let mut bytes = [0; 4];
        self.file.read_exact(&mut bytes[..self.component_bytes])?;
        Ok(u32::from_le_bytes(bytes))
    }
}

struct SparseIndices {
    indices: ScalarReader,
    values: ScalarReader,
    remaining: u64,
    current: Option<(u64, u32)>,
    previous: Option<u64>,
    count: u64,
}

impl SparseIndices {
    fn advance(&mut self) -> Result<()> {
        self.current = if self.remaining == 0 {
            None
        } else {
            let index = self.indices.next()? as u64;
            if index >= self.count || self.previous.is_some_and(|previous| index <= previous) {
                return Err(invalid(
                    "GLB : indices sparse hors limites ou non croissants",
                ));
            }
            self.previous = Some(index);
            self.remaining -= 1;
            Some((index, self.values.next()?))
        };
        Ok(())
    }
}

struct IndexReader {
    base: Option<ScalarReader>,
    sparse: Option<SparseIndices>,
    ordinal: u64,
    indexed: bool,
    vertex_count: u64,
}

impl IndexReader {
    fn new(path: &Path, document: &Document, primitive: &Value, vertex_count: u64) -> Result<Self> {
        let mut reader = Self {
            base: None,
            sparse: None,
            ordinal: 0,
            indexed: false,
            vertex_count,
        };
        let Some(accessor_index) = primitive.get("indices") else {
            return Ok(reader);
        };
        reader.indexed = true;
        let accessor = &array(&document.json, "accessors")?
            [index(accessor_index, document.accessors.len(), "indices")?];
        let component = number(accessor, "componentType")?;
        if accessor.get("bufferView").is_some() {
            reader.base = Some(ScalarReader::new(
                path, document, accessor, component, true,
            )?);
        }
        if let Some(sparse) = accessor.get("sparse") {
            let mut overrides = SparseIndices {
                indices: ScalarReader::new(
                    path,
                    document,
                    &sparse["indices"],
                    number(&sparse["indices"], "componentType")?,
                    false,
                )?,
                values: ScalarReader::new(path, document, &sparse["values"], component, false)?,
                remaining: number(sparse, "count")?,
                current: None,
                previous: None,
                count: number(accessor, "count")?,
            };
            overrides.advance()?;
            reader.sparse = Some(overrides);
        }
        Ok(reader)
    }

    fn next(&mut self) -> Result<u32> {
        let mut value = if !self.indexed {
            self.ordinal as u32
        } else if let Some(base) = self.base.as_mut() {
            base.next()?
        } else {
            0
        };
        if let Some(sparse) = self.sparse.as_mut()
            && let Some((ordinal, replacement)) = sparse.current
            && ordinal == self.ordinal
        {
            value = replacement;
            sparse.advance()?;
        }
        self.ordinal += 1;
        if value as u64 >= self.vertex_count {
            return Err(invalid("GLB : indice de sommet hors limites"));
        }
        Ok(value)
    }
}

fn edge_upper_bound(mode: u64, count: u64) -> u64 {
    match mode {
        0 => 0,
        1 => count / 2,
        2 if count > 1 => count,
        2 | 3 => count.saturating_sub(1),
        4 => count / 3 * 3,
        5 | 6 => count.saturating_sub(2) * 3,
        _ => 0, // Modes have already been validated by statistics.
    }
}

fn count_mesh_edges(
    path: &Path,
    document: &Document,
    mesh: &Value,
    working_budget: u64,
) -> Result<Option<u64>> {
    let primitives = array(mesh, "primitives")?;
    let mut upper_bound = 0;
    for primitive in primitives {
        let position = index(
            &primitive["attributes"]["POSITION"],
            document.accessors.len(),
            "POSITION",
        )?;
        let count = if let Some(indices) = primitive.get("indices") {
            document.accessors[index(indices, document.accessors.len(), "indices")?].count
        } else {
            document.accessors[position].count
        };
        upper_bound = checked_add(
            upper_bound,
            edge_upper_bound(optional_number(primitive, "mode", 4)?, count),
        )?;
    }
    // Three readers (base, sparse offsets, sparse values) coexist with the edge buffer.
    let available = working_budget.saturating_sub(3 * INDEX_READER_BYTES as u64);
    if upper_bound > available / std::mem::size_of::<[u32; 3]>() as u64 {
        return Ok(None);
    }
    let mut edges = Vec::<[u32; 3]>::new();
    if edges.try_reserve_exact(upper_bound as usize).is_err() {
        return Ok(None);
    }
    for primitive in primitives {
        let position = index(
            &primitive["attributes"]["POSITION"],
            document.accessors.len(),
            "POSITION",
        )?;
        let vertices = document.accessors[position].count;
        let count = if let Some(indices) = primitive.get("indices") {
            document.accessors[index(indices, document.accessors.len(), "indices")?].count
        } else {
            vertices
        };
        let mode = optional_number(primitive, "mode", 4)?;
        if mode == 0 {
            continue;
        }
        let mut reader = IndexReader::new(path, document, primitive, vertices)?;
        let mut add = |a: u32, b: u32| {
            if a != b {
                edges.push([position as u32, a.min(b), a.max(b)]);
            }
        };
        match mode {
            1 => {
                for _ in 0..count / 2 {
                    add(reader.next()?, reader.next()?);
                }
            }
            2 | 3 if count > 0 => {
                let first = reader.next()?;
                let mut previous = first;
                for _ in 1..count {
                    let next = reader.next()?;
                    add(previous, next);
                    previous = next;
                }
                if mode == 2 && count > 1 {
                    add(previous, first);
                }
            }
            4 => {
                for _ in 0..count / 3 {
                    let (a, b, c) = (reader.next()?, reader.next()?, reader.next()?);
                    add(a, b);
                    add(b, c);
                    add(c, a);
                }
            }
            5 | 6 if count >= 3 => {
                let mut a = reader.next()?;
                let mut b = reader.next()?;
                for _ in 2..count {
                    let c = reader.next()?;
                    add(a, b);
                    add(b, c);
                    add(c, a);
                    if mode == 5 {
                        a = b;
                    }
                    b = c;
                }
            }
            _ => {}
        }
    }
    edges.sort_unstable();
    edges.dedup();
    Ok(Some(edges.len() as u64))
}

fn count_scene_edges(path: &Path, document: &Document) -> Result<Option<u64>> {
    let meshes = array(&document.json, "meshes")?;
    let nodes = array(&document.json, "nodes")?;
    let scenes = array(&document.json, "scenes")?;
    let mut stack = if scenes.is_empty() {
        let mut children = vec![false; nodes.len()];
        for node in nodes {
            for child in array(node, "children")? {
                children[index(child, nodes.len(), "enfant")?] = true;
            }
        }
        children
            .iter()
            .enumerate()
            .filter_map(|(i, child)| (!child).then_some(i))
            .collect::<Vec<_>>()
    } else {
        let scene = &scenes[optional_number(&document.json, "scene", 0)? as usize];
        array(scene, "nodes")?
            .iter()
            .map(|node| index(node, nodes.len(), "scene.nodes"))
            .collect::<Result<Vec<_>>>()?
    };
    let mut instances = vec![0u64; meshes.len()];
    while let Some(node) = stack.pop() {
        if let Some(mesh) = nodes[node].get("mesh") {
            let i = index(mesh, meshes.len(), "mesh")?;
            instances[i] += 1;
        }
        for child in array(&nodes[node], "children")? {
            stack.push(index(child, nodes.len(), "enfant")?);
        }
    }
    drop(stack);
    let working_budget =
        MAX_EDGE_WORKING_BYTES - (instances.len() * std::mem::size_of::<u64>()) as u64;
    let mut total = 0;
    for (mesh, instances) in meshes.iter().zip(instances) {
        if instances == 0 {
            continue;
        }
        let Some(count) = count_mesh_edges(path, document, mesh, working_budget)? else {
            return Ok(None);
        };
        total = checked_add(total, checked_mul(count, instances)?)?;
    }
    Ok(Some(total))
}

/// Validate viewer capabilities without imposing file, geometry or decoded texture quotas.
pub fn guard_preview(path: &Path) -> Result<GlbStats> {
    let document = read_document(path)?;
    for image in array(&document.json, "images")? {
        if let Some(mime) = image.get("mimeType")
            && !matches!(
                mime.as_str(),
                Some("image/png" | "image/jpeg" | "image/webp")
            )
        {
            return Err(invalid(
                "GLB : format d’image non pris en charge par l’aperçu",
            ));
        }
    }
    for extension in array(&document.json, "extensionsRequired")? {
        let name = extension
            .as_str()
            .ok_or_else(|| invalid("Extension GLB invalide"))?;
        if [
            "KHR_draco_mesh_compression",
            "EXT_meshopt_compression",
            "KHR_meshopt_compression",
            "KHR_texture_basisu",
        ]
        .contains(&name)
        {
            return Err(invalid(format!(
                "Compression {name} non prise en charge par l’aperçu"
            )));
        }
    }
    statistics(&document)
}
