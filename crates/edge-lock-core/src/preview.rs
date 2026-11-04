//! Disposable GLB previews. Sources are read-only; only the library's managed cache is written.
use crate::{
    CoreError, ExportConfig, Library, ManifestEntry, OperationContext, Result,
    blender::{BlenderRunner, Inspection},
    files::{ensure_real_directory, safe_relative},
    glb,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const CACHE_VERSION: u32 = 2;
const TEMP_MARKER: &str = ".edge-lock-preview-temporary";

pub struct PreviewRequest<'a> {
    pub variant_id: &'a str,
    pub snapshot_id: Option<&'a str>,
    pub preview_file: Option<&'a str>,
    pub blender_path: Option<&'a Path>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PreviewKind {
    AssetGlb,
    GeneratedGlb,
}

pub struct PreviewArtifact {
    pub path: PathBuf,
    pub artifact_key: String,
    pub inspection: Inspection,
    pub source_file: String,
    pub source_kind: PreviewKind,
    pub updates_geometry: bool,
}

pub enum PreviewOutcome {
    Ready(Box<PreviewArtifact>),
    Choose(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SourceFile {
    pub(crate) path: String,
    pub(crate) hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SourceFingerprint {
    pub(crate) path: String,
    pub(crate) members: Vec<SourceFile>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CacheRecord {
    version: u32,
    identity: String,
    converter: Option<String>,
    artifact: String,
    artifact_hash: String,
    source_file: String,
    source_kind: PreviewKind,
    sources: Vec<SourceFingerprint>,
    inspection: Inspection,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreviewPreference {
    version: u32,
    #[serde(default)]
    variant_id: Option<String>,
    #[serde(default)]
    snapshot_id: Option<String>,
    config: ExportConfig,
    source_file: String,
}

fn preference_path(cache: &Path, variant_id: &str, snapshot_id: Option<&str>) -> PathBuf {
    let variant = blake3::hash(variant_id.as_bytes()).to_hex();
    let context = snapshot_id
        .map(|snapshot| format!("-{}", blake3::hash(snapshot.as_bytes()).to_hex()))
        .unwrap_or_default();
    cache.join(format!("choice-{variant}{context}.json"))
}

fn remembered_choice(
    root: &Path,
    variant_id: &str,
    snapshot_id: Option<&str>,
    config: &ExportConfig,
    entries: &[ManifestEntry],
) -> Option<String> {
    let cache = root.join("cache");
    ensure_real_directory(&cache).ok()?;
    let cache = cache.join("previews");
    ensure_real_directory(&cache).ok()?;
    let path = preference_path(&cache, variant_id, snapshot_id);
    let meta = fs::symlink_metadata(&path).ok()?;
    if !meta.is_file() || is_link(&meta) || meta.len() > 4096 {
        return None;
    }
    let choice: PreviewPreference = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    (choice.version == 1
        && choice
            .variant_id
            .as_deref()
            .is_none_or(|owner| owner == variant_id)
        && choice.snapshot_id.as_deref() == snapshot_id
        && choice.config == *config
        && (extension(&choice.source_file, "glb") || extension(&choice.source_file, "blend"))
        && safe_relative(&choice.source_file).is_ok()
        && entries
            .iter()
            .any(|entry| entry.kind == "file" && entry.path == choice.source_file))
    .then_some(choice.source_file)
}

fn extension(path: &str, expected: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|value| value.eq_ignore_ascii_case(expected))
}

fn choose_glb(
    entries: &[ManifestEntry],
    config: &ExportConfig,
    selected: Option<&str>,
) -> Result<Vec<String>> {
    let mut files: Vec<_> = entries
        .iter()
        .filter(|e| e.kind == "file" && extension(&e.path, "glb"))
        .map(|e| e.path.clone())
        .collect();
    files.sort();
    if let Some(selected) = selected {
        safe_relative(selected)?;
        if !(extension(selected, "glb") || extension(selected, "blend"))
            || !entries
                .iter()
                .any(|entry| entry.kind == "file" && entry.path == selected)
        {
            return Err(CoreError::Invalid(
                "Le fichier .blend ou .glb choisi doit appartenir au dossier de l’asset.".into(),
            ));
        }
        return Ok(vec![selected.into()]);
    }
    if let Some(generated) = config
        .generated_glb_file
        .as_ref()
        .filter(|path| files.contains(path))
    {
        return Ok(vec![generated.clone()]);
    }
    if let Some(primary) = config.primary_file.as_ref() {
        safe_relative(primary)?;
        if files.contains(primary) {
            return Ok(vec![primary.clone()]);
        }
        let matching: Vec<_> = files
            .iter()
            .filter(|file| {
                Path::new(file)
                    .file_stem()
                    .zip(Path::new(primary).file_stem())
                    .is_some_and(|(a, b)| a.eq_ignore_ascii_case(b))
            })
            .cloned()
            .collect();
        if matching.len() == 1 {
            return Ok(matching);
        }
    }
    Ok(files)
}

fn is_link(meta: &fs::Metadata) -> bool {
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

fn managed_cache(root: &Path) -> Result<PathBuf> {
    let mut path = root.to_path_buf();
    for name in ["cache", "previews"] {
        path.push(name);
        if !path.exists() {
            fs::create_dir(&path)?;
        }
        ensure_real_directory(&path)?;
    }
    Ok(path)
}

/// Called while the desktop operation gate is held. Only marked app-owned work dirs are removed.
pub fn cleanup_temporary(root: &Path) -> Result<()> {
    let cache = managed_cache(root)?;
    for entry in fs::read_dir(&cache)? {
        let entry = entry?;
        let path = entry.path();
        let meta = fs::symlink_metadata(&path)?;
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with(".preview-tmp-")
            && meta.is_dir()
            && !is_link(&meta)
        {
            let marker = path.join(TEMP_MARKER);
            if fs::symlink_metadata(&marker).is_ok_and(|m| m.is_file() && !is_link(&m))
                && fs::read(&marker).is_ok_and(|v| v == b"edge-lock-preview-v2")
                && fs::canonicalize(&path)?.starts_with(fs::canonicalize(&cache)?)
            {
                fs::remove_dir_all(&path)?;
            }
        }
    }
    Ok(())
}

pub(crate) fn hash_file(path: &Path, ctx: &OperationContext) -> Result<Option<String>> {
    ctx.check_cancelled()?;
    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let before = file.metadata()?;
    if !before.is_file() {
        return Err(CoreError::Invalid(
            "Une dépendance doit être un fichier ordinaire.".into(),
        ));
    }
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        ctx.check_cancelled()?;
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let after = fs::metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            CoreError::Inconsistent("Une source a disparu pendant sa lecture.".into())
        } else {
            error.into()
        }
    })?;
    if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
        return Err(CoreError::Inconsistent(
            "Une source a changé pendant sa lecture.".into(),
        ));
    }
    Ok(Some(hasher.finalize().to_hex().to_string()))
}

fn pattern_parts(name: &str) -> Option<(&str, usize, &str)> {
    if let Some((prefix, suffix)) = name.split_once("<UDIM>") {
        return Some((prefix, 4, suffix));
    }
    if let Some(start) = name.find('#') {
        let count = name[start..].bytes().take_while(|b| *b == b'#').count();
        return Some((&name[..start], count, &name[start + count..]));
    }
    if let Some(start) = name.find("%0") {
        let end = name[start + 2..].find('d')? + start + 2;
        let count = name[start + 2..end].parse().ok()?;
        return Some((&name[..start], count, &name[end + 1..]));
    }
    None
}

fn matches_pattern(pattern: &str, name: &str) -> bool {
    let Some((prefix, count, suffix)) = pattern_parts(pattern) else {
        return pattern == name;
    };
    name.strip_prefix(prefix)
        .and_then(|v| v.strip_suffix(suffix))
        .is_some_and(|v| v.len() == count && v.bytes().all(|b| b.is_ascii_digit()))
}

fn source_members(
    path: &Path,
    root: &Path,
    entries: Option<&[ManifestEntry]>,
) -> Result<Vec<PathBuf>> {
    let name = path
        .file_name()
        .and_then(|p| p.to_str())
        .unwrap_or_default();
    if pattern_parts(name).is_none() {
        return Ok(vec![path.into()]);
    }
    let parent = path
        .parent()
        .ok_or_else(|| CoreError::Invalid("Motif de dépendance invalide.".into()))?;
    if let Some(entries) = entries
        && parent.starts_with(root)
    {
        return Ok(entries
            .iter()
            .filter(|e| e.kind == "file")
            .filter_map(|e| {
                let candidate = root.join(&e.path);
                (candidate.parent() == Some(parent)
                    && candidate
                        .file_name()
                        .and_then(|p| p.to_str())
                        .is_some_and(|n| matches_pattern(name, n)))
                .then_some(candidate)
            })
            .collect());
    }
    match fs::read_dir(parent) {
        Ok(items) => items
            .map(|e| e.map(|e| e.path()))
            .collect::<std::io::Result<Vec<_>>>()
            .map(|items| {
                items
                    .into_iter()
                    .filter(|p| {
                        p.file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(|n| matches_pattern(name, n))
                    })
                    .collect()
            })
            .map_err(Into::into),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(e) => Err(e.into()),
    }
}

pub(crate) fn fingerprints(
    paths: &[String],
    root: &Path,
    entries: Option<&[ManifestEntry]>,
    ctx: &OperationContext,
) -> Result<Vec<SourceFingerprint>> {
    paths
        .iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|value| {
            ctx.check_cancelled()?;
            let path = Path::new(value);
            if !path.is_absolute() {
                return Err(CoreError::Invalid(
                    "Une dépendance doit avoir un chemin absolu.".into(),
                ));
            }
            let mut members = Vec::new();
            for member in source_members(path, root, entries)? {
                let hash = if let Some(entries) = entries
                    && let Ok(relative) = member.strip_prefix(root)
                {
                    let relative = relative.to_string_lossy().replace('\\', "/");
                    entries
                        .iter()
                        .find(|e| e.kind == "file" && e.path == relative)
                        .and_then(|e| e.hash.clone())
                } else {
                    hash_file(&member, ctx)?
                };
                members.push(SourceFile {
                    path: member.to_string_lossy().into_owned(),
                    hash,
                });
            }
            members.sort_by(|a, b| a.path.cmp(&b.path));
            Ok(SourceFingerprint {
                path: value.clone(),
                members,
            })
        })
        .collect()
}

