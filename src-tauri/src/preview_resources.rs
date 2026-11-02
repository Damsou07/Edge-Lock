//! Bounded local resource handles and disposable, content-addressed library thumbnails.
use crate::state::{AppError, AppResult, AppState, lock};
use edge_lock_core::Library;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

pub const MAX_THUMBNAIL_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_RESOURCES: usize = 128;
const RENDERER_VERSION: u32 = 1;

#[derive(Clone)]
pub struct ThumbnailSource {
    pub variant_id: String,
    pub artifact_key: String,
}

#[derive(Clone)]
pub struct PreviewResource {
    pub path: PathBuf,
    pub thumbnail: Option<ThumbnailSource>,
    is_thumbnail: bool,
}

impl PreviewResource {
    pub fn glb(path: PathBuf, thumbnail: Option<ThumbnailSource>) -> Self {
        Self {
            path,
            thumbnail,
            is_thumbnail: false,
        }
    }

    pub fn png(path: PathBuf, thumbnail: ThumbnailSource) -> Self {
        Self {
            path,
            thumbnail: Some(thumbnail),
            is_thumbnail: true,
        }
    }

    pub fn mime(&self) -> &'static str {
        if self.is_thumbnail {
            "image/png"
        } else {
            "model/gltf-binary"
        }
    }

    pub fn read(&self) -> AppResult<Vec<u8>> {
        let limit = if self.is_thumbnail {
            MAX_THUMBNAIL_BYTES as u64
        } else {
            edge_lock_core::glb::MAX_PREVIEW_BYTES
        };
        read_bounded(&self.path, limit)
    }

    pub fn is_thumbnail(&self) -> bool {
        self.is_thumbnail
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceUrl {
    pub url: String,
    pub resource_token: String,
}

#[derive(Default)]
pub struct PreviewResources {
    entries: HashMap<String, PreviewResource>,
}

impl PreviewResources {
    pub fn register(&mut self, resource: PreviewResource) -> AppResult<ResourceUrl> {
        if self.entries.len() >= MAX_RESOURCES {
            return Err(AppError::new(
                "busy",
                "Trop d’aperçus actifs. Fermez un aperçu avant de réessayer.",
            ));
        }
        let token = uuid::Uuid::new_v4().simple().to_string();
        self.entries.insert(token.clone(), resource);
        Ok(ResourceUrl {
            url: format!("http://edge-preview.localhost/{token}"),
            resource_token: token,
        })
    }

    pub fn get(&self, token: &str) -> Option<PreviewResource> {
        self.entries.get(token).cloned()
    }

    pub fn release(&mut self, tokens: &[String]) -> AppResult<()> {
        if tokens.len() > MAX_RESOURCES {
            return Err(AppError::new("invalid", "Trop de ressources à libérer."));
        }
        for token in tokens {
            self.entries.remove(token);
        }
        Ok(())
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
    pub fn paths(&self) -> Vec<PathBuf> {
        self.entries
            .values()
            .map(|entry| entry.path.clone())
            .collect()
    }
    pub fn remove_path(&mut self, path: &Path) {
        self.entries.retain(|_, resource| resource.path != path);
    }
    pub fn available(&self) -> usize {
        MAX_RESOURCES - self.entries.len()
    }
}

/// Lock the library and registry together so a library switch cannot publish an old handle.
pub fn register(
    state: &AppState,
    library: &Arc<Library>,
    resource: PreviewResource,
) -> AppResult<ResourceUrl> {
    let current = lock(&state.library)?;
    if !current
        .as_ref()
        .is_some_and(|current| Arc::ptr_eq(current, library))
    {
        return Err(AppError::new(
            "library_changed",
            "La bibliothèque a changé.",
        ));
    }
    lock(&state.previews)?.register(resource)
}

pub fn register_batch(
    state: &AppState,
    library: &Arc<Library>,
    resources: HashMap<String, PreviewResource>,
) -> AppResult<HashMap<String, ResourceUrl>> {
    let current = lock(&state.library)?;
    if !current
        .as_ref()
        .is_some_and(|current| Arc::ptr_eq(current, library))
    {
        return Err(AppError::new(
            "library_changed",
            "La bibliothèque a changé.",
        ));
    }
    let mut registry = lock(&state.previews)?;
    if registry.available() < resources.len() {
        return Err(AppError::new("busy", "Trop d’aperçus actifs."));
    }
    resources
        .into_iter()
        .map(|(id, resource)| Ok((id, registry.register(resource)?)))
        .collect()
}

/// Fast catalog path: validate only cached PNGs, never hash or convert a model.
pub fn cached_asset_resources(
    library: &Library,
    asset_ids: &[String],
) -> AppResult<HashMap<String, PreviewResource>> {
    if asset_ids.len() > 64 {
        return Err(invalid("Demandez au maximum 64 miniatures."));
    }
    let mut resources = HashMap::new();
    for id in asset_ids {
        if resources.contains_key(id) {
            continue;
        }
        let asset = library.get_asset(id)?;
        if let Some(variant) = asset.variants.first()
            && let Some(resource) = cached_thumbnail_resource(library.root(), &variant.id)?
        {
            resources.insert(id.clone(), resource);
        }
    }
    Ok(resources)
}

pub fn store(
    state: &AppState,
    library: &Arc<Library>,
    preview_token: &str,
    png_bytes: &[u8],
) -> AppResult<ResourceUrl> {
    let _gate = lock(&state.operation_gate)?;
    if !lock(&state.library)?
        .as_ref()
        .is_some_and(|current| Arc::ptr_eq(current, library))
    {
        return Err(AppError::new(
            "library_changed",
            "La bibliothèque a changé.",
        ));
    }
    let resource = lock(&state.previews)?
        .get(preview_token)
        .ok_or_else(|| AppError::new("not_found", "L’aperçu a été libéré."))?;
    if resource.is_thumbnail() {
        return Err(invalid("Un aperçu GLB est nécessaire."));
    }
    let source = resource
        .thumbnail
        .ok_or_else(|| invalid("Ce jeton ne permet pas d’enregistrer une miniature."))?;
    let path = save_thumbnail(
        library.root(),
        &source.variant_id,
        &source.artifact_key,
        png_bytes,
    )?;
    let url = register(state, library, PreviewResource::png(path.clone(), source))?;
    crate::jobs::trim_cache(&library.root().join("cache"), &path, state);
    Ok(url)
}

fn invalid(message: &str) -> AppError {
    AppError::new("invalid", message)
}

pub fn is_link(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}

fn ensure_directory(path: &Path) -> AppResult<()> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir() || is_link(&meta) {
        return Err(invalid("Le cache doit contenir des dossiers ordinaires."));
    }
    Ok(())
}

fn cache_directory(root: &Path, create: bool) -> AppResult<Option<PathBuf>> {
    ensure_directory(root)?;
    let mut directory = root.to_path_buf();
    for component in ["cache", "thumbnails"] {
        directory.push(component);
        if !directory.exists() {
            if !create {
                return Ok(None);
            }
            fs::create_dir(&directory)?;
        }
        ensure_directory(&directory)?;
    }
    Ok(Some(directory))
}

fn read_bounded(path: &Path, limit: u64) -> AppResult<Vec<u8>> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || is_link(&meta) || meta.len() > limit {
        return Err(invalid("Ressource d’aperçu invalide ou trop volumineuse."));
    }
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    fs::File::open(path)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(invalid("Ressource d’aperçu trop volumineuse."));
    }
    Ok(bytes)
}

