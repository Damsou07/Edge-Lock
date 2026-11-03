use crate::preview_resources::{self, PreviewResource, ThumbnailSource};
use crate::state::*;
use edge_lock_core::{
    ExportConfig, Library, OperationContext, Snapshot, StorageAction,
    blender::{BlenderRunner, Inspection},
    glb,
    preview::{self, PreviewOutcome, PreviewRequest},
    sync,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};
use tauri::{AppHandle, Emitter};

#[derive(Clone, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum JobRequest {
    StoragePlan {
        variant_id: String,
        action: StorageAction,
        snapshot_id: Option<String>,
    },
    StorageApply {
        variant_id: String,
        action: StorageAction,
        snapshot_id: Option<String>,
        plan_token: String,
    },
    Restore {
        variant_id: String,
        expected_head: String,
        expected_backup: String,
    },
    Import {
        source: String,
        name: String,
        config: ExportConfig,
    },
    Create {
        name: String,
    },
    Snapshot {
        variant_id: String,
        message: String,
    },
    Variant {
        snapshot_id: String,
        name: String,
    },
    Changes {
        variant_id: String,
    },
    Inspect {
        variant_id: String,
        snapshot_id: Option<String>,
    },
    Preview {
        variant_id: String,
        snapshot_id: Option<String>,
        preview_file: Option<String>,
    },
    Thumbnail {
        asset_id: String,
    },
    Export {
        variant_id: String,
        snapshot_id: Option<String>,
        destination: String,
        overwrite: bool,
    },
}
impl JobRequest {
    fn kind(&self) -> &str {
        match self {
            Self::StoragePlan { .. } => "storagePlan",
            Self::StorageApply { .. } => "storageApply",
            Self::Restore { .. } => "restore",
            Self::Import { .. } => "import",
            Self::Create { .. } => "create",
            Self::Snapshot { .. } => "snapshot",
            Self::Variant { .. } => "variant",
            Self::Changes { .. } => "changes",
            Self::Inspect { .. } => "inspect",
            Self::Preview { .. } => "preview",
            Self::Thumbnail { .. } => "thumbnail",
            Self::Export { .. } => "export",
        }
    }
}

pub fn start(app: AppHandle, state: Arc<AppState>, request: JobRequest) -> AppResult<String> {
    let id = uuid::Uuid::new_v4().to_string();
    let cancelled = Arc::new(AtomicBool::new(false));
    let library = {
        let _lifecycle = lock(&state.lifecycle_gate)?;
        state.ensure_running()?;
        let library = state.library()?;
        let mut jobs = lock(&state.jobs)?;
        if jobs
            .values()
            .filter(|j| matches!(j.job.status.as_str(), "queued" | "running"))
            .count()
            >= 32
        {
            return Err(AppError::new("busy", "La file de tâches est pleine."));
        }
        if jobs.len() > 200 {
            jobs.retain(|_, entry| matches!(entry.job.status.as_str(), "queued" | "running"));
        }
        jobs.insert(
            id.clone(),
            JobEntry {
                cancelled: cancelled.clone(),
                job: Job {
                    id: id.clone(),
                    kind: request.kind().into(),
                    status: "queued".into(),
                    phase: "En attente".into(),
                    completed: 0,
                    total: None,
                    result: Value::Null,
                    error: None,
                },
            },
        );
        library
    };
    let job_id = id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let progress_state = state.clone();
        let progress_app = app.clone();
        let progress_id = job_id.clone();
        let ctx = OperationContext {
            cancelled,
            progress: Some(Arc::new(move |progress| {
                update_job(&progress_app, &progress_state, &progress_id, |job| {
                    job.phase = progress.phase;
                    job.completed = progress.completed;
                    job.total = progress.total;
                });
            })),
        };
        let result = (|| {
            let _gate = lock(&state.operation_gate)?;
            ctx.check_cancelled()?;
            if !Arc::ptr_eq(&library, &state.library()?) {
                return Err(AppError::new(
                    "library_changed",
                    "La bibliothèque a changé. Relancez cette opération.",
                ));
            }
            update_job(&app, &state, &job_id, |job| {
                job.status = "running".into();
                job.phase = "Préparation".into();
            });
            run(&state, &library, &request, &ctx)
        })();
        let refresh_variants: Vec<String> = match (&request, &result) {
            (JobRequest::Import { .. } | JobRequest::Create { .. }, Ok(value)) => value["variants"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|variant| variant["id"].as_str().map(str::to_owned))
                .collect(),
            (JobRequest::Variant { .. }, Ok(value)) => value["id"]
                .as_str()
                .map(str::to_owned)
                .into_iter()
                .collect(),
            (JobRequest::Snapshot { variant_id, .. }, Ok(_)) => vec![variant_id.clone()],
            (JobRequest::Restore { variant_id, .. }, _) => vec![variant_id.clone()],
            _ => Vec::new(),
        };
        update_job(&app, &state, &job_id, |job| match result {
            Ok(value) => {
                job.status = "completed".into();
                job.phase = "Terminé".into();
                job.result = value;
            }
            Err(error) => {
                job.status = if error.code == "cancelled" {
                    "cancelled"
                } else {
                    "failed"
                }
                .into();
                job.phase = error.message.clone();
                job.error = Some(error);
            }
        });
        for variant_id in refresh_variants {
            let _ = crate::workspace::request(&app, &state, &variant_id);
        }
    });
    Ok(id)
}

