//! Canonical, read-only Blender sources and regenerable GLB synchronization receipts.
use crate::{
    AssetDetail, CoreError, ExportConfig, Library, ManifestEntry, OperationContext, Result,
    Snapshot, Variant,
    blender::{BlenderRunner, Inspection},
    files::{FileStore, ensure_real_directory, list_files, safe_relative},
    glb,
    preview::{
        SourceFingerprint, atomic_json, converter_identity, copy_file, fingerprints, hash_file,
        source_paths,
    },
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

const RECEIPT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectLayout {
    pub blend_file: String,
    pub glb_file: String,
}

impl ProjectLayout {
    pub fn config(&self) -> ExportConfig {
        ExportConfig {
            primary_file: Some(self.blend_file.clone()),
            generated_glb_file: Some(self.glb_file.clone()),
            scene: None,
            collection: None,
            apply_modifiers: true,
        }
    }
}

pub(crate) fn is_glb(path: &str) -> bool {
    has_extension(path, "glb")
}

fn has_extension(path: &str, extension: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|value| value.eq_ignore_ascii_case(extension))
}

/// Compatibility entry point when no source directory or saved selection is available.
pub fn validate_project(entries: &[ManifestEntry]) -> Result<ProjectLayout> {
    resolve_project(Path::new(""), entries, &ExportConfig::default())
}

/// Infer once from original metadata, then keep a valid saved choice stable.
pub fn resolve_project(
    root: &Path,
    entries: &[ManifestEntry],
    config: &ExportConfig,
) -> Result<ProjectLayout> {
    resolve_project_named(root, entries, config, None)
}

/// Asset names only affect fresh inference; saved selections remain authoritative.
pub fn resolve_project_named(
    root: &Path,
    entries: &[ManifestEntry],
    config: &ExportConfig,
    asset_name: Option<&str>,
) -> Result<ProjectLayout> {
    let blends: Vec<_> = entries
        .iter()
        .filter(|entry| entry.kind == "file" && has_extension(&entry.path, "blend"))
        .collect();
    if blends.is_empty() {
        return Err(CoreError::Invalid(
            "Aucun fichier .blend dans ce workspace. Les fichiers ont été conservés.".into(),
        ));
    }
    for blend in &blends {
        safe_relative(&blend.path)?;
    }
    let saved = config
        .primary_file
        .as_deref()
        .and_then(|path| blends.iter().find(|entry| entry.path == path).copied());
    let selected = saved.unwrap_or_else(|| {
        let modified: Vec<_> = blends
            .iter()
            .map(|entry| {
                (!root.as_os_str().is_empty())
                    .then(|| {
                        fs::symlink_metadata(root.join(&entry.path))
                            .ok()
                            .filter(|meta| meta.is_file() && !meta.file_type().is_symlink())
                            .and_then(|meta| meta.modified().ok())
                    })
                    .flatten()
            })
            .collect();
        let rank = |smaller: usize, weight: u64| {
            if blends.len() > 1 {
                weight * smaller as u64 / (blends.len() - 1) as u64
            } else {
                weight
            }
        };
        blends
            .iter()
            .enumerate()
            .max_by(|(a_index, a), (b_index, b)| {
                let score = |index: usize, entry: &ManifestEntry| {
                    let size = rank(
                        blends
                            .iter()
                            .filter(|other| other.size < entry.size)
                            .count(),
                        40,
                    );
                    let recent = rank(
                        modified
                            .iter()
                            .filter(|other| **other < modified[index])
                            .count(),
                        30,
                    );
                    let stem = Path::new(&entry.path)
                        .file_stem()
                        .and_then(|stem| stem.to_str())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    let normalized = |value: &str| {
                        value
                            .chars()
                            .filter(|character| character.is_alphanumeric())
                            .flat_map(char::to_lowercase)
                            .collect::<String>()
                    };
                    let name = if matches!(stem.as_str(), "main" | "principal" | "asset")
                        || asset_name.is_some_and(|name| normalized(name) == normalized(&stem))
                    {
                        20
                    } else {
                        0
                    };
                    let depth = Path::new(&entry.path)
                        .components()
                        .count()
                        .saturating_sub(1) as u64;
                    size + recent + name + 10 / (depth + 1)
                };
                score(*a_index, a)
                    .cmp(&score(*b_index, b))
                    .then_with(|| b.path.cmp(&a.path))
            })
            .map(|(_, entry)| *entry)
            .unwrap()
    });
    let blend_file = selected.path.clone();
    let glb_file = if let Some(path) = config
        .generated_glb_file
        .as_ref()
        .filter(|_| saved.is_some())
    {
        safe_relative(path)?;
        if !is_glb(path) {
            return Err(CoreError::Invalid(
                "Le fichier généré doit porter l’extension .glb.".into(),
            ));
        }
        path.clone()
    } else {
        let base = Path::new(&blend_file)
            .with_extension("glb")
            .to_string_lossy()
            .replace('\\', "/");
        let occupied = |path: &str| {
            entries
                .iter()
                .any(|entry| entry.path.eq_ignore_ascii_case(path))
        };
        if !occupied(&base) {
            base
        } else {
            let mut index = 1u64;
            loop {
                let extension = if index == 1 {
                    "edge-lock.glb".into()
                } else {
                    format!("edge-lock-{index}.glb")
                };
                let candidate = Path::new(&blend_file)
                    .with_extension(extension)
                    .to_string_lossy()
                    .replace('\\', "/");
                if !occupied(&candidate) {
                    break candidate;
                }
                index += 1;
            }
        }
    };
    Ok(ProjectLayout {
        blend_file,
        glb_file,
    })
}

/// With no ownership configuration every file, including GLBs, is a source.
pub fn source_changed(previous: &[ManifestEntry], current: &[ManifestEntry]) -> bool {
    source_changed_with_config(
        previous,
        &ExportConfig::default(),
        current,
        &ExportConfig::default(),
    )
}