pub fn validate_thumbnail(bytes: &[u8]) -> AppResult<()> {
    if bytes.len() > MAX_THUMBNAIL_BYTES {
        return Err(invalid("Miniature limitée à 2 Mio."));
    }
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_limits(png::Limits {
        bytes: 4 * 1024 * 1024,
    });
    decoder.set_ignore_text_chunk(true);
    decoder.set_ignore_iccp_chunk(true);
    let mut reader = decoder
        .read_info()
        .map_err(|_| invalid("Miniature PNG invalide."))?;
    let info = reader.info();
    if info.width != 512
        || info.height != 384
        || info.bit_depth != png::BitDepth::Eight
        || !matches!(info.color_type, png::ColorType::Rgb | png::ColorType::Rgba)
        || info.animation_control.is_some()
    {
        return Err(invalid(
            "La miniature doit être un PNG statique de 512 × 384 pixels.",
        ));
    }
    let length = reader
        .output_buffer_size()
        .filter(|length| *length <= 512 * 384 * 4)
        .ok_or_else(|| invalid("Miniature PNG trop volumineuse."))?;
    reader
        .next_frame(&mut vec![0; length])
        .map_err(|_| invalid("Miniature PNG endommagée."))?;
    reader
        .finish()
        .map_err(|_| invalid("Miniature PNG incomplète."))?;
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThumbnailRecord {
    version: u32,
    artifact_key: String,
    image_hash: String,
}

fn valid_key(key: &str) -> bool {
    key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit())
}
fn index_path(directory: &Path, variant_id: &str) -> PathBuf {
    directory.join(format!(
        "index-{}.json",
        blake3::hash(variant_id.as_bytes()).to_hex()
    ))
}
fn image_path(directory: &Path, key: &str) -> PathBuf {
    directory.join(format!("{key}-v{RENDERER_VERSION}.png"))
}