pub(crate) fn source_paths(primary: &Path, inspection: &Inspection) -> Vec<String> {
    let mut paths = vec![primary.to_string_lossy().into_owned()];
    paths.extend(
        inspection
            .dependencies
            .iter()
            .filter(|d| d.render_relevant && d.status != "packed")
            .filter_map(|d| d.source_path.clone()),
    );
    paths.sort();
    paths.dedup();
    paths
}

pub(crate) fn converter_identity(path: Option<&Path>) -> Option<String> {
    let path = path?;
    let meta = fs::metadata(path).ok()?;
    Some(format!(
        "{}:{}:{:?}",
        path.display(),
        meta.len(),
        meta.modified().ok()
    ))
}

fn read_record(path: &Path) -> Option<CacheRecord> {
    let meta = fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || is_link(&meta) || meta.len() > 4 * 1024 * 1024 {
        return None;
    }
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

pub(crate) fn atomic_json(path: &Path, value: &impl Serialize, staging: &Path) -> Result<()> {
    let mut temporary = tempfile::NamedTempFile::new_in(staging)?;
    temporary.write_all(&serde_json::to_vec(value)?)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .map_err(|e| CoreError::Io(e.error))?;
    Ok(())
}

pub(crate) fn copy_file(source: &Path, destination: &Path, ctx: &OperationContext) -> Result<()> {
    let mut input = fs::File::open(source)?;
    let mut output = fs::File::create(destination)?;
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        ctx.check_cancelled()?;
        let length = input.read(&mut buffer)?;
        if length == 0 {
            break;
        }
        output.write_all(&buffer[..length])?;
    }
    output.sync_all()?;
    Ok(())
}

