use crate::{
    jobs::{self, JobRequest},
    preview_resources::{self, ResourceUrl},
    settings,
    state::*,
    workspace,
};
use edge_lock_core::{
    AssetDetail, AssetFilter, AssetPage, ExportConfig, FileSelectionKind, FileSelections, Library,
    LibraryInfo, Taxonomy, VariantDetail, blender::BlenderRunner,
};
use serde::Serialize;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
};
use tauri::{AppHandle, State};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

async fn blocking<T: Send + 'static>(
    operation: impl FnOnce() -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|e| AppError::new("internal", e))?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bootstrap {
    settings: settings::Settings,
    library: Option<LibraryInfo>,
    startup_error: Option<String>,
    version: &'static str,
}

#[tauri::command]
pub async fn get_bootstrap(state: State<'_, ManagedState>) -> AppResult<Bootstrap> {
    let state = state.0.clone();
    blocking(move || {
        Ok(Bootstrap {
            settings: lock(&state.settings)?.clone(),
            library: lock(&state.library)?
                .as_ref()
                .map(|l| l.info())
                .transpose()?,
            startup_error: lock(&state.startup_error)?.clone(),
            version: env!("CARGO_PKG_VERSION"),
        })
    })
    .await
}

#[tauri::command]
pub async fn open_library(
    path: String,
    app: AppHandle,
    state: State<'_, ManagedState>,
) -> AppResult<LibraryInfo> {
    let state = state.0.clone();
    blocking(move || {
        let _gate = lock(&state.operation_gate)?;
        let _lifecycle = lock(&state.lifecycle_gate)?;
        state.ensure_running()?;
        if state.busy()? {
            return Err(AppError::new(
                "busy",
                "Attendez la fin des tâches avant de changer de bibliothèque.",
            ));
        }
        if let Some(current) = lock(&state.library)?.as_ref()
            && std::fs::canonicalize(&path).ok() == std::fs::canonicalize(current.root()).ok()
        {
            return Ok(current.info()?);
        }
        let library = Arc::new(Library::open(PathBuf::from(path))?);
        let _ = edge_lock_core::preview::cleanup_temporary(library.root());
        let info = library.info()?;
        let mut changed = lock(&state.settings)?.clone();
        changed.library_path = Some(info.root.clone());
        settings::save(&state.settings_file, &changed).map_err(|e| AppError::new("settings", e))?;
        *lock(&state.settings)? = changed;
        state.watcher_generation.fetch_add(1, Ordering::SeqCst);
        *lock(&state.watcher)? = None;
        {
            let mut current = lock(&state.library)?;
            let mut previews = lock(&state.previews)?;
            *current = Some(library);
            previews.clear();
        }
        *lock(&state.startup_error)? = None;
        drop(_lifecycle);
        drop(_gate);
        workspace::install(&app, &state);
        Ok(info)
    })
    .await
}

#[tauri::command]
pub async fn list_assets(
    filter: AssetFilter,
    state: State<'_, ManagedState>,
) -> AppResult<AssetPage> {
    let library = state.0.library()?;
    blocking(move || Ok(library.list_assets(filter)?)).await
}
#[tauri::command]
pub async fn get_asset(id: String, state: State<'_, ManagedState>) -> AppResult<AssetDetail> {
    let library = state.0.library()?;
    blocking(move || Ok(library.get_asset(&id)?)).await
}
#[tauri::command]
pub async fn get_snapshot(
    id: String,
    state: State<'_, ManagedState>,
) -> AppResult<edge_lock_core::Snapshot> {
    let library = state.0.library()?;
    blocking(move || Ok(library.get_snapshot(&id)?)).await
}
#[tauri::command]
pub async fn list_snapshots(
    variant_id: String,
    offset: u32,
    limit: u32,
    state: State<'_, ManagedState>,
) -> AppResult<edge_lock_core::SnapshotPage> {
    let library = state.0.library()?;
    blocking(move || Ok(library.list_snapshots(&variant_id, offset, limit)?)).await
}
#[tauri::command]
pub async fn get_variant(id: String, state: State<'_, ManagedState>) -> AppResult<VariantDetail> {
    let library = state.0.library()?;
    blocking(move || Ok(library.get_variant(&id)?)).await
}