pub fn source_changed_with_config(
    previous: &[ManifestEntry],
    previous_config: &ExportConfig,
    current: &[ManifestEntry],
    current_config: &ExportConfig,
) -> bool {
    fn source<'a>(
        entries: &'a [ManifestEntry],
        previous_config: &ExportConfig,
        current_config: &ExportConfig,
    ) -> std::collections::BTreeMap<&'a str, &'a ManifestEntry> {
        entries
            .iter()
            .filter(|entry| {
                entry.kind != "file"
                    || ![
                        previous_config.generated_glb_file.as_deref(),
                        current_config.generated_glb_file.as_deref(),
                    ]
                    .contains(&Some(entry.path.as_str()))
            })
            .map(|entry| (entry.path.as_str(), entry))
            .collect()
    }
    source(previous, previous_config, current_config)
        != source(current, previous_config, current_config)
        || previous_config.primary_file != current_config.primary_file
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReceipt {
    version: u32,
    worker: String,
    converter: Option<String>,
    blend_file: String,
    glb_file: String,
    artifact_hash: String,
    sources: Vec<SourceFingerprint>,
    inspection: Inspection,
}

pub struct SyncArtifact {
    /// An immutable content-addressed file, safe for an already running preview reader.
    pub path: PathBuf,
    pub artifact_key: String,
    pub source_file: String,
    pub inspection: Inspection,
    pub source_changed: bool,
    pub receipt: SyncReceipt,
}

fn worker_identity() -> String {
    blake3::hash(include_bytes!("../../../blender/edge_lock.py"))
        .to_hex()
        .to_string()
}

fn managed_directory(root: &Path, parts: &[&str]) -> Result<PathBuf> {
    let mut path = root.to_path_buf();
    for part in parts {
        path.push(part);
        if !path.exists() {
            fs::create_dir(&path)?;
        }
        ensure_real_directory(&path)?;
    }
    Ok(path)
}

fn receipt_path(root: &Path, variant_id: &str) -> Result<PathBuf> {
    Ok(managed_directory(root, &["cache", "sync"])?.join(format!(
        "{}.json",
        blake3::hash(variant_id.as_bytes()).to_hex()
    )))
}

fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| {
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            metadata.is_file() && metadata.file_attributes() & 0x400 == 0
        }
        #[cfg(not(windows))]
        {
            metadata.is_file() && !metadata.file_type().is_symlink()
        }
    })
}

fn workspace_destination(workspace: &Path, relative: &str) -> Result<PathBuf> {
    ensure_real_directory(workspace)?;
    let relative = safe_relative(relative)?;
    let mut directory = workspace.to_path_buf();
    if let Some(parent) = relative.parent() {
        for component in parent.components() {
            directory.push(component);
            ensure_real_directory(&directory)?;
        }
    }
    if !fs::canonicalize(&directory)?.starts_with(fs::canonicalize(workspace)?) {
        return Err(CoreError::Invalid(
            "La destination du GLB sort du projet.".into(),
        ));
    }
    let destination = workspace.join(relative);
    if fs::symlink_metadata(&destination).is_ok() && !is_regular_file(&destination) {
        return Err(CoreError::Invalid(
            "Le GLB doit être un fichier ordinaire, sans lien.".into(),
        ));
    }
    Ok(destination)
}

fn read_receipt(root: &Path, variant_id: &str) -> Result<Option<SyncReceipt>> {
    let path = receipt_path(root, variant_id)?;
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !is_regular_file(&path) || metadata.len() > 4 * 1024 * 1024 {
        return Ok(None);
    }
    Ok(serde_json::from_slice(&fs::read(path)?).ok())
}

fn verified_copy(
    source: &Path,
    destination: &Path,
    expected_hash: &str,
    ctx: &OperationContext,
) -> Result<tempfile::NamedTempFile> {
    let parent = destination
        .parent()
        .ok_or_else(|| CoreError::Invalid("Destination GLB invalide.".into()))?;
    ensure_real_directory(parent)?;
    let temporary = tempfile::Builder::new()
        .prefix(".edge-lock-")
        .tempfile_in(parent)?;
    copy_file(source, temporary.path(), ctx)?;
    if hash_file(temporary.path(), ctx)?.as_deref() != Some(expected_hash) {
        return Err(CoreError::Inconsistent(
            "Le GLB a changé pendant sa copie ; le fichier précédent a été conservé.".into(),
        ));
    }
    ctx.check_cancelled()?;
    Ok(temporary)
}

fn atomic_copy(
    source: &Path,
    destination: &Path,
    expected_hash: &str,
    ctx: &OperationContext,
) -> Result<()> {
    let temporary = verified_copy(source, destination, expected_hash, ctx)?;
    temporary
        .persist(destination)
        .map_err(|error| CoreError::Io(error.error))?;
    Ok(())
}

fn artifact_path(root: &Path, hash: &str) -> Result<PathBuf> {
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(CoreError::Inconsistent("Empreinte GLB invalide.".into()));
    }
    Ok(managed_directory(root, &["cache", "canonical"])?.join(format!("{hash}.glb")))
}

fn ensure_artifact(
    root: &Path,
    source: &Path,
    receipt: &SyncReceipt,
    ctx: &OperationContext,
) -> Result<PathBuf> {
    let path = artifact_path(root, &receipt.artifact_hash)?;
    if !is_regular_file(&path) || hash_file(&path, ctx)?.as_deref() != Some(&receipt.artifact_hash)
    {
        atomic_copy(source, &path, &receipt.artifact_hash, ctx)?;
    }
    Ok(path)
}

impl SyncReceipt {
    pub(crate) fn config(&self) -> ExportConfig {
        ProjectLayout {
            blend_file: self.blend_file.clone(),
            glb_file: self.glb_file.clone(),
        }
        .config()
    }

    fn paths(&self) -> Vec<String> {
        self.sources
            .iter()
            .map(|source| source.path.clone())
            .collect()
    }