fn update_job(app: &AppHandle, state: &AppState, id: &str, update: impl FnOnce(&mut Job)) {
    let payload = if let Ok(mut jobs) = state.jobs.lock() {
        jobs.get_mut(id).map(|entry| {
            update(&mut entry.job);
            entry.job.clone()
        })
    } else {
        None
    };
    if let Some(job) = payload {
        let _ = app.emit("edge-lock://job", job);
    }
}

fn runner(state: &AppState) -> AppResult<BlenderRunner> {
    let path = lock(&state.settings)?
        .blender_path
        .clone()
        .ok_or_else(|| AppError::new("blender_missing", "Configurez Blender dans les réglages."))?;
    Ok(BlenderRunner::new(PathBuf::from(path)))
}

fn optional_runner(state: &AppState) -> AppResult<Option<BlenderRunner>> {
    Ok(lock(&state.settings)?
        .blender_path
        .as_ref()
        .map(|path| BlenderRunner::new(path.into())))
}

fn current_preview(
    state: &AppState,
    library: &Library,
    variant_id: &str,
    ctx: &OperationContext,
) -> AppResult<Box<preview::PreviewArtifact>> {
    let artifact =
        sync::synchronize_variant(library, variant_id, optional_runner(state)?.as_ref(), ctx)?;
    // Viewer budgets do not limit the canonical export or the version history.
    glb::guard_preview(&artifact.path)?;
    Ok(Box::new(preview::PreviewArtifact {
        path: artifact.path,
        artifact_key: artifact.artifact_key,
        inspection: artifact.inspection,
        source_file: artifact.source_file,
        source_kind: preview::PreviewKind::GeneratedGlb,
        updates_geometry: true,
    }))
}