fn read_record(path: &Path) -> Option<ThumbnailRecord> {
    serde_json::from_slice(&read_bounded(path, 4096).ok()?).ok()
}

fn atomic_write(directory: &Path, path: &Path, bytes: &[u8]) -> AppResult<()> {
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

pub fn save_thumbnail(
    root: &Path,
    variant_id: &str,
    key: &str,
    bytes: &[u8],
) -> AppResult<PathBuf> {
    if !valid_key(key) {
        return Err(invalid("Clé d’aperçu invalide."));
    }
    validate_thumbnail(bytes)?;
    let directory = cache_directory(root, true)?.expect("created cache directory");
    let path = image_path(&directory, key);
    atomic_write(&directory, &path, bytes)?;
    let record = ThumbnailRecord {
        version: RENDERER_VERSION,
        artifact_key: key.into(),
        image_hash: blake3::hash(bytes).to_hex().to_string(),
    };
    atomic_write(
        &directory,
        &index_path(&directory, variant_id),
        &serde_json::to_vec(&record)?,
    )?;
    Ok(path)
}

pub fn find_thumbnail(
    root: &Path,
    variant_id: &str,
    expected_key: Option<&str>,
) -> AppResult<Option<PathBuf>> {
    let Some(directory) = cache_directory(root, false)? else {
        return Ok(None);
    };
    let Some(record) = read_record(&index_path(&directory, variant_id)) else {
        return Ok(None);
    };
    if record.version != RENDERER_VERSION
        || !valid_key(&record.artifact_key)
        || expected_key.is_some_and(|expected| expected != record.artifact_key)
    {
        return Ok(None);
    }
    let path = image_path(&directory, &record.artifact_key);
    let Ok(bytes) = read_bounded(&path, MAX_THUMBNAIL_BYTES as u64) else {
        return Ok(None);
    };
    if blake3::hash(&bytes).to_hex().as_str() != record.image_hash
        || validate_thumbnail(&bytes).is_err()
    {
        return Ok(None);
    }
    Ok(Some(path))
}

pub fn cached_thumbnail_resource(
    root: &Path,
    variant_id: &str,
) -> AppResult<Option<PreviewResource>> {
    let Some(path) = find_thumbnail(root, variant_id, None)? else {
        return Ok(None);
    };
    let key = path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.get(..64))
        .filter(|key| valid_key(key))
        .ok_or_else(|| invalid("Clé de miniature invalide."))?
        .to_owned();
    Ok(Some(PreviewResource::png(
        path,
        ThumbnailSource {
            variant_id: variant_id.into(),
            artifact_key: key,
        },
    )))
}