    fn tracks_blend(&self, root: &Path) -> bool {
        let blend = root.join(&self.blend_file);
        self.sources.iter().any(|source| {
            Path::new(&source.path) == blend
                && source
                    .members
                    .iter()
                    .any(|member| Path::new(&member.path) == blend && member.hash.is_some())
        })
    }

    /// A legacy receipt may establish output ownership even when its source has since changed.
    fn owns_output(&self, root: &Path, ctx: &OperationContext) -> Result<bool> {
        if self.version != RECEIPT_VERSION
            || self.worker != worker_identity()
            || !self.tracks_blend(root)
            || !is_glb(&self.glb_file)
        {
            return Ok(false);
        }
        let Ok(path) = workspace_destination(root, &self.glb_file) else {
            return Ok(false);
        };
        if !is_regular_file(&path) {
            return Ok(false);
        }
        Ok(hash_file(&path, ctx)?.as_deref() == Some(self.artifact_hash.as_str()))
    }

    pub(crate) fn validate_capture(
        &self,
        root: &Path,
        entries: &[ManifestEntry],
        ctx: &OperationContext,
    ) -> Result<()> {
        let layout = resolve_project(root, entries, &self.config())?;
        let output_hash = entries
            .iter()
            .find(|entry| entry.path == layout.glb_file && entry.kind == "file")
            .and_then(|entry| entry.hash.as_deref());
        if !self.tracks_blend(root)
            || self.version != RECEIPT_VERSION
            || self.worker != worker_identity()
            || layout.blend_file != self.blend_file
            || layout.glb_file != self.glb_file
            || output_hash != Some(self.artifact_hash.as_str())
            || fingerprints(&self.paths(), root, Some(entries), ctx)? != self.sources
        {
            return Err(CoreError::Inconsistent("Le .blend ou ses dépendances ont changé depuis la génération du GLB. Synchronisation à relancer.".into()));
        }
        Ok(())
    }

    fn rebase(&mut self, previous: &Path, next: &Path) {
        fn rebase_path(value: &mut String, previous: &Path, next: &Path) {
            if let Ok(relative) = Path::new(value).strip_prefix(previous) {
                *value = next.join(relative).to_string_lossy().into_owned();
            }
        }
        for source in &mut self.sources {
            rebase_path(&mut source.path, previous, next);
            for member in &mut source.members {
                rebase_path(&mut member.path, previous, next);
            }
        }
        for dependency in &mut self.inspection.dependencies {
            if let Some(path) = &mut dependency.source_path {
                rebase_path(path, previous, next);
            }
        }
    }
}

fn matches_capture(
    sources: &[SourceFingerprint],
    logical_root: &Path,
    entries: &[ManifestEntry],
) -> bool {
    sources
        .iter()
        .flat_map(|source| &source.members)
        .all(|member| {
            let Ok(relative) = Path::new(&member.path).strip_prefix(logical_root) else {
                return true;
            };
            let path = relative.to_string_lossy().replace('\\', "/");
            entries
                .iter()
                .find(|entry| entry.path == path && entry.kind == "file")
                .and_then(|entry| entry.hash.as_ref())
                == member.hash.as_ref()
        })
}

struct GenerationSource<'a> {
    material: &'a Path,
    logical_root: &'a Path,
    entries: &'a [ManifestEntry],
    historical: bool,
}

fn generate(
    runner: &BlenderRunner,
    source: GenerationSource<'_>,
    config: &ExportConfig,
    output: &Path,
    ctx: &OperationContext,
) -> Result<SyncReceipt> {
    let GenerationSource {
        material,
        logical_root,
        entries,
        historical,
    } = source;
    let layout = resolve_project(logical_root, entries, config)?;
    let config = layout.config();
    let discovery = runner.inspect_preview(material, logical_root, &config, ctx)?;
    let paths = source_paths(&logical_root.join(&layout.blend_file), &discovery);
    let manifest = historical.then_some(entries);
    let before = fingerprints(&paths, logical_root, manifest, ctx)?;
    if !matches_capture(&before, logical_root, entries) {
        return Err(CoreError::Inconsistent(
            "Les sources ont changé pendant leur copie.".into(),
        ));
    }
    let mut inspection = runner.generate_glb(material, logical_root, &config, output, ctx)?;
    let captured_blend_hash = entries
        .iter()
        .find(|entry| entry.kind == "file" && entry.path == layout.blend_file)
        .and_then(|entry| entry.hash.as_deref());
    if hash_file(&material.join(&layout.blend_file), ctx)?.as_deref() != captured_blend_hash {
        return Err(CoreError::Inconsistent(
            "La copie Blender a changé pendant la génération ; publication annulée.".into(),
        ));
    }
    if source_paths(&logical_root.join(&layout.blend_file), &inspection) != paths
        || fingerprints(&paths, logical_root, manifest, ctx)? != before
    {
        return Err(CoreError::Inconsistent(
            "Les sources ont changé pendant la génération du GLB.".into(),
        ));
    }
    inspection.geometry = glb::inspect_glb(output)?.geometry;
    let artifact_hash = hash_file(output, ctx)?
        .ok_or_else(|| CoreError::Inconsistent("GLB généré absent.".into()))?;
    Ok(SyncReceipt {
        version: RECEIPT_VERSION,
        worker: worker_identity(),
        converter: converter_identity(Some(runner.executable())),
        blend_file: layout.blend_file,
        glb_file: layout.glb_file,
        artifact_hash,
        sources: before,
        inspection,
    })
}

fn copy_source(
    source: &Path,
    destination: &Path,
    staging: &Path,
    ctx: &OperationContext,
) -> Result<Vec<ManifestEntry>> {
    let objects = staging.join("objects");
    fs::create_dir(&objects)?;
    let store = FileStore {
        objects,
        staging: staging.to_path_buf(),
    };
    let entries = store.capture(source, true, ctx)?;
    store.materialize(&entries, destination, ctx)?;
    Ok(entries)
}