fn glb_inspection(path: &Path) -> Result<Inspection> {
    Ok(Inspection {
        geometry: glb::inspect_glb(path)?.geometry,
        scenes: vec![],
        collections: vec![],
        dependencies: vec![],
        warnings: vec![],
        blender_version: "GLB 2.0".into(),
    })
}

/// Caller serializes jobs for a library; a queued duplicate observes the published cache.
pub fn prepare_preview(
    library: &Library,
    request: PreviewRequest<'_>,
    ctx: &OperationContext,
) -> Result<PreviewOutcome> {
    prepare_preview_inner(library, request, ctx, true)
}

/// Desktop selections are stored by the application, independently from legacy cache choices.
pub fn prepare_view_preview(
    library: &Library,
    request: PreviewRequest<'_>,
    ctx: &OperationContext,
) -> Result<PreviewOutcome> {
    prepare_preview_inner(library, request, ctx, false)
}

/// Gallery previews are deterministic and never overwrite the detail view's selection.
pub fn prepare_library_preview(
    library: &Library,
    variant_id: &str,
    blender_path: Option<&Path>,
    ctx: &OperationContext,
) -> Result<Box<PreviewArtifact>> {
    let variant = library.get_variant(variant_id)?;
    let selected = choose_glb(&variant.files, &variant.variant.config, None)?
        .into_iter()
        .next();
    match prepare_preview_inner(
        library,
        PreviewRequest {
            variant_id,
            snapshot_id: None,
            preview_file: selected.as_deref(),
            blender_path,
        },
        ctx,
        false,
    )? {
        PreviewOutcome::Ready(artifact) => Ok(artifact),
        PreviewOutcome::Choose(_) => Err(CoreError::Inconsistent(
            "La source de la vignette a changé pendant sa préparation.".into(),
        )),
    }
}