fn run(
    state: &AppState,
    library: &Arc<Library>,
    request: &JobRequest,
    ctx: &OperationContext,
) -> AppResult<Value> {
    match request {
        JobRequest::StoragePlan {
            variant_id,
            action,
            snapshot_id,
        } => Ok(serde_json::to_value(library.preview_storage_action(
            variant_id,
            *action,
            snapshot_id.as_deref(),
            ctx,
        )?)?),
        JobRequest::StorageApply {
            variant_id,
            action,
            snapshot_id,
            plan_token,
        } => Ok(serde_json::to_value(library.apply_storage_action(
            variant_id,
            *action,
            snapshot_id.as_deref(),
            plan_token,
            ctx,
        )?)?),
        JobRequest::Restore {
            variant_id,
            expected_head,
            expected_backup,
        } => Ok(serde_json::to_value(library.restore_backup(
            variant_id,
            expected_head,
            expected_backup,
            ctx,
        )?)?),
        JobRequest::Import {
            source,
            name,
            config,
        } => {
            // Retain the old IPC field for callers; the canonical policy owns export settings.
            let _ = config;
            Ok(serde_json::to_value(sync::import_project(
                library,
                Path::new(source),
                name,
                &runner(state)?,
                ctx,
            )?)?)
        }
        JobRequest::Create { name } => {
            let project = tempfile::tempdir()?;
            for directory in ["textures", "references", "reports", "screenshots"] {
                fs::create_dir(project.path().join(directory))?;
            }
            runner(state)?.create_project(project.path(), ctx)?;
            Ok(serde_json::to_value(sync::import_project(
                library,
                project.path(),
                name,
                &runner(state)?,
                ctx,
            )?)?)
        }
        JobRequest::Snapshot {
            variant_id,
            message,
        } => Ok(serde_json::to_value(sync::create_snapshot(
            library,
            variant_id,
            message,
            optional_runner(state)?.as_ref(),
            ctx,
        )?)?),
        JobRequest::Variant { snapshot_id, name } => Ok(serde_json::to_value(
            sync::create_variant(library, snapshot_id, name, &runner(state)?, ctx)?,
        )?),
        JobRequest::Changes { variant_id } => {
            Ok(serde_json::to_value(library.changes(variant_id, ctx)?)?)
        }
        JobRequest::Inspect {
            variant_id,
            snapshot_id,
        } => {
            if snapshot_id.is_none() {
                let artifact = sync::synchronize_variant(
                    library,
                    variant_id,
                    optional_runner(state)?.as_ref(),
                    ctx,
                )?;
                return Ok(serde_json::to_value(artifact.inspection)?);
            }
            let material = prepare(library, variant_id, snapshot_id.as_deref(), ctx)?;
            let inspection = inspect(state, &material.path, &material.snapshot.config, ctx)?;
            if snapshot_id.is_none() {
                library.set_geometry(variant_id, inspection.geometry.triangles)?;
            }
            Ok(serde_json::to_value(inspection)?)
        }
        JobRequest::Preview {
            variant_id,
            snapshot_id,
            preview_file,
        } => {
            let blender_path = lock(&state.settings)?.blender_path.clone();
            let artifact = if snapshot_id.is_none() {
                current_preview(state, library, variant_id, ctx)?
            } else {
                match preview::prepare_preview(
                    library,
                    PreviewRequest {
                        variant_id,
                        snapshot_id: snapshot_id.as_deref(),
                        preview_file: preview_file.as_deref(),
                        blender_path: blender_path.as_deref().map(Path::new),
                    },
                    ctx,
                )? {
                    PreviewOutcome::Ready(artifact) => artifact,
                    PreviewOutcome::Choose(candidates) => {
                        let mut error = AppError::new(
                            "preview_ambiguous",
                            "Plusieurs GLB sont disponibles. Choisissez le fichier d’aperçu.",
                        );
                        error.details = Some(json!({ "candidates": candidates }));
                        return Err(error);
                    }
                }
            };
            if snapshot_id.is_none() && artifact.updates_geometry {
                library.set_geometry(variant_id, artifact.inspection.geometry.triangles)?;
            }
            let resource = preview_resources::register(
                state,
                library,
                PreviewResource::glb(artifact.path.clone(), None),
            )?;
            trim_cache(&library.root().join("cache"), &artifact.path, state);
            Ok(
                json!({ "url": resource.url, "resourceToken":resource.resource_token,"artifactKey":artifact.artifact_key,"inspection": artifact.inspection,
                    "sourceLabel": if snapshot_id.is_some() { "Version enregistrée" } else { "Travail enregistré sur disque" },
                    "sourceFile": artifact.source_file, "sourceKind": artifact.source_kind }),
            )
        }
        JobRequest::Thumbnail { asset_id } => {
            let variant = library
                .get_asset(asset_id)?
                .variants
                .into_iter()
                .next()
                .ok_or_else(|| {
                    AppError::new("not_found", "Cet asset ne contient aucune variante.")
                })?;
            let artifact = current_preview(state, library, &variant.id, ctx)?;
            ctx.check_cancelled()?;
            let source = ThumbnailSource {
                variant_id: variant.id,
                artifact_key: artifact.artifact_key.clone(),
            };
            if let Some(path) = preview_resources::find_thumbnail(
                library.root(),
                &source.variant_id,
                Some(&source.artifact_key),
            )? {
                let resource = preview_resources::register(
                    state,
                    library,
                    PreviewResource::png(path.clone(), source),
                )?;
                trim_cache(&library.root().join("cache"), &path, state);
                return Ok(
                    json!({"state":"ready","url":resource.url,"resourceToken":resource.resource_token}),
                );
            }
            let resource = preview_resources::register(
                state,
                library,
                PreviewResource::glb(artifact.path.clone(), Some(source)),
            )?;
            trim_cache(&library.root().join("cache"), &artifact.path, state);
            Ok(
                json!({"state":"render","resourceToken":resource.resource_token,"preview":{
                    "url":resource.url,"resourceToken":resource.resource_token,"artifactKey":artifact.artifact_key,
                    "inspection":artifact.inspection,"sourceLabel":"Travail enregistré sur disque",
                    "sourceFile":artifact.source_file,"sourceKind":artifact.source_kind
                }}),
            )
        }
        JobRequest::Export {
            variant_id,
            snapshot_id,
            destination,
            overwrite,
        } => {
            let destination = validate_export_path(library, Path::new(destination))?;
            if destination.exists() && !overwrite {
                return Err(AppError::new(
                    "destination_exists",
                    "Ce fichier existe déjà. Confirmez son remplacement.",
                ));
            }
            if let Some(id) = snapshot_id {
                let snapshot = library.get_snapshot(id)?;
                if library.get_variant(&snapshot.variant_id)?.variant.asset_id
                    != library.get_variant(variant_id)?.variant.asset_id
                {
                    return Err(AppError::new(
                        "invalid",
                        "Cette version appartient à un autre asset.",
                    ));
                }
                // Historical export serves the saved artifact, without regenerating old sources.
                let stored_glb = library.snapshot_glb_path(id);
                if snapshot.storage.preview_path.is_some()
                    || snapshot.storage.kind == "preview"
                    || stored_glb.is_ok()
                {
                    let relative = stored_glb?;
                    let temp = tempfile::tempdir_in(library.root().join("staging"))?;
                    let path = temp.path().join("historical.glb");
                    library.extract_snapshot_file(id, &relative, &path, ctx)?;
                    glb::validate_glb(&path)?;
                    publish_export(&path, &destination, *overwrite, ctx)?;
                    return Ok(json!({"path":destination.to_string_lossy()}));
                }
            }
            let material = prepare(library, variant_id, snapshot_id.as_deref(), ctx)?;
            let (path, _) = derived_glb(state, library, &material, ctx)?;
            ctx.check_cancelled()?;
            publish_export(&path, &destination, *overwrite, ctx)?;
            Ok(json!({ "path": destination.to_string_lossy() }))
        }
    }
}