fn save_receipt(library: &Library, variant_id: &str, receipt: &SyncReceipt) -> Result<()> {
    let path = receipt_path(library.root(), variant_id)?;
    atomic_json(&path, receipt, path.parent().expect("receipt parent"))
}

fn backup_unversioned_glb(
    library: &Library,
    variant_id: &str,
    source: &Path,
    hash: &str,
    ctx: &OperationContext,
) -> Result<()> {
    let variant = library.get_variant(variant_id)?.variant;
    let head = library.get_snapshot(&variant.head_snapshot_id)?;
    if head
        .entries
        .iter()
        .any(|entry| entry.hash.as_deref() == Some(hash))
    {
        return Ok(());
    }
    let directory = managed_directory(library.root(), &["backups", "glb"])?;
    let backup = directory.join(format!("{hash}.glb"));
    if hash_file(&backup, ctx)?.as_deref() != Some(hash) {
        atomic_copy(source, &backup, hash, ctx)?;
    }
    library.register_glb_backup(variant_id, hash)?;
    Ok(())
}

fn finish_artifact(
    library: &Library,
    variant_id: &str,
    receipt: SyncReceipt,
    changed: bool,
    ctx: &OperationContext,
) -> Result<SyncArtifact> {
    let workspace = library.workspace_path(variant_id)?;
    let changes = library.changes_checked(variant_id, &receipt, ctx)?;
    let path = ensure_artifact(
        library.root(),
        &workspace.join(&receipt.glb_file),
        &receipt,
        ctx,
    )?;
    library.record_synchronized(
        variant_id,
        &receipt.config(),
        receipt.inspection.geometry.triangles,
        changed,
    )?;
    Ok(SyncArtifact {
        path,
        artifact_key: receipt.artifact_hash.clone(),
        source_file: receipt.glb_file.clone(),
        inspection: receipt.inspection.clone(),
        source_changed: changes.has_source_changes(),
        receipt,
    })
}

/// The desktop operation gate serializes callers. Source writes from Blender are checked separately.
pub fn synchronize_variant(
    library: &Library,
    variant_id: &str,
    runner: Option<&BlenderRunner>,
    ctx: &OperationContext,
) -> Result<SyncArtifact> {
    for attempt in 0..2 {
        let result = synchronize_once(library, variant_id, runner, ctx);
        match result {
            Err(CoreError::Inconsistent(_)) if attempt == 0 => continue,
            result => return result,
        }
    }
    unreachable!()
}

fn synchronize_once(
    library: &Library,
    variant_id: &str,
    runner: Option<&BlenderRunner>,
    ctx: &OperationContext,
) -> Result<SyncArtifact> {
    let variant = library.get_variant(variant_id)?;
    let workspace = dunce::simplified(Path::new(&variant.variant.workspace_path)).to_path_buf();
    if !fs::canonicalize(&workspace)?.starts_with(fs::canonicalize(library.root())?) {
        return Err(CoreError::Invalid(
            "Le dossier de travail sort de la bibliothèque.".into(),
        ));
    }
    let previous_receipt = read_receipt(library.root(), variant_id)?;
    let mut selected_config = variant.variant.config.clone();
    // Existing certified projects retain their managed destination across the upgrade.
    if selected_config.generated_glb_file.is_none()
        && let Some(receipt) = previous_receipt.as_ref()
        && selected_config.primary_file.as_deref() == Some(&receipt.blend_file)
        && receipt.owns_output(&workspace, ctx)?
    {
        selected_config.generated_glb_file = Some(receipt.glb_file.clone());
    }
    let asset_name = library.get_asset(&variant.variant.asset_id)?.asset.name;
    let layout = resolve_project_named(
        &workspace,
        &variant.files,
        &selected_config,
        Some(&asset_name),
    )?;
    if let Some(receipt) = previous_receipt.as_ref()
        && receipt.version == RECEIPT_VERSION
        && receipt.worker == worker_identity()
        && receipt.blend_file == layout.blend_file
        && receipt.glb_file == layout.glb_file
        && receipt.tracks_blend(&workspace)
        && runner
            .and_then(|runner| converter_identity(Some(runner.executable())))
            .is_none_or(|converter| receipt.converter.as_ref() == Some(&converter))
        && fingerprints(&receipt.paths(), &workspace, None, ctx)? == receipt.sources
        && hash_file(&workspace.join(&layout.glb_file), ctx)?.as_deref()
            == Some(&receipt.artifact_hash)
    {
        return finish_artifact(library, variant_id, receipt.clone(), false, ctx);
    }
    let runner = runner.ok_or_else(|| {
        CoreError::External("Configurez Blender dans les réglages pour synchroniser le GLB.".into())
    })?;
    let temporary = tempfile::Builder::new()
        .prefix("sync-")
        .tempdir_in(library.root().join("staging"))?;
    let material = temporary.path().join("source");
    let captured = library.capture_preview(variant_id, &material, ctx)?;
    let output = temporary.path().join("generated.glb");
    let receipt = generate(
        runner,
        GenerationSource {
            material: &material,
            logical_root: &workspace,
            entries: &captured.entries,
            historical: false,
        },
        &layout.config(),
        &output,
        ctx,
    )?;
    // Sources used by conversion must still match; independent models may coexist.
    let current = library.workspace_manifest(variant_id, ctx)?;
    resolve_project(&workspace, &current, &layout.config())?;
    if !matches_capture(&receipt.sources, &workspace, &current)
        || fingerprints(&receipt.paths(), &workspace, None, ctx)? != receipt.sources
    {
        return Err(CoreError::Inconsistent(
            "Le projet a changé avant publication du GLB.".into(),
        ));
    }
    let destination = workspace_destination(&workspace, &receipt.glb_file)?;
    let previous_hash = hash_file(&destination, ctx)?;
    if previous_hash.is_some() && selected_config.generated_glb_file.is_none() {
        return Err(CoreError::Inconsistent("Un fichier utilisateur est apparu à la destination du GLB ; son contenu a été conservé.".into()));
    }
    let previously_generated = previous_receipt.as_ref().is_some_and(|previous| {
        previous.version == RECEIPT_VERSION
            && previous.glb_file == layout.glb_file
            && previous_hash.as_deref() == Some(&previous.artifact_hash)
    });
    if let Some(hash) = &previous_hash
        && !previously_generated
    {
        backup_unversioned_glb(library, variant_id, &destination, hash, ctx)?;
    }
    let artifact = ensure_artifact(library.root(), &output, &receipt, ctx)?;
    let replacement = verified_copy(&artifact, &destination, &receipt.artifact_hash, ctx)?;
    // Preserve a GLB changed by another writer while the potentially large replacement was copied.
    if hash_file(&destination, ctx)? != previous_hash {
        return Err(CoreError::Inconsistent(
            "Le GLB a changé avant son remplacement ; son contenu a été conservé.".into(),
        ));
    }
    workspace_destination(&workspace, &receipt.glb_file)?;
    replacement
        .persist(&destination)
        .map_err(|error| CoreError::Io(error.error))?;
    // A raced save cannot make an obsolete output eligible for snapshot publication.
    if fingerprints(&receipt.paths(), &workspace, None, ctx)? != receipt.sources {
        return Err(CoreError::Inconsistent(
            "Le .blend a changé pendant publication du GLB.".into(),
        ));
    }
    save_receipt(library, variant_id, &receipt)?;
    finish_artifact(
        library,
        variant_id,
        receipt.clone(),
        previous_hash.as_deref() != Some(&receipt.artifact_hash),
        ctx,
    )
}