#[tauri::command]
pub async fn get_workspace_status(
    variant_id: String,
    app: AppHandle,
    state: State<'_, ManagedState>,
) -> AppResult<workspace::WorkspaceStatus> {
    workspace::status(&app, &state.0, &variant_id)
}

#[tauri::command]
pub async fn request_workspace_sync(
    variant_id: String,
    app: AppHandle,
    state: State<'_, ManagedState>,
) -> AppResult<workspace::WorkspaceStatus> {
    workspace::request(&app, &state.0, &variant_id)
}
#[tauri::command]
pub async fn get_taxonomy(state: State<'_, ManagedState>) -> AppResult<Taxonomy> {
    let library = state.0.library()?;
    blocking(move || Ok(library.taxonomy()?)).await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn update_asset(
    id: String,
    name: String,
    category_id: Option<String>,
    tags: Vec<String>,
    game_ids: Vec<String>,
    archived: bool,
    state: State<'_, ManagedState>,
) -> AppResult<()> {
    let library = state.0.library()?;
    blocking(move || {
        Ok(library.update_asset(&id, &name, category_id.as_deref(), tags, game_ids, archived)?)
    })
    .await
}
#[tauri::command]
pub async fn save_category(
    id: Option<String>,
    name: String,
    parent_id: Option<String>,
    state: State<'_, ManagedState>,
) -> AppResult<()> {
    let library = state.0.library()?;
    blocking(move || {
        library.save_category(id.as_deref(), &name, parent_id.as_deref())?;
        Ok(())
    })
    .await
}
#[tauri::command]
pub async fn save_game(
    id: Option<String>,
    name: String,
    state: State<'_, ManagedState>,
) -> AppResult<()> {
    let library = state.0.library()?;
    blocking(move || {
        library.save_game(id.as_deref(), &name)?;
        Ok(())
    })
    .await
}
#[tauri::command]
pub async fn delete_category(id: String, state: State<'_, ManagedState>) -> AppResult<u64> {
    let state = state.0.clone();
    let pending = state.register_config_write()?;
    let library = state.library()?;
    blocking(move || {
        let _pending = pending;
        let _gate = lock(&state.operation_gate)?;
        state.ensure_running()?;
        if !Arc::ptr_eq(&library, &state.library()?) {
            return Err(AppError::new(
                "library_changed",
                "La bibliothèque a changé.",
            ));
        }
        Ok(library.delete_category(&id)?)
    })
    .await
}
#[tauri::command]
pub async fn delete_game(id: String, state: State<'_, ManagedState>) -> AppResult<u64> {
    let state = state.0.clone();
    let pending = state.register_config_write()?;
    let library = state.library()?;
    blocking(move || {
        let _pending = pending;
        let _gate = lock(&state.operation_gate)?;
        state.ensure_running()?;
        if !Arc::ptr_eq(&library, &state.library()?) {
            return Err(AppError::new(
                "library_changed",
                "La bibliothèque a changé.",
            ));
        }
        Ok(library.delete_game(&id)?)
    })
    .await
}
#[tauri::command]
pub async fn update_config(
    variant_id: String,
    config: ExportConfig,
    state: State<'_, ManagedState>,
) -> AppResult<()> {
    let state = state.0.clone();
    let pending = state.register_config_write()?;
    let library = state.library()?;
    blocking(move || {
        let _pending = pending;
        persist_config(&state, &library, &variant_id, config)
    })
    .await
}

async fn write_metadata(
    state: Arc<AppState>,
    operation: impl FnOnce(&Library) -> edge_lock_core::Result<()> + Send + 'static,
) -> AppResult<()> {
    let pending = state.register_config_write()?;
    let library = state.library()?;
    blocking(move || {
        let _pending = pending;
        persist_metadata(&state, &library, operation)
    })
    .await
}

fn persist_metadata(
    state: &AppState,
    library: &Arc<Library>,
    operation: impl FnOnce(&Library) -> edge_lock_core::Result<()>,
) -> AppResult<()> {
    let _gate = lock(&state.operation_gate)?;
    if !Arc::ptr_eq(library, &state.library()?) {
        return Err(AppError::new(
            "library_changed",
            "La bibliothèque a changé. Relancez cette opération.",
        ));
    }
    Ok(operation(library)?)
}

#[tauri::command]
pub async fn rename_snapshot(
    id: String,
    message: String,
    state: State<'_, ManagedState>,
) -> AppResult<()> {
    write_metadata(state.0.clone(), move |library| {
        library.rename_snapshot(&id, &message)
    })
    .await
}

#[tauri::command]
pub async fn rename_variant(
    id: String,
    name: String,
    state: State<'_, ManagedState>,
) -> AppResult<()> {
    write_metadata(state.0.clone(), move |library| {
        library.rename_variant(&id, &name)
    })
    .await
}

#[tauri::command]
pub async fn get_file_selections(
    variant_id: String,
    snapshot_id: Option<String>,
    state: State<'_, ManagedState>,
) -> AppResult<FileSelections> {
    let library = state.0.library()?;
    blocking(move || Ok(library.get_file_selections(&variant_id, snapshot_id.as_deref())?)).await
}

#[tauri::command]
pub async fn set_file_selection(
    variant_id: String,
    snapshot_id: Option<String>,
    kind: FileSelectionKind,
    file: Option<String>,
    state: State<'_, ManagedState>,
) -> AppResult<()> {
    write_metadata(state.0.clone(), move |library| {
        library.set_file_selection(&variant_id, snapshot_id.as_deref(), kind, file.as_deref())
    })
    .await
}

fn persist_config(
    state: &AppState,
    library: &Arc<Library>,
    variant_id: &str,
    config: ExportConfig,
) -> AppResult<()> {
    let _gate = lock(&state.operation_gate)?;
    if !Arc::ptr_eq(library, &state.library()?) {
        return Err(AppError::new(
            "library_changed",
            "La bibliothèque a changé. Relancez cette opération.",
        ));
    }
    Ok(library.update_config(variant_id, config)?)
}

#[tauri::command]
pub async fn choose_folder(app: AppHandle) -> AppResult<Option<String>> {
    blocking(move || {
        app.dialog()
            .file()
            .set_title("Choisir un dossier")
            .set_can_create_directories(true)
            .blocking_pick_folder()
            .map(|p| {
                p.into_path()
                    .map(|p| p.to_string_lossy().into_owned())
                    .map_err(|e| AppError::new("dialog", e))
            })
            .transpose()
    })
    .await
}
#[tauri::command]
pub async fn choose_export_path(
    suggested_name: String,
    app: AppHandle,
) -> AppResult<Option<String>> {
    blocking(move || {
        app.dialog()
            .file()
            .set_title("Exporter un GLB")
            .add_filter("glTF binaire", &["glb"])
            .set_file_name(suggested_name)
            .blocking_save_file()
            .map(|p| {
                p.into_path()
                    .map(|p| p.to_string_lossy().into_owned())
                    .map_err(|e| AppError::new("dialog", e))
            })
            .transpose()
    })
    .await
}
#[tauri::command]
pub async fn choose_blender_path(app: AppHandle) -> AppResult<Option<String>> {
    blocking(move || {
        app.dialog()
            .file()
            .set_title("Choisir blender.exe")
            .add_filter("Blender", &["exe"])
            .blocking_pick_file()
            .map(|p| {
                p.into_path()
                    .map(|p| p.to_string_lossy().into_owned())
                    .map_err(|e| AppError::new("dialog", e))
            })
            .transpose()
    })
    .await
}
#[tauri::command]
pub fn copy_text(text: String, app: AppHandle) -> AppResult<()> {
    app.clipboard()
        .write_text(text)
        .map_err(|e| AppError::new("clipboard", e))
}
#[tauri::command]
pub async fn set_blender_path(
    path: String,
    app: AppHandle,
    state: State<'_, ManagedState>,
) -> AppResult<String> {
    let state = state.0.clone();
    blocking(move || {
        let path = std::fs::canonicalize(path)?;
        if !path.is_file() {
            return Err(AppError::new("invalid", "Choisissez blender.exe."));
        }
        let version = BlenderRunner::new(path.clone()).probe()?;
        let _gate = lock(&state.operation_gate)?;
        let mut settings = lock(&state.settings)?.clone();
        settings.blender_path = Some(dunce::simplified(&path).to_string_lossy().into_owned());
        settings::save(&state.settings_file, &settings)
            .map_err(|e| AppError::new("settings", e))?;
        *lock(&state.settings)? = settings;
        drop(_gate);
        workspace::reconcile(&app, &state);
        Ok(version.version)
    })
    .await
}
#[tauri::command]
pub async fn open_workspace(
    variant_id: String,
    app: AppHandle,
    state: State<'_, ManagedState>,
) -> AppResult<()> {
    let library = state.0.library()?;
    blocking(move || {
        app.opener()
            .open_path(
                dunce::simplified(&library.workspace_path(&variant_id)?).to_string_lossy(),
                None::<String>,
            )
            .map_err(|e| AppError::new("opener", e))
    })
    .await
}
#[tauri::command]
pub async fn open_blender(variant_id: String, state: State<'_, ManagedState>) -> AppResult<()> {
    let state = state.0.clone();
    let library = state.library()?;
    blocking(move || {
        let detail = library.get_variant(&variant_id)?;
        let path = edge_lock_core::sync::resolve_project(
            std::path::Path::new(&detail.variant.workspace_path),
            &detail.files,
            &detail.variant.config,
        )
        .map(|layout| layout.blend_file)
        .or_else(|error| detail.variant.config.primary_file.clone().ok_or(error))?;
        let variant = detail.variant;
        let workspace = std::fs::canonicalize(variant.workspace_path)?;
        let primary = std::fs::canonicalize(workspace.join(path))?;
        if !primary.starts_with(&workspace)
            || !primary
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("blend"))
        {
            return Err(AppError::new(
                "invalid",
                "Le fichier principal doit être un .blend du dossier de travail.",
            ));
        }
        let blender = lock(&state.settings)?.blender_path.clone().ok_or_else(|| {
            AppError::new("blender_missing", "Configurez Blender dans les réglages.")
        })?;
        let mut child = std::process::Command::new(blender)
            .arg("--disable-autoexec")
            .arg(dunce::simplified(&primary))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    })
    .await
}