struct Material {
    _temporary: tempfile::TempDir,
    path: PathBuf,
    snapshot: Snapshot,
}
fn prepare(
    library: &Library,
    variant_id: &str,
    snapshot_id: Option<&str>,
    ctx: &OperationContext,
) -> AppResult<Material> {
    let root = library.root();
    let temporary = tempfile::Builder::new()
        .prefix("derived-")
        .tempdir_in(root.join("staging"))?;
    let path = temporary.path().join("source");
    let mut snapshot = if let Some(id) = snapshot_id {
        let snapshot = library.get_snapshot(id)?;
        let source_asset = library.get_variant(&snapshot.variant_id)?.variant.asset_id;
        if source_asset != library.get_variant(variant_id)?.variant.asset_id {
            return Err(AppError::new(
                "invalid",
                "Cette version appartient à un autre asset.",
            ));
        }
        library.materialize(id, &path, ctx)?;
        snapshot
    } else {
        library.capture_working(variant_id, &path, ctx)?
    };
    if snapshot_id.is_none() {
        let layout = sync::validate_project(&snapshot.entries)?;
        snapshot.config = ExportConfig {
            primary_file: Some(layout.blend_file),
            scene: None,
            collection: None,
            apply_modifiers: true,
        };
    }
    Ok(Material {
        _temporary: temporary,
        path,
        snapshot,
    })
}