pub fn import_project(
    library: &Library,
    source: &Path,
    name: &str,
    runner: &BlenderRunner,
    ctx: &OperationContext,
) -> Result<AssetDetail> {
    ensure_real_directory(source)?;
    let source = fs::canonicalize(source)?;
    if source.starts_with(library.root()) || library.root().starts_with(&source) {
        return Err(CoreError::Invalid(
            "Le dossier importé doit être extérieur à la bibliothèque et ne pas la contenir."
                .into(),
        ));
    }
    let layout = resolve_project_named(
        &source,
        &list_files(&source)?,
        &ExportConfig::default(),
        Some(name),
    )?;
    for attempt in 0..2 {
        let result = (|| {
            let temporary = tempfile::Builder::new()
                .prefix("import-")
                .tempdir_in(library.root().join("staging"))?;
            let material = temporary.path().join("source");
            let entries = copy_source(&source, &material, temporary.path(), ctx)?;
            if entries
                .iter()
                .any(|entry| entry.path.eq_ignore_ascii_case(&layout.glb_file))
            {
                return Err(CoreError::Inconsistent(
                    "Un fichier utilisateur est apparu à la destination du GLB pendant l’import."
                        .into(),
                ));
            }
            let logical = dunce::simplified(&source);
            let output = temporary.path().join("generated.glb");
            let mut receipt = generate(
                runner,
                GenerationSource {
                    material: &material,
                    logical_root: logical,
                    entries: &entries,
                    historical: false,
                },
                &layout.config(),
                &output,
                ctx,
            )?;
            atomic_copy(
                &output,
                &material.join(&receipt.glb_file),
                &receipt.artifact_hash,
                ctx,
            )?;
            let asset = library.import_prepared(&material, name, layout.config(), ctx)?;
            let variant = &asset.variants[0];
            receipt.rebase(logical, Path::new(&variant.workspace_path));
            // Publication has committed: cache failures must not report a failed import and invite duplicates.
            let _ = save_receipt(library, &variant.id, &receipt);
            let _ = finish_artifact(library, &variant.id, receipt, false, ctx);
            library.get_asset(&asset.asset.id)
        })();
        match result {
            Err(CoreError::Inconsistent(_)) if attempt == 0 => continue,
            result => return result,
        }
    }
    unreachable!()
}

pub fn create_snapshot(
    library: &Library,
    variant_id: &str,
    message: &str,
    runner: Option<&BlenderRunner>,
    ctx: &OperationContext,
) -> Result<Snapshot> {
    for attempt in 0..2 {
        let artifact = synchronize_variant(library, variant_id, runner, ctx)?;
        match library.create_snapshot_checked(variant_id, message, &artifact.receipt, ctx) {
            Err(CoreError::Inconsistent(_)) if attempt == 0 => continue,
            result => return result,
        }
    }
    unreachable!()
}