pub fn remove_thumbnail(root: &Path, source: &ThumbnailSource) -> AppResult<()> {
    if !valid_key(&source.artifact_key) {
        return Err(invalid("Clé de miniature invalide."));
    }
    let Some(directory) = cache_directory(root, false)? else {
        return Ok(());
    };
    let index = index_path(&directory, &source.variant_id);
    if read_record(&index).is_some_and(|record| record.artifact_key == source.artifact_key) {
        fs::remove_file(index)?;
    }
    let path = image_path(&directory, &source.artifact_key);
    if let Ok(meta) = fs::symlink_metadata(&path) {
        if !meta.is_file() || is_link(&meta) {
            return Err(invalid("Fichier de miniature invalide."));
        }
        fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png() -> Vec<u8> {
        let mut output = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut output, 512, 384);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&vec![128; 512 * 384 * 4]).unwrap();
        }
        output
    }

    #[test]
    fn thumbnail_cache_is_keyed_validated_and_preserves_other_files() {
        let root = tempfile::tempdir().unwrap();
        let key = "a".repeat(64);
        assert!(
            find_thumbnail(root.path(), "variant", None)
                .unwrap()
                .is_none()
        );
        let path = save_thumbnail(root.path(), "variant", &key, &png()).unwrap();
        assert_eq!(
            find_thumbnail(root.path(), "variant", Some(&key)).unwrap(),
            Some(path.clone())
        );
        assert!(
            find_thumbnail(root.path(), "variant", Some(&"b".repeat(64)))
                .unwrap()
                .is_none()
        );
        std::fs::write(root.path().join("keep.txt"), b"keep").unwrap();
        std::fs::write(&path, b"broken").unwrap();
        assert!(
            find_thumbnail(root.path(), "variant", None)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            std::fs::read(root.path().join("keep.txt")).unwrap(),
            b"keep"
        );
    }

    #[test]
    fn png_validation_rejects_truncated_wrong_size_and_excessive_input() {
        let valid = png();
        assert!(validate_thumbnail(&valid).is_ok());
        assert!(validate_thumbnail(&valid[..valid.len() - 12]).is_err());
        let mut corrupted = valid.clone();
        corrupted[50] ^= 0xff;
        assert!(validate_thumbnail(&corrupted).is_err());
        let mut wrong_size = valid.clone();
        wrong_size[16..20].copy_from_slice(&8192u32.to_be_bytes());
        assert!(validate_thumbnail(&wrong_size).is_err());
        assert!(validate_thumbnail(&vec![0; MAX_THUMBNAIL_BYTES + 1]).is_err());
        assert!(
            save_thumbnail(
                tempfile::tempdir().unwrap().path(),
                "variant",
                "../escape",
                &valid
            )
            .is_err()
        );
    }

    #[test]
    fn resources_keep_multiple_tokens_until_release_and_enforce_the_bound() {
        let mut resources = PreviewResources::default();
        let resource = PreviewResource::glb(std::path::PathBuf::from("model.glb"), None);
        let first = resources.register(resource.clone()).unwrap();
        let second = resources.register(resource.clone()).unwrap();
        assert!(resources.get(&first.resource_token).is_some());
        assert!(resources.get(&second.resource_token).is_some());
        resources
            .release(std::slice::from_ref(&first.resource_token))
            .unwrap();
        assert!(resources.get(&first.resource_token).is_none());
        assert!(resources.get(&second.resource_token).is_some());
        for _ in 1..MAX_RESOURCES {
            resources.register(resource.clone()).unwrap();
        }
        assert!(resources.register(resource).is_err());
        assert_eq!(
            resources.get(&second.resource_token).unwrap().mime(),
            "model/gltf-binary"
        );
    }

    #[test]
    fn invalidation_of_an_old_thumbnail_preserves_the_newer_index_and_unrelated_png() {
        let root = tempfile::tempdir().unwrap();
        let first_key = "a".repeat(64);
        let second_key = "b".repeat(64);
        let old = save_thumbnail(root.path(), "variant", &first_key, &png()).unwrap();
        let current = save_thumbnail(root.path(), "variant", &second_key, &png()).unwrap();
        let unrelated =
            save_thumbnail(root.path(), "other-variant", &"c".repeat(64), &png()).unwrap();
        remove_thumbnail(
            root.path(),
            &ThumbnailSource {
                variant_id: "variant".into(),
                artifact_key: first_key,
            },
        )
        .unwrap();
        assert!(!old.exists());
        assert_eq!(
            find_thumbnail(root.path(), "variant", None).unwrap(),
            Some(current)
        );
        assert!(unrelated.is_file());
        let resource = cached_thumbnail_resource(root.path(), "variant")
            .unwrap()
            .unwrap();
        assert_eq!(resource.mime(), "image/png");
        assert_eq!(resource.read().unwrap(), png());
        remove_thumbnail(root.path(), resource.thumbnail.as_ref().unwrap()).unwrap();
        assert!(
            find_thumbnail(root.path(), "variant", None)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn cache_refuses_an_unrelated_file_in_place_of_a_directory() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("cache"), b"keep").unwrap();
        assert!(save_thumbnail(root.path(), "variant", &"a".repeat(64), &png()).is_err());
        assert_eq!(fs::read(root.path().join("cache")).unwrap(), b"keep");
    }

    #[test]
    fn store_requires_an_authorized_glb_and_rejects_a_switched_library() {
        let root = tempfile::tempdir().unwrap();
        let library = Arc::new(Library::open(root.path().join("one")).unwrap());
        let state = crate::state::test_state(library.clone());
        let path = library.root().join("model.glb");
        let ordinary =
            register(&state, &library, PreviewResource::glb(path.clone(), None)).unwrap();
        assert_eq!(
            store(&state, &library, &ordinary.resource_token, &png())
                .unwrap_err()
                .code,
            "invalid"
        );
        let source = ThumbnailSource {
            variant_id: "variant".into(),
            artifact_key: "a".repeat(64),
        };
        let preview = register(&state, &library, PreviewResource::glb(path, Some(source))).unwrap();
        let stored = store(&state, &library, &preview.resource_token, &png()).unwrap();
        assert_eq!(
            lock(&state.previews)
                .unwrap()
                .get(&stored.resource_token)
                .unwrap()
                .mime(),
            "image/png"
        );
        assert_eq!(
            store(&state, &library, &stored.resource_token, &png())
                .unwrap_err()
                .code,
            "invalid"
        );
        let replacement = Arc::new(Library::open(root.path().join("two")).unwrap());
        *lock(&state.library).unwrap() = Some(replacement);
        assert_eq!(
            store(&state, &library, &preview.resource_token, &png())
                .unwrap_err()
                .code,
            "library_changed"
        );
        assert_eq!(
            register(
                &state,
                &library,
                PreviewResource::glb("unused".into(), None)
            )
            .unwrap_err()
            .code,
            "library_changed"
        );
    }
}