fn primary(workspace: &Path, config: &ExportConfig) -> AppResult<PathBuf> {
    let relative = config.primary_file.as_deref().ok_or_else(|| {
        AppError::new(
            "primary_missing",
            "Choisissez un fichier principal .blend ou .glb dans les réglages de la variante.",
        )
    })?;
    let path = fs::canonicalize(workspace.join(relative))?;
    if !path.starts_with(fs::canonicalize(workspace)?) {
        return Err(AppError::new(
            "invalid",
            "Le fichier principal doit appartenir au dossier de l’asset.",
        ));
    }
    Ok(path)
}

fn inspect(
    state: &AppState,
    workspace: &Path,
    config: &ExportConfig,
    ctx: &OperationContext,
) -> AppResult<Inspection> {
    let path = primary(workspace, config)?;
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("glb"))
    {
        let stats = glb::inspect_glb(&path)?;
        Ok(Inspection {
            geometry: stats.geometry,
            scenes: vec![],
            collections: vec![],
            dependencies: vec![],
            warnings: vec![],
            blender_version: "GLB 2.0".into(),
        })
    } else {
        Ok(runner(state)?.inspect(workspace, config, ctx)?)
    }
}

fn derived_glb(
    state: &AppState,
    library: &Library,
    material: &Material,
    ctx: &OperationContext,
) -> AppResult<(PathBuf, Inspection)> {
    let primary = primary(&material.path, &material.snapshot.config)?;
    let direct_glb = primary
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("glb"));
    let blender = if direct_glb {
        None
    } else {
        Some(runner(state)?)
    };
    let version = if let Some(ref runner) = blender {
        serde_json::to_string(&runner.probe()?)?
    } else {
        "glb2".into()
    };
    let mut hasher = blake3::Hasher::new();
    hasher.update(env!("CARGO_PKG_VERSION").as_bytes());
    hasher.update(include_bytes!("../../blender/edge_lock.py"));
    hasher.update(version.as_bytes());
    hasher.update(&serde_json::to_vec(&material.snapshot.config)?);
    hasher.update(&serde_json::to_vec(&material.snapshot.entries)?);
    let key = hasher.finalize().to_hex().to_string();
    let cache = managed_cache_dir(library.root())?;
    let cached = cache.join(format!("{key}.glb"));
    let metadata = cache.join(format!("{key}.json"));
    if cached.is_file()
        && metadata.is_file()
        && fs::symlink_metadata(&metadata)?.len() <= 4 * 1024 * 1024
        && !is_link(&fs::symlink_metadata(&cached)?)
        && !is_link(&fs::symlink_metadata(&metadata)?)
        && glb::validate_glb(&cached).is_ok()
        && let Ok(inspection) = serde_json::from_slice::<Inspection>(&fs::read(&metadata)?)
    {
        return Ok((cached, inspection));
    }
    ctx.report("Préparation du GLB", 0, None);
    let staged = material._temporary.path().join("preview.glb");
    let inspection = if let Some(blender) = blender {
        blender.export_glb(&material.path, &material.snapshot.config, &staged, ctx)?
    } else {
        let stats = glb::inspect_glb(&primary)?;
        copy_stream(&primary, &staged, ctx)?;
        Inspection {
            geometry: stats.geometry,
            scenes: vec![],
            collections: vec![],
            dependencies: vec![],
            warnings: vec![],
            blender_version: "GLB 2.0".into(),
        }
    };
    glb::validate_glb(&staged)?;
    ctx.check_cancelled()?;
    let cache_temp = tempfile::NamedTempFile::new_in(&cache)?;
    copy_stream(&staged, cache_temp.path(), ctx)?;
    cache_temp
        .persist(&cached)
        .map_err(|e| AppError::new("io", e))?;
    let mut meta_temp = tempfile::NamedTempFile::new_in(&cache)?;
    meta_temp.write_all(&serde_json::to_vec(&inspection)?)?;
    meta_temp.as_file().sync_all()?;
    meta_temp
        .persist(&metadata)
        .map_err(|e| AppError::new("io", e))?;
    trim_cache(&library.root().join("cache"), &cached, state);
    Ok((cached, inspection))
}

fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn managed_cache_dir(root: &Path) -> AppResult<PathBuf> {
    let mut path = root.to_owned();
    for component in ["cache", "exports"] {
        path.push(component);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() && !is_link(&metadata) => {}
            Ok(_) => {
                return Err(AppError::new(
                    "invalid",
                    "Le cache doit contenir des dossiers ordinaires, sans lien ni jonction.",
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir(&path)?,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(path)
}

pub(crate) fn trim_cache(cache: &Path, keep: &Path, state: &AppState) {
    let protected: Vec<PathBuf> = state.previews.lock().map(|p| p.paths()).unwrap_or_default();
    trim_cache_to_budget(cache, keep, &protected, 2 * 1024 * 1024 * 1024);
}

fn trim_cache_to_budget(cache: &Path, keep: &Path, protected: &[PathBuf], budget: u64) {
    let entries = [
        cache.to_path_buf(),
        cache.join("previews"),
        cache.join("exports"),
        cache.join("thumbnails"),
        cache.join("canonical"),
    ]
    .into_iter()
    .filter(|p| fs::symlink_metadata(p).is_ok_and(|m| m.is_dir() && !is_link(&m)))
    .filter_map(|p| fs::read_dir(p).ok())
    .flatten();
    let mut files: Vec<_> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let meta = fs::symlink_metadata(&path).ok()?;
            (meta.is_file()
                && !is_link(&meta)
                && path.extension().is_some_and(|e| e == "glb" || e == "png"))
            .then_some((path, meta.len(), meta.modified().ok()))
        })
        .collect();
    let mut total: u64 = files.iter().map(|(_, size, _)| size).sum();
    files.sort_by_key(|(_, _, modified)| *modified);
    for (path, bytes, _) in files {
        if total <= budget {
            break;
        }
        if path == keep || protected.contains(&path) {
            continue;
        }
        if fs::remove_file(&path).is_ok() {
            let _ = fs::remove_file(path.with_extension("json"));
            total = total.saturating_sub(bytes);
        }
    }
}

fn copy_stream(source: &Path, destination: &Path, ctx: &OperationContext) -> AppResult<()> {
    let mut source = fs::File::open(source)?;
    let mut target = fs::File::create(destination)?;
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        ctx.check_cancelled()?;
        let count = source.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        target.write_all(&buffer[..count])?;
    }
    target.sync_all()?;
    Ok(())
}

fn validate_export_path(library: &Library, destination: &Path) -> AppResult<PathBuf> {
    if !destination.is_absolute()
        || !destination
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("glb"))
    {
        return Err(AppError::new(
            "invalid",
            "Choisissez un chemin absolu avec l’extension .glb.",
        ));
    }
    let parent = fs::canonicalize(
        destination
            .parent()
            .ok_or_else(|| AppError::new("invalid", "Dossier d’export absent"))?,
    )?;
    let root = fs::canonicalize(library.root())?;
    if parent.starts_with(root) {
        return Err(AppError::new(
            "invalid",
            "Exportez hors de la bibliothèque pour préserver les sources et l’historique.",
        ));
    }
    Ok(parent.join(
        destination
            .file_name()
            .ok_or_else(|| AppError::new("invalid", "Nom d’export absent"))?,
    ))
}