fn prepare_preview_inner(
    library: &Library,
    request: PreviewRequest<'_>,
    ctx: &OperationContext,
    remember_selection: bool,
) -> Result<PreviewOutcome> {
    ctx.check_cancelled()?;
    let variant = library.get_variant(request.variant_id)?;
    let snapshot = request
        .snapshot_id
        .map(|id| library.get_snapshot(id))
        .transpose()?;
    let (logical_root, entries, mut config) = if let Some(snapshot) = &snapshot {
        if snapshot.storage.kind == "deleted" || !snapshot.storage.preview_trusted {
            return Err(CoreError::Invalid("Cette capture ne dispose pas d’un aperçu historique certifié. Son dossier complet reste restaurable.".into()));
        }
        (
            library.snapshot_workspace(request.variant_id, &snapshot.id)?,
            snapshot.entries.clone(),
            snapshot.config.clone(),
        )
    } else {
        (
            PathBuf::from(&variant.variant.workspace_path),
            variant.files,
            variant.variant.config.clone(),
        )
    };
    let logical_root = dunce::simplified(&logical_root).to_path_buf();
    let remembered = if remember_selection && request.preview_file.is_none() {
        remembered_choice(
            library.root(),
            request.variant_id,
            request.snapshot_id,
            &config,
            &entries,
        )
    } else {
        None
    };
    let preference_config = config.clone();
    let candidates = choose_glb(
        &entries,
        &config,
        request.preview_file.or(remembered.as_deref()),
    )?;
    if candidates.len() > 1 {
        return Ok(PreviewOutcome::Choose(candidates));
    }
    let kind = if candidates
        .first()
        .is_none_or(|file| extension(file, "blend"))
    {
        PreviewKind::GeneratedGlb
    } else {
        PreviewKind::AssetGlb
    };
    if let Some(selected) = candidates.first().filter(|file| extension(file, "blend")) {
        if config.primary_file.as_ref() != Some(selected) {
            config.scene = None;
            config.collection = None;
        }
        config.primary_file = Some(selected.clone());
    }
    if kind == PreviewKind::GeneratedGlb
        && !config.primary_file.as_ref().is_some_and(|primary| {
            extension(primary, "blend")
                && entries
                    .iter()
                    .any(|e| e.kind == "file" && &e.path == primary)
        })
    {
        config.primary_file =
            Some(crate::sync::resolve_project(&logical_root, &entries, &config)?.blend_file);
    }
    let source_file = candidates
        .first()
        .cloned()
        .or_else(|| config.primary_file.clone())
        .ok_or_else(|| {
            CoreError::Invalid(
                "Choisissez un fichier principal .blend ou un GLB pour l’aperçu.".into(),
            )
        })?;
    safe_relative(&source_file)?;
    let updates_geometry = preference_config.primary_file.as_deref() == Some(&source_file)
        || (kind == PreviewKind::GeneratedGlb && preference_config.primary_file.is_none());
    let primary = logical_root.join(&source_file);
    let converter = if kind == PreviewKind::GeneratedGlb {
        converter_identity(request.blender_path)
    } else {
        None
    };
    let identity = blake3::hash(&serde_json::to_vec(&(
        CACHE_VERSION,
        kind,
        &source_file,
        (kind == PreviewKind::GeneratedGlb).then_some(&config),
        blake3::hash(include_bytes!("../../../blender/edge_lock.py"))
            .to_hex()
            .to_string(),
    ))?)
    .to_hex()
    .to_string();
    let index = blake3::hash(&serde_json::to_vec(&(
        request.variant_id,
        request.snapshot_id,
        &source_file,
    ))?)
    .to_hex()
    .to_string();
    let cache = managed_cache(library.root())?;
    cleanup_temporary(library.root())?;
    let remember_choice = || {
        if remember_selection && request.preview_file.is_some() {
            // This is disposable UI preference data; failure must not hide a valid preview.
            let _ = atomic_json(
                &preference_path(&cache, request.variant_id, request.snapshot_id),
                &PreviewPreference {
                    version: 1,
                    variant_id: Some(request.variant_id.into()),
                    snapshot_id: request.snapshot_id.map(str::to_string),
                    config: preference_config.clone(),
                    source_file: source_file.clone(),
                },
                &cache,
            );
        }
    };
    let record_path = cache.join(format!("index-{index}.json"));
    let manifest = snapshot.as_ref().map(|s| s.entries.as_slice());
    if let Some(record) = read_record(&record_path)
        && record.version == CACHE_VERSION
        && record.identity == identity
        && (converter.is_none() || record.converter == converter)
        && record.artifact.len() == 64
        && record.artifact.bytes().all(|b| b.is_ascii_hexdigit())
    {
        let path = cache.join(format!("{}.glb", record.artifact));
        let paths: Vec<_> = record.sources.iter().map(|s| s.path.clone()).collect();
        let unchanged = match fingerprints(&paths, &logical_root, manifest, ctx) {
            Ok(sources) => sources == record.sources,
            Err(CoreError::Inconsistent(_)) => false,
            Err(error) => return Err(error),
        };
        if unchanged
            && fs::symlink_metadata(&path).is_ok_and(|m| m.is_file() && !is_link(&m))
            && glb::guard_preview(&path).is_ok()
            && hash_file(&path, ctx)?.as_deref() == Some(&record.artifact_hash)
        {
            ctx.report("Aperçu en cache", 1, Some(1));
            remember_choice();
            return Ok(PreviewOutcome::Ready(Box::new(PreviewArtifact {
                path,
                artifact_key: record.artifact,
                inspection: record.inspection,
                source_file,
                source_kind: kind,
                updates_geometry,
            })));
        }
    }
    for attempt in 0..2 {
        let result = (|| -> Result<PreviewOutcome> {
            let temporary = tempfile::Builder::new()
                .prefix(".preview-tmp-")
                .tempdir_in(&cache)?;
            fs::write(temporary.path().join(TEMP_MARKER), b"edge-lock-preview-v2")?;
            let staged = temporary.path().join("preview.glb");
            ctx.report("Préparation de l’aperçu", 0, None);
            let (before, inspection) = if kind == PreviewKind::AssetGlb {
                let paths = vec![primary.to_string_lossy().into_owned()];
                let before = fingerprints(&paths, &logical_root, manifest, ctx)?;
                if let Some(snapshot) = &snapshot {
                    library.extract_snapshot_file(&snapshot.id, &source_file, &staged, ctx)?;
                } else {
                    copy_file(&primary, &staged, ctx)?;
                }
                if fingerprints(&paths, &logical_root, manifest, ctx)? != before {
                    return Err(CoreError::Inconsistent(
                        "Le GLB source a changé pendant sa copie.".into(),
                    ));
                }
                (before, glb_inspection(&staged)?)
            } else {
                let executable = request.blender_path.ok_or_else(|| {
                    CoreError::External(
                        "Configurez Blender dans les réglages pour générer cet aperçu.".into(),
                    )
                })?;
                let runner = BlenderRunner::new(executable.into());
                let material = temporary.path().join("source");
                let captured = if let Some(snapshot) = &snapshot {
                    library.materialize(&snapshot.id, &material, ctx)?;
                    snapshot.clone()
                } else {
                    library.capture_preview(request.variant_id, &material, ctx)?
                };
                let discovery = runner.inspect_preview(&material, &logical_root, &config, ctx)?;
                let paths = source_paths(&primary, &discovery);
                let before = fingerprints(&paths, &logical_root, manifest, ctx)?;
                // Match live inputs to the bytes of the stable staged copy before exporting.
                for source in &before {
                    for member in &source.members {
                        if let Ok(relative) = Path::new(&member.path).strip_prefix(&logical_root) {
                            let relative = relative.to_string_lossy().replace('\\', "/");
                            let staged_hash = captured
                                .entries
                                .iter()
                                .find(|e| e.path == relative && e.kind == "file")
                                .and_then(|e| e.hash.as_ref());
                            if staged_hash != member.hash.as_ref() {
                                return Err(CoreError::Inconsistent(
                                    "Les sources de l’aperçu ont changé pendant leur copie.".into(),
                                ));
                            }
                        }
                    }
                }
                let inspection =
                    runner.preview_glb(&material, &logical_root, &config, &staged, ctx)?;
                if source_paths(&primary, &inspection) != paths {
                    return Err(CoreError::Inconsistent(
                        "Les dépendances de l’aperçu ont changé pendant la conversion.".into(),
                    ));
                }
                (before, inspection)
            };
            glb::guard_preview(&staged)?;
            let paths: Vec<_> = before.iter().map(|s| s.path.clone()).collect();
            if fingerprints(&paths, &logical_root, manifest, ctx)? != before {
                return Err(CoreError::Inconsistent(
                    "Les sources de l’aperçu ont changé pendant la conversion.".into(),
                ));
            }
            ctx.check_cancelled()?;
            let artifact = blake3::hash(&serde_json::to_vec(&(&identity, &converter, &before))?)
                .to_hex()
                .to_string();
            let path = cache.join(format!("{artifact}.glb"));
            let artifact_hash = hash_file(&staged, ctx)?
                .ok_or_else(|| CoreError::Inconsistent("GLB temporaire absent.".into()))?;
            let output = tempfile::NamedTempFile::new_in(temporary.path())?;
            copy_file(&staged, output.path(), ctx)?;
            ctx.check_cancelled()?;
            output.persist(&path).map_err(|e| CoreError::Io(e.error))?;
            let record = CacheRecord {
                version: CACHE_VERSION,
                identity: identity.clone(),
                converter: converter.clone(),
                artifact,
                artifact_hash,
                source_file: source_file.clone(),
                source_kind: kind,
                sources: before,
                inspection,
            };
            atomic_json(&record_path, &record, temporary.path())?;
            remember_choice();
            Ok(PreviewOutcome::Ready(Box::new(PreviewArtifact {
                path,
                artifact_key: record.artifact,
                inspection: record.inspection,
                source_file: source_file.clone(),
                source_kind: kind,
                updates_geometry,
            })))
        })();
        match result {
            Err(CoreError::Inconsistent(_)) if attempt == 0 => continue,
            result => return result,
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fingerprints_track_missing_restore_patterns_and_same_size_content() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = OperationContext::default();
        let texture = dir.path().join("image.png");
        let tile = dir.path().join("skin.<UDIM>.png");
        let paths = vec![
            texture.to_string_lossy().into_owned(),
            tile.to_string_lossy().into_owned(),
        ];
        let missing = fingerprints(&paths, dir.path(), None, &ctx).unwrap();
        fs::write(&texture, b"one").unwrap();
        fs::write(dir.path().join("skin.1001.png"), b"tile").unwrap();
        let restored = fingerprints(&paths, dir.path(), None, &ctx).unwrap();
        assert_ne!(missing, restored);
        fs::write(&texture, b"two").unwrap();
        assert_ne!(
            restored,
            fingerprints(&paths, dir.path(), None, &ctx).unwrap()
        );
        let first = fingerprints(&paths, dir.path(), None, &ctx).unwrap();
        fs::write(dir.path().join("skin.1002.png"), b"tile").unwrap();
        assert_ne!(first, fingerprints(&paths, dir.path(), None, &ctx).unwrap());
    }
    #[test]
    fn cleanup_preserves_unmarked_data() {
        let root = tempfile::tempdir().unwrap();
        let cache = managed_cache(root.path()).unwrap();
        let owned = cache.join(".preview-tmp-old");
        fs::create_dir(&owned).unwrap();
        fs::write(owned.join(TEMP_MARKER), b"edge-lock-preview-v2").unwrap();
        let unrelated = cache.join(".preview-tmp-user");
        fs::create_dir(&unrelated).unwrap();
        fs::write(unrelated.join("keep"), b"keep").unwrap();
        cleanup_temporary(root.path()).unwrap();
        assert!(!owned.exists());
        assert!(unrelated.join("keep").exists());
    }
}