#[tauri::command]
pub fn start_job(
    request: JobRequest,
    app: AppHandle,
    state: State<'_, ManagedState>,
) -> AppResult<String> {
    jobs::start(app, state.0.clone(), request)
}
#[tauri::command]
pub fn get_job(id: String, state: State<'_, ManagedState>) -> AppResult<Job> {
    lock(&state.0.jobs)?
        .get(&id)
        .map(|e| e.job.clone())
        .ok_or_else(|| AppError::new("not_found", "Tâche introuvable"))
}
#[tauri::command]
pub fn cancel_job(id: String, state: State<'_, ManagedState>) -> AppResult<()> {
    let jobs = lock(&state.0.jobs)?;
    let entry = jobs
        .get(&id)
        .ok_or_else(|| AppError::new("not_found", "Tâche introuvable"))?;
    entry.cancelled.store(true, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub async fn get_asset_thumbnails(
    asset_ids: Vec<String>,
    state: State<'_, ManagedState>,
) -> AppResult<HashMap<String, ResourceUrl>> {
    if asset_ids.len() > 64 {
        return Err(AppError::new(
            "invalid",
            "Demandez au maximum 64 miniatures.",
        ));
    }
    let state = state.0.clone();
    let library = state.library()?;
    blocking(move || {
        // Serialize the cache read and token publication with permanent deletion.
        let _gate = lock(&state.operation_gate)?;
        let resources = preview_resources::cached_asset_resources(&library, &asset_ids)?;
        preview_resources::register_batch(&state, &library, resources)
    })
    .await
}

#[tauri::command]
pub async fn store_thumbnail(
    preview_token: String,
    png_bytes: Vec<u8>,
    state: State<'_, ManagedState>,
) -> AppResult<ResourceUrl> {
    let state = state.0.clone();
    let library = state.library()?;
    blocking(move || preview_resources::store(&state, &library, &preview_token, &png_bytes)).await
}

#[tauri::command]
pub fn release_preview_resources(
    tokens: Vec<String>,
    state: State<'_, ManagedState>,
) -> AppResult<()> {
    lock(&state.0.previews)?.release(&tokens)
}

#[tauri::command]
pub async fn invalidate_thumbnail(
    resource_token: String,
    state: State<'_, ManagedState>,
) -> AppResult<()> {
    let state = state.0.clone();
    blocking(move || {
        let _gate = lock(&state.operation_gate)?;
        let library = state.library()?;
        let Some(resource) = lock(&state.previews)?.get(&resource_token) else {
            return Ok(());
        };
        if !resource.is_thumbnail() {
            return Err(AppError::new(
                "invalid",
                "Ce jeton ne désigne pas une miniature.",
            ));
        }
        let source = resource
            .thumbnail
            .as_ref()
            .ok_or_else(|| AppError::new("invalid", "Origine de miniature absente."))?;
        preview_resources::remove_thumbnail(library.root(), source)?;
        lock(&state.previews)?.remove_path(&resource.path);
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use edge_lock_core::OperationContext;

    #[test]
    fn a_submitted_config_write_finishes_after_close_is_requested() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("main.blend"), b"source").unwrap();
        let library = Arc::new(Library::open(root.path().join("library")).unwrap());
        let asset = library
            .import_folder(
                &source,
                "Drone",
                ExportConfig::default(),
                &OperationContext::default(),
            )
            .unwrap();
        let variant = &asset.variants[0];
        let state = Arc::new(crate::state::test_state(library.clone()));
        let gate = lock(&state.operation_gate).unwrap();
        let pending = state.register_config_write().unwrap();
        let config = ExportConfig {
            apply_modifiers: true,
            ..variant.config.clone()
        };
        let write_state = state.clone();
        let write_library = library.clone();
        let variant_id = variant.id.clone();
        let submitted_config = config.clone();
        let worker = std::thread::spawn(move || {
            let _pending = pending;
            persist_config(&write_state, &write_library, &variant_id, submitted_config)
        });
        assert!(matches!(
            state.request_close().unwrap(),
            CloseDecision::Wait
        ));
        assert!(
            !library
                .get_variant(&variant.id)
                .unwrap()
                .variant
                .config
                .apply_modifiers
        );
        drop(gate);
        worker.join().unwrap().unwrap();
        assert_eq!(
            library.get_variant(&variant.id).unwrap().variant.config,
            config
        );
        assert_eq!(std::fs::read(source.join("main.blend")).unwrap(), b"source");
        assert!(!state.busy().unwrap());
    }

    #[test]
    fn a_config_write_refuses_a_library_that_changed_while_waiting() {
        let root = tempfile::tempdir().unwrap();
        let library = Arc::new(Library::open(root.path().join("previous")).unwrap());
        let current = Arc::new(Library::open(root.path().join("current")).unwrap());
        let state = Arc::new(crate::state::test_state(library.clone()));
        let _pending = state.register_config_write().unwrap();
        *lock(&state.library).unwrap() = Some(current);
        let error =
            persist_config(&state, &library, "unused", ExportConfig::default()).unwrap_err();
        assert_eq!(error.code, "library_changed");
    }

    #[test]
    fn metadata_writes_reject_a_library_switch_without_running_the_mutation() {
        let root = tempfile::tempdir().unwrap();
        let previous = Arc::new(Library::open(root.path().join("previous")).unwrap());
        let current = Arc::new(Library::open(root.path().join("current")).unwrap());
        let state = crate::state::test_state(current);
        let mut called = false;
        let error = persist_metadata(&state, &previous, |_| {
            called = true;
            Ok(())
        })
        .unwrap_err();
        assert_eq!(error.code, "library_changed");
        assert!(!called);
    }

    #[test]
    fn submitted_metadata_write_completes_before_shutdown() {
        let root = tempfile::tempdir().unwrap();
        let library = Arc::new(Library::open(root.path().join("library")).unwrap());
        let state = Arc::new(crate::state::test_state(library.clone()));
        let gate = lock(&state.operation_gate).unwrap();
        let pending = state.register_config_write().unwrap();
        let worker_state = state.clone();
        let worker_library = library.clone();
        let worker = std::thread::spawn(move || {
            let _pending = pending;
            persist_metadata(&worker_state, &worker_library, |library| {
                library.save_game(None, "Après fermeture").map(|_| ())
            })
        });
        assert!(matches!(
            state.request_close().unwrap(),
            CloseDecision::Wait
        ));
        drop(gate);
        worker.join().unwrap().unwrap();
        assert_eq!(library.taxonomy().unwrap().games[0].name, "Après fermeture");
        assert!(!state.busy().unwrap());
    }
}