fn publish_export(
    source: &Path,
    destination: &Path,
    overwrite: bool,
    ctx: &OperationContext,
) -> AppResult<()> {
    let temporary = tempfile::NamedTempFile::new_in(
        destination
            .parent()
            .ok_or_else(|| AppError::new("invalid", "Dossier d’export absent"))?,
    )?;
    copy_stream(source, temporary.path(), ctx)?;
    ctx.check_cancelled()?;
    if overwrite {
        temporary.persist(destination)
    } else {
        temporary.persist_noclobber(destination)
    }
    .map_err(|e| AppError::new("io", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;
    #[test]
    fn cache_never_replaces_an_unrelated_file() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("cache"), b"keep").unwrap();
        assert!(managed_cache_dir(directory.path()).is_err());
        assert_eq!(fs::read(directory.path().join("cache")).unwrap(), b"keep");
    }
    #[test]
    fn export_does_not_replace_without_consent_or_after_cancellation() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.glb");
        let dest = temp.path().join("output.glb");
        fs::write(&source, b"new").unwrap();
        fs::write(&dest, b"old").unwrap();
        let ctx = OperationContext::default();
        assert!(publish_export(&source, &dest, false, &ctx).is_err());
        assert_eq!(fs::read(&dest).unwrap(), b"old");
        ctx.cancelled.store(true, Ordering::Relaxed);
        assert!(publish_export(&source, &dest, true, &ctx).is_err());
        assert_eq!(fs::read(&dest).unwrap(), b"old");
        ctx.cancelled.store(false, Ordering::Relaxed);
        publish_export(&source, &dest, true, &ctx).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"new");
    }
    #[test]
    fn request_wire_contract_uses_camel_case() {
        let request: JobRequest =
            serde_json::from_value(json!({"kind":"snapshot","variantId":"abc","message":"v2"}))
                .unwrap();
        assert!(matches!(request, JobRequest::Snapshot { variant_id, .. } if variant_id == "abc"));
        let thumbnail: JobRequest =
            serde_json::from_value(json!({"kind":"thumbnail","assetId":"asset"})).unwrap();
        assert!(matches!(thumbnail, JobRequest::Thumbnail {asset_id} if asset_id == "asset"));
    }

    #[test]
    fn cache_budget_counts_thumbnails_and_keeps_active_resources() {
        let root = tempfile::tempdir().unwrap();
        let cache = root.path().join("cache");
        fs::create_dir_all(cache.join("thumbnails")).unwrap();
        fs::create_dir_all(cache.join("previews")).unwrap();
        fs::create_dir_all(cache.join("canonical")).unwrap();
        let active = cache.join("thumbnails/active.png");
        let stale = cache.join("thumbnails/stale.png");
        let current = cache.join("previews/current.glb");
        let old_canonical = cache.join("canonical/previous.glb");
        for path in [&active, &stale, &current, &old_canonical] {
            fs::write(path, [1; 50]).unwrap();
        }
        fs::write(cache.join("keep.txt"), b"personal").unwrap();
        trim_cache_to_budget(&cache, &current, std::slice::from_ref(&active), 100);
        assert!(active.is_file());
        assert!(current.is_file());
        assert!(!stale.exists());
        assert!(!old_canonical.exists());
        assert_eq!(fs::read(cache.join("keep.txt")).unwrap(), b"personal");
    }

    #[test]
    fn nonconforming_legacy_asset_keeps_its_cached_thumbnail_and_historical_glb() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        fs::create_dir(&source).unwrap();
        let mut json = serde_json::to_vec(&json!({
            "asset":{"version":"2.0"},"buffers":[{"byteLength":36}],
            "bufferViews":[{"buffer":0,"byteLength":36}],
            "accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"}],
            "meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}],
            "nodes":[{"mesh":0}],"scenes":[{"nodes":[0]}]
        }))
        .unwrap();
        while !json.len().is_multiple_of(4) {
            json.push(b' ');
        }
        let mut glb = b"glTF".to_vec();
        glb.extend(2u32.to_le_bytes());
        glb.extend(((12 + 8 + json.len() + 8 + 36) as u32).to_le_bytes());
        glb.extend((json.len() as u32).to_le_bytes());
        glb.extend(0x4e4f534au32.to_le_bytes());
        glb.extend(json);
        glb.extend(36u32.to_le_bytes());
        glb.extend(0x004e4942u32.to_le_bytes());
        glb.extend([0; 36]);
        fs::write(source.join("triangle.glb"), &glb).unwrap();
        let library = Arc::new(Library::open(root.path().join("library")).unwrap());
        let ctx = OperationContext::default();
        let asset = library
            .import_folder(&source, "Triangle", ExportConfig::default(), &ctx)
            .unwrap();
        let state = crate::state::test_state(library.clone());
        assert!(
            preview_resources::cached_asset_resources(
                &library,
                std::slice::from_ref(&asset.asset.id)
            )
            .unwrap()
            .is_empty()
        );
        let request = JobRequest::Thumbnail {
            asset_id: asset.asset.id.clone(),
        };
        assert!(run(&state, &library, &request, &ctx).is_err());
        let PreviewOutcome::Ready(artifact) = preview::prepare_preview(
            &library,
            PreviewRequest {
                variant_id: &asset.variants[0].id,
                snapshot_id: Some(&asset.variants[0].head_snapshot_id),
                preview_file: None,
                blender_path: None,
            },
            &ctx,
        )
        .unwrap() else {
            panic!("historical GLB remains readable")
        };
        let generated = preview_resources::register(
            &state,
            &library,
            PreviewResource::glb(
                artifact.path,
                Some(ThumbnailSource {
                    variant_id: asset.variants[0].id.clone(),
                    artifact_key: artifact.artifact_key,
                }),
            ),
        )
        .unwrap();
        let token = generated.resource_token.as_str();
        assert_eq!(
            lock(&state.previews)
                .unwrap()
                .get(token)
                .unwrap()
                .read()
                .unwrap(),
            glb
        );
        let mut png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png, 512, 384);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&vec![128; 512 * 384 * 4])
                .unwrap();
        }
        let stored = preview_resources::store(&state, &library, token, &png).unwrap();
        assert!(run(&state, &library, &request, &ctx).is_err());
        assert!(
            lock(&state.previews)
                .unwrap()
                .get(&stored.resource_token)
                .is_some()
        );
        assert!(lock(&state.previews).unwrap().get(token).is_some());
        assert!(state.watcher.lock().unwrap().is_none());
        assert_eq!(state.watcher_generation.load(Ordering::SeqCst), 0);
        // The immediate catalog path does not inspect the now-unavailable source GLB.
        fs::remove_file(Path::new(&asset.variants[0].workspace_path).join("triangle.glb")).unwrap();
        let cached = preview_resources::cached_asset_resources(
            &library,
            std::slice::from_ref(&asset.asset.id),
        )
        .unwrap();
        assert_eq!(cached[&asset.asset.id].mime(), "image/png");
        assert!(
            preview_resources::cached_asset_resources(&library, &vec![asset.asset.id; 65]).is_err()
        );
        assert_eq!(fs::read(source.join("triangle.glb")).unwrap(), glb);
    }
}
