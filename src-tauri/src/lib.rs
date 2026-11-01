mod commands;
mod jobs;
mod settings;
mod state;

use state::{AppState, ManagedState};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(
            tauri_plugin_opener::Builder::new()
                .open_js_links_on_click(false)
                .build(),
        )
        .setup(|app| {
            let settings_dir = if cfg!(debug_assertions) {
                std::env::var_os("EDGE_LOCK_TEST_DATA_DIR")
                    .map(std::path::PathBuf::from)
                    .unwrap_or(app.path().app_config_dir()?)
            } else {
                app.path().app_config_dir()?
            };
            let settings_file = settings_dir.join("settings.json");
            let (mut settings, mut startup_error) = match settings::load(&settings_file) {
                Ok(s) => (s, None),
                Err(e) => (settings::Settings::default(), Some(e)),
            };
            if settings.blender_path.is_none() {
                settings.blender_path = settings::detect_blender();
            }
            let library =
                settings.library_path.as_ref().and_then(
                    |path| match settings::open_previous_library(std::path::Path::new(path)) {
                        Ok(library) => Some(Arc::new(library)),
                        Err(error) => {
                            startup_error = Some(error);
                            None
                        }
                    },
                );
            if let Some(library) = &library {
                let _ = edge_lock_core::preview::cleanup_temporary(library.root());
            }
            app.manage(ManagedState(Arc::new(AppState {
                settings: Mutex::new(settings),
                settings_file,
                startup_error: Mutex::new(startup_error),
                library: Mutex::new(library),
                jobs: Mutex::new(HashMap::new()),
                operation_gate: Mutex::new(()),
                previews: Mutex::new(HashMap::new()),
                watcher: Mutex::new(None),
                watcher_generation: std::sync::atomic::AtomicU64::new(0),
                closing: std::sync::atomic::AtomicBool::new(false),
            })));
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<ManagedState>().0.clone();
                if state.busy().unwrap_or(false) {
                    api.prevent_close();
                    use std::sync::atomic::Ordering;
                    if !state.closing.swap(true, Ordering::SeqCst) {
                        if let Ok(jobs) = state.jobs.lock() {
                            for entry in jobs.values() {
                                entry.cancelled.store(true, Ordering::Relaxed);
                            }
                        }
                        let window = window.clone();
                        std::thread::spawn(move || {
                            // Let workers terminate Blender and finish any atomic publication
                            // before the application process exits.
                            while state.busy().unwrap_or(false) {
                                std::thread::sleep(std::time::Duration::from_millis(50));
                            }
                            let _ = window.close();
                        });
                    }
                }
            }
        })
        .register_asynchronous_uri_scheme_protocol("edge-preview", |context, request, responder| {
            let state = context.app_handle().state::<ManagedState>().0.clone();
            tauri::async_runtime::spawn_blocking(move || {
                let token = request.uri().path().trim_start_matches('/');
                let origin = request
                    .headers()
                    .get("Origin")
                    .and_then(|o| o.to_str().ok())
                    .unwrap_or("http://tauri.localhost");
                let origin = if [
                    "http://tauri.localhost",
                    "tauri://localhost",
                    "http://localhost:1420",
                    "http://127.0.0.1:1420",
                ]
                .contains(&origin)
                {
                    origin
                } else {
                    "http://tauri.localhost"
                };
                let path = if token.len() == 32 && token.bytes().all(|b| b.is_ascii_hexdigit()) {
                    state
                        .previews
                        .lock()
                        .ok()
                        .and_then(|paths| paths.get(token).cloned())
                } else {
                    None
                };
                let response = if request.method() != "GET" {
                    tauri::http::Response::builder()
                        .status(405)
                        .body(Vec::new())
                } else if let Some(path) = path {
                    match std::fs::metadata(&path).and_then(|meta| {
                        if meta.len() <= edge_lock_core::glb::MAX_PREVIEW_BYTES {
                            std::fs::read(&path)
                        } else {
                            Err(std::io::Error::other("Aperçu trop volumineux"))
                        }
                    }) {
                        Ok(bytes) => tauri::http::Response::builder()
                            .status(200)
                            .header("Content-Type", "model/gltf-binary")
                            .header("Access-Control-Allow-Origin", origin)
                            .header("Cache-Control", "private, max-age=3600")
                            .body(bytes),
                        Err(_) => tauri::http::Response::builder()
                            .status(404)
                            .body(Vec::new()),
                    }
                } else {
                    tauri::http::Response::builder()
                        .status(404)
                        .body(Vec::new())
                };
                responder
                    .respond(response.unwrap_or_else(|_| tauri::http::Response::new(Vec::new())));
            });
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_bootstrap,
            commands::open_library,
            commands::list_assets,
            commands::get_asset,
            commands::get_variant,
            commands::get_snapshot,
            commands::list_snapshots,
            commands::get_taxonomy,
            commands::update_asset,
            commands::save_category,
            commands::save_game,
            commands::update_config,
            commands::choose_folder,
            commands::choose_export_path,
            commands::choose_blender_path,
            commands::copy_text,
            commands::set_blender_path,
            commands::open_workspace,
            commands::open_blender,
            commands::start_job,
            commands::get_job,
            commands::cancel_job
        ])
        .run(tauri::generate_context!())
        .expect("Impossible de démarrer Edge Lock");
}