pub fn create_variant(
    library: &Library,
    snapshot_id: &str,
    name: &str,
    runner: &BlenderRunner,
    ctx: &OperationContext,
) -> Result<Variant> {
    let snapshot = library.get_snapshot(snapshot_id)?;
    let owner = library.workspace_path(&snapshot.variant_id)?;
    let layout = resolve_project(&owner, &snapshot.entries, &snapshot.config)?;
    let logical = dunce::simplified(&owner);
    let temporary = tempfile::Builder::new()
        .prefix("variant-")
        .tempdir_in(library.root().join("staging"))?;
    let material = temporary.path().join("source");
    library.materialize(snapshot_id, &material, ctx)?;
    let output = temporary.path().join("generated.glb");
    let mut receipt = generate(
        runner,
        GenerationSource {
            material: &material,
            logical_root: logical,
            entries: &snapshot.entries,
            historical: true,
        },
        &layout.config(),
        &output,
        ctx,
    )?;
    atomic_copy(
        &output,
        &material.join(&receipt.glb_file),
        &receipt.artifact_hash,
        ctx,
    )?;
    let variant =
        library.create_variant_prepared(snapshot_id, name, &material, layout.config(), ctx)?;
    receipt.rebase(logical, Path::new(&variant.workspace_path));
    let _ = save_receipt(library, &variant.id, &receipt);
    let _ = finish_artifact(library, &variant.id, receipt, false, ctx);
    Ok(variant)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiple_models_choose_a_stable_principal_and_preserve_user_glbs() {
        let temporary = tempfile::tempdir().unwrap();
        fs::create_dir(temporary.path().join("nested")).unwrap();
        fs::write(temporary.path().join("main.blend"), b"main").unwrap();
        fs::write(temporary.path().join("nested/part.blend"), b"part").unwrap();
        for path in ["main.blend", "nested/part.blend"] {
            fs::File::options()
                .write(true)
                .open(temporary.path().join(path))
                .unwrap()
                .set_times(fs::FileTimes::new().set_modified(std::time::SystemTime::UNIX_EPOCH))
                .unwrap();
        }
        fs::write(temporary.path().join("main.glb"), b"user model").unwrap();
        fs::write(temporary.path().join("other.glb"), b"other user model").unwrap();
        let entries = list_files(temporary.path()).unwrap();
        let layout = resolve_project(temporary.path(), &entries, &ExportConfig::default()).unwrap();
        assert_eq!(layout.blend_file, "main.blend");
        assert_ne!(layout.glb_file, "main.glb");
        assert_ne!(layout.glb_file, "other.glb");
        let saved = layout.config();
        fs::write(temporary.path().join("nested/part.blend"), vec![0; 100_000]).unwrap();
        let entries = list_files(temporary.path()).unwrap();
        assert_eq!(
            resolve_project(temporary.path(), &entries, &saved).unwrap(),
            layout
        );
        let manual = ExportConfig {
            primary_file: Some("nested/part.blend".into()),
            ..Default::default()
        };
        assert_eq!(
            resolve_project(temporary.path(), &entries, &manual)
                .unwrap()
                .blend_file,
            "nested/part.blend"
        );
        assert_eq!(
            fs::read(temporary.path().join("main.glb")).unwrap(),
            b"user model"
        );
    }

    #[test]
    fn secondary_glb_edits_are_source_changes() {
        let before = vec![
            entry("main.blend"),
            entry("main.glb"),
            entry("secondary.glb"),
        ];
        let mut after = before.clone();
        after[2].size += 1;
        let config = ExportConfig {
            primary_file: Some("main.blend".into()),
            generated_glb_file: Some("main.glb".into()),
            ..Default::default()
        };
        assert!(source_changed_with_config(
            &before, &config, &after, &config
        ));
        after[2] = before[2].clone();
        after[1].size += 1;
        assert!(!source_changed_with_config(
            &before, &config, &after, &config
        ));
    }

    #[test]
    fn principal_inference_combines_size_and_original_modification_dates() {
        let temporary = tempfile::tempdir().unwrap();
        fs::write(temporary.path().join("main.blend"), b"small").unwrap();
        fs::write(temporary.path().join("large.blend"), vec![1; 100]).unwrap();
        for (path, age) in [("main.blend", 10), ("large.blend", 20)] {
            fs::File::options()
                .write(true)
                .open(temporary.path().join(path))
                .unwrap()
                .set_times(fs::FileTimes::new().set_modified(
                    std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(age),
                ))
                .unwrap();
        }
        let entries = list_files(temporary.path()).unwrap();
        assert_eq!(
            resolve_project(temporary.path(), &entries, &ExportConfig::default())
                .unwrap()
                .blend_file,
            "large.blend"
        );
        let pinned = ExportConfig {
            primary_file: Some("main.blend".into()),
            ..Default::default()
        };
        assert_eq!(
            resolve_project(temporary.path(), &entries, &pinned)
                .unwrap()
                .blend_file,
            "main.blend"
        );
        fs::remove_file(temporary.path().join("main.blend")).unwrap();
        assert_eq!(
            resolve_project(
                temporary.path(),
                &list_files(temporary.path()).unwrap(),
                &pinned
            )
            .unwrap()
            .blend_file,
            "large.blend"
        );
    }
    fn entry(path: &str) -> ManifestEntry {
        ManifestEntry {
            path: path.into(),
            kind: "file".into(),
            hash: Some("a".repeat(64)),
            size: 1,
        }
    }
    #[test]
    fn canonical_layout_is_recursive_case_insensitive_and_ignores_blender_backups() {
        let entries = vec![entry("models/Main.BLEND"), entry("models/Main.blend1")];
        let layout = validate_project(&entries).unwrap();
        assert_eq!(layout.glb_file, "models/Main.glb");
        assert!(layout.config().apply_modifiers);
        assert!(layout.config().scene.is_none());
        assert!(validate_project(&[entry("only.glb")]).is_err());
        assert!(validate_project(&[entry("a.blend"), entry("nested/b.BLEND")]).is_ok());
        assert!(
            validate_project(&[entry("a.blend"), entry("a.glb"), entry("nested/b.GLB")]).is_ok()
        );
    }
    #[test]
    fn derived_output_is_excluded_but_all_other_saved_content_is_compared() {
        let previous = vec![entry("main.blend"), entry("old.glb")];
        let mut current = vec![entry("main.blend"), entry("new.GLB")];
        let previous_config = ExportConfig {
            generated_glb_file: Some("old.glb".into()),
            ..Default::default()
        };
        let current_config = ExportConfig {
            generated_glb_file: Some("new.GLB".into()),
            ..Default::default()
        };
        assert!(!source_changed_with_config(
            &previous,
            &previous_config,
            &current,
            &current_config
        ));
        current[0].hash = Some("b".repeat(64));
        assert!(source_changed(&previous, &current));
        current = previous.clone();
        current.push(entry("report.txt"));
        assert!(source_changed(&previous, &current));
        let mut directory = entry("references.glb");
        directory.kind = "directory".into();
        current = previous.clone();
        current.push(directory);
        assert!(source_changed(&previous, &current));
    }

    fn cached_project() -> (tempfile::TempDir, Library, String, SyncReceipt) {
        let temporary = tempfile::tempdir().unwrap();
        let source = temporary.path().join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("main.blend"), b"aaaa").unwrap();
        let mut json = br#"{"asset":{"version":"2.0"},"scenes":[{"nodes":[]}],"scene":0}"#.to_vec();
        while !json.len().is_multiple_of(4) {
            json.push(b' ');
        }
        let mut glb = b"glTF".to_vec();
        glb.extend(2u32.to_le_bytes());
        glb.extend(((20 + json.len()) as u32).to_le_bytes());
        glb.extend((json.len() as u32).to_le_bytes());
        glb.extend(0x4e4f534au32.to_le_bytes());
        glb.extend(json);
        fs::write(source.join("main.glb"), glb).unwrap();
        let library = Library::open(temporary.path().join("library")).unwrap();
        let ctx = OperationContext::default();
        let asset = library
            .import_folder(&source, "Cached", ExportConfig::default(), &ctx)
            .unwrap();
        let variant = &asset.variants[0];
        let workspace = dunce::simplified(Path::new(&variant.workspace_path));
        let receipt = SyncReceipt {
            version: RECEIPT_VERSION,
            worker: worker_identity(),
            converter: None,
            blend_file: "main.blend".into(),
            glb_file: "main.glb".into(),
            artifact_hash: hash_file(&workspace.join("main.glb"), &ctx)
                .unwrap()
                .unwrap(),
            sources: fingerprints(
                &[workspace.join("main.blend").to_string_lossy().into_owned()],
                workspace,
                None,
                &ctx,
            )
            .unwrap(),
            inspection: Inspection {
                geometry: glb::inspect_glb(&workspace.join("main.glb"))
                    .unwrap()
                    .geometry,
                scenes: vec![],
                collections: vec![],
                dependencies: vec![],
                warnings: vec![],
                blender_version: "fixture".into(),
            },
        };
        save_receipt(&library, &variant.id, &receipt).unwrap();
        let id = variant.id.clone();
        (temporary, library, id, receipt)
    }

    #[test]
    fn a_verified_receipt_works_offline_and_normalizes_only_current_config() {
        let (_temporary, library, variant_id, _receipt) = cached_project();
        let original = library.get_variant(&variant_id).unwrap().variant;
        let old = library.get_snapshot(&original.head_snapshot_id).unwrap();
        assert!(!old.config.apply_modifiers);
        let artifact =
            synchronize_variant(&library, &variant_id, None, &OperationContext::default()).unwrap();
        assert!(!artifact.source_changed);
        assert!(artifact.path.is_file());
        assert!(
            library
                .get_variant(&variant_id)
                .unwrap()
                .variant
                .config
                .apply_modifiers
        );
        assert!(
            !library
                .get_snapshot(&old.id)
                .unwrap()
                .config
                .apply_modifiers
        );
        assert!(
            library
                .create_snapshot_checked(
                    &variant_id,
                    "No source edits",
                    &artifact.receipt,
                    &OperationContext::default()
                )
                .is_err()
        );
    }

    #[test]
    fn redetecting_the_same_principal_keeps_generated_ownership_after_receipt_loss() {
        let (_temporary, library, variant_id, _receipt) = cached_project();
        synchronize_variant(&library, &variant_id, None, &OperationContext::default()).unwrap();
        fs::remove_file(receipt_path(library.root(), &variant_id).unwrap()).unwrap();
        let original = library.get_variant(&variant_id).unwrap().variant.config;
        assert_eq!(original.generated_glb_file.as_deref(), Some("main.glb"));
        library
            .update_config(
                &variant_id,
                ExportConfig {
                    primary_file: None,
                    generated_glb_file: None,
                    ..original.clone()
                },
            )
            .unwrap();
        let detail = library.get_variant(&variant_id).unwrap();
        assert_eq!(
            detail.variant.config.generated_glb_file,
            original.generated_glb_file
        );
        assert_eq!(
            resolve_project(
                Path::new(&detail.variant.workspace_path),
                &detail.files,
                &detail.variant.config
            )
            .unwrap()
            .glb_file,
            "main.glb"
        );
    }

    #[test]
    fn legacy_receipt_ownership_requires_its_certified_output_but_allows_a_source_edit() {
        let (_temporary, library, variant_id, receipt) = cached_project();
        let workspace = library.workspace_path(&variant_id).unwrap();
        let workspace = dunce::simplified(&workspace);
        let ctx = OperationContext::default();
        assert!(receipt.owns_output(workspace, &ctx).unwrap());
        fs::write(
            workspace.join("main.blend"),
            b"source changed since last generation",
        )
        .unwrap();
        assert!(receipt.owns_output(workspace, &ctx).unwrap());
        fs::write(workspace.join("valuable.glb"), b"unrelated user model").unwrap();
        let mut corrupt = receipt.clone();
        corrupt.glb_file = "valuable.glb".into();
        assert!(!corrupt.owns_output(workspace, &ctx).unwrap());
        corrupt.glb_file = "../outside.glb".into();
        assert!(!corrupt.owns_output(workspace, &ctx).unwrap());
        corrupt = receipt;
        corrupt.version += 1;
        assert!(!corrupt.owns_output(workspace, &ctx).unwrap());
    }

    #[test]
    fn same_size_same_timestamp_edit_invalidates_receipt_and_cannot_publish_stale_snapshot() {
        let (_temporary, library, variant_id, receipt) = cached_project();
        let variant = library.get_variant(&variant_id).unwrap().variant;
        let blend = Path::new(&variant.workspace_path).join("main.blend");
        let modified = fs::metadata(&blend).unwrap().modified().unwrap();
        fs::write(&blend, b"bbbb").unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(&blend)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(modified))
            .unwrap();
        assert!(
            synchronize_variant(&library, &variant_id, None, &OperationContext::default()).is_err()
        );
        assert!(
            library
                .create_snapshot_checked(
                    &variant_id,
                    "Stale GLB",
                    &receipt,
                    &OperationContext::default()
                )
                .is_err()
        );
        assert_eq!(
            library
                .get_variant(&variant_id)
                .unwrap()
                .variant
                .head_snapshot_id,
            variant.head_snapshot_id
        );
        assert_eq!(fs::read(&blend).unwrap(), b"bbbb");
    }

    #[test]
    fn canonical_artifacts_remain_immutable_and_unversioned_glbs_are_preserved() {
        let (_temporary, library, variant_id, receipt) = cached_project();
        let ctx = OperationContext::default();
        let artifact = synchronize_variant(&library, &variant_id, None, &ctx).unwrap();
        let original = fs::read(&artifact.path).unwrap();
        let destination = library
            .workspace_path(&variant_id)
            .unwrap()
            .join("main.glb");
        fs::write(&destination, b"unversioned user GLB").unwrap();
        let hash = hash_file(&destination, &ctx).unwrap().unwrap();
        backup_unversioned_glb(&library, &variant_id, &destination, &hash, &ctx).unwrap();
        assert_eq!(
            fs::read(
                library
                    .root()
                    .join("backups/glb")
                    .join(format!("{hash}.glb"))
            )
            .unwrap(),
            b"unversioned user GLB"
        );
        assert_eq!(fs::read(&artifact.path).unwrap(), original);
        assert!(synchronize_variant(&library, &variant_id, None, &ctx).is_err());
        assert!(
            !library
                .changes(&variant_id, &ctx)
                .unwrap()
                .has_source_changes()
        );
        assert_ne!(hash, receipt.artifact_hash);
    }

    #[test]
    fn additional_models_keep_certified_principal_available_and_failed_imports_do_not_publish() {
        let (_temporary, library, variant_id, _receipt) = cached_project();
        let workspace = library.workspace_path(&variant_id).unwrap();
        fs::write(workspace.join("second.BLEND"), b"second source").unwrap();
        assert!(
            synchronize_variant(&library, &variant_id, None, &OperationContext::default())
                .unwrap()
                .source_changed
        );
        let runner = BlenderRunner::new("missing-blender".into());
        let source = workspace
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("source");
        fs::write(source.join("second.BLEND"), b"second source").unwrap();
        assert!(
            import_project(
                &library,
                &source,
                "Ambiguous",
                &runner,
                &OperationContext::default()
            )
            .is_err()
        );
        assert_eq!(library.info().unwrap().asset_count, 1);
        assert_eq!(
            fs::read(workspace.join("second.BLEND")).unwrap(),
            b"second source"
        );
        let variant = library.get_variant(&variant_id).unwrap().variant;
        assert!(library.get_snapshot(&variant.head_snapshot_id).is_ok());
    }

    #[test]
    fn missing_corrupt_or_incomplete_receipts_never_reuse_an_uncertified_glb() {
        let (_temporary, library, variant_id, mut receipt) = cached_project();
        let path = receipt_path(library.root(), &variant_id).unwrap();
        let ctx = OperationContext::default();
        let workspace = library.workspace_path(&variant_id).unwrap();
        let blend = fs::read(workspace.join("main.blend")).unwrap();
        let glb = fs::read(workspace.join("main.glb")).unwrap();
        fs::remove_file(&path).unwrap();
        assert!(synchronize_variant(&library, &variant_id, None, &ctx).is_err());
        fs::write(&path, b"incomplete receipt").unwrap();
        assert!(synchronize_variant(&library, &variant_id, None, &ctx).is_err());
        receipt.sources.clear();
        save_receipt(&library, &variant_id, &receipt).unwrap();
        assert!(synchronize_variant(&library, &variant_id, None, &ctx).is_err());
        assert_eq!(fs::read(workspace.join("main.blend")).unwrap(), blend);
        assert_eq!(fs::read(workspace.join("main.glb")).unwrap(), glb);
    }

    #[test]
    fn a_glb_changed_between_hash_and_copy_cannot_destroy_previous_data() {
        let (_temporary, library, variant_id, _receipt) = cached_project();
        let ctx = OperationContext::default();
        let source = library
            .workspace_path(&variant_id)
            .unwrap()
            .join("main.glb");
        fs::write(&source, b"unversioned original GLB").unwrap();
        let expected = hash_file(&source, &ctx).unwrap().unwrap();
        fs::write(&source, b"concurrently saved GLB").unwrap();
        let destination = library.root().join("cache/test.glb");
        fs::write(&destination, b"previous destination").unwrap();
        assert!(atomic_copy(&source, &destination, &expected, &ctx).is_err());
        assert_eq!(fs::read(&destination).unwrap(), b"previous destination");
        assert!(backup_unversioned_glb(&library, &variant_id, &source, &expected, &ctx).is_err());
        assert_eq!(fs::read(&source).unwrap(), b"concurrently saved GLB");
    }

    #[test]
    fn an_independent_blend_added_during_validation_remains_versionable() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let (_temporary, library, variant_id, receipt) = cached_project();
        let workspace = library.workspace_path(&variant_id).unwrap();
        let changed = Arc::new(AtomicBool::new(false));
        let flag = changed.clone();
        let extra = workspace.join("second.blend");
        let ctx = OperationContext {
            progress: Some(Arc::new(move |event| {
                if event.phase == "capture" && !flag.swap(true, Ordering::SeqCst) {
                    fs::write(&extra, b"second project source").unwrap();
                }
            })),
            ..Default::default()
        };
        assert!(
            finish_artifact(&library, &variant_id, receipt, false, &ctx)
                .unwrap()
                .source_changed
        );
        assert!(changed.load(Ordering::SeqCst));
        assert_eq!(
            fs::read(workspace.join("second.blend")).unwrap(),
            b"second project source"
        );
        assert_eq!(library.get_variant(&variant_id).unwrap().snapshots.len(), 1);
    }
}
