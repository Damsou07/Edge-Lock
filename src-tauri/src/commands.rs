use crate::{
    jobs::{self, JobRequest},
    settings,
    state::*,
};
use edge_lock_core::{
    AssetDetail, AssetFilter, AssetPage, ExportConfig, Library, LibraryInfo, Taxonomy,
    VariantDetail, blender::BlenderRunner,
};
use notify::Watcher;
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, State};
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
pub async fn open_library(path: String, state: State<'_, ManagedState>) -> AppResult<LibraryInfo> {
    let state = state.0.clone();
    blocking(move || {
        let _gate = lock(&state.operation_gate)?;
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
        *lock(&state.library)? = Some(library);
        lock(&state.previews)?.clear();
        *lock(&state.startup_error)? = None;
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
pub async fn get_variant(
    id: String,
    app: AppHandle,
    state: State<'_, ManagedState>,
) -> AppResult<VariantDetail> {
    let state = state.0.clone();
    let library = state.library()?;
    let generation = state.watcher_generation.fetch_add(1, Ordering::SeqCst) + 1;
    blocking(move || {
        let detail = library.get_variant(&id)?;
        let variant_id = id.clone();
        let mut last_event = Instant::now() - Duration::from_secs(1);
        let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            if let Ok(event) = event
                && !matches!(event.kind, notify::EventKind::Access(_))
                && last_event.elapsed() >= Duration::from_millis(500)
            {
                last_event = Instant::now();
                let _ = app.emit(
                    "edge-lock://workspace-changed",
                    serde_json::json!({"variantId":variant_id}),
                );
            }
        });
        if let Ok(mut watcher) = watcher
            && watcher
                .watch(
                    PathBuf::from(&detail.variant.workspace_path).as_path(),
                    notify::RecursiveMode::Recursive,
                )
                .is_ok()
        {
            let mut current = lock(&state.watcher)?;
            if state.watcher_generation.load(Ordering::SeqCst) == generation {
                *current = Some(watcher);
            }
        }
        Ok(detail)
    })
    .await
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
pub async fn update_config(
    variant_id: String,
    config: ExportConfig,
    state: State<'_, ManagedState>,
) -> AppResult<()> {
    let state = state.0.clone();
    let library = state.library()?;
    blocking(move || {
        let _gate = lock(&state.operation_gate)?;
        Ok(library.update_config(&variant_id, config)?)
    })
    .await
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
pub async fn set_blender_path(path: String, state: State<'_, ManagedState>) -> AppResult<String> {
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
        let variant = library.get_variant(&variant_id)?.variant;
        let path = variant.config.primary_file.ok_or_else(|| {
            AppError::new("primary_missing", "Choisissez un fichier principal .blend.")
        })?;
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
