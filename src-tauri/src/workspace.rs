//! Library-wide, trailing-edge checks of editable project files.
use crate::state::*;
use edge_lock_core::{
    AssetFilter, Library, OperationContext, WorkspaceDiscardReport, WorkspaceDiscardState, sync,
};
use notify::Watcher;
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter};

const SAVE_DELAY: Duration = Duration::from_millis(500);
const DISCARD_RETRY_MIN: Duration = Duration::from_secs(2);
const DISCARD_RETRY_MAX: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceStatus {
    pub variant_id: String,
    pub revision: u64,
    pub state: String,
    pub source_changed: bool,
    pub can_create_snapshot: bool,
    pub artifact_key: Option<String>,
    pub error: Option<AppError>,
    pub discard_state: Option<WorkspaceDiscardState>,
    pub discard_cleanup_pending: bool,
}

pub struct WorkspaceEntry {
    pub status: WorkspaceStatus,
    requested: Instant,
    sequence: u64,
    running: bool,
}

impl WorkspaceEntry {
    fn new(variant_id: &str) -> Self {
        Self {
            status: WorkspaceStatus {
                variant_id: variant_id.into(),
                revision: 0,
                state: "checking".into(),
                source_changed: false,
                can_create_snapshot: false,
                artifact_key: None,
                error: None,
                discard_state: None,
                discard_cleanup_pending: false,
            },
            requested: Instant::now(),
            sequence: 0,
            running: false,
        }
    }

    fn request(&mut self) -> bool {
        self.sequence += 1;
        self.requested = Instant::now();
        self.status.revision += 1;
        self.status.state = "checking".into();
        self.status.can_create_snapshot = false;
        self.status.error = None;
        let start = !self.running;
        self.running = true;
        start
    }

    fn schedule(&mut self, invalidate_running: bool) -> bool {
        if self.running && !invalidate_running {
            return false;
        }
        self.request();
        true
    }
}

fn emit(app: &AppHandle, status: &WorkspaceStatus) {
    let _ = app.emit("edge-lock://workspace-status", status);
}

pub fn status(
    app: &AppHandle,
    state: &Arc<AppState>,
    variant_id: &str,
) -> AppResult<WorkspaceStatus> {
    if let Some(entry) = lock(&state.workspaces)?.get(variant_id) {
        return Ok(entry.status.clone());
    }
    request(app, state, variant_id)
}

pub fn request(
    app: &AppHandle,
    state: &Arc<AppState>,
    variant_id: &str,
) -> AppResult<WorkspaceStatus> {
    schedule(app, state, variant_id, WorkspaceRequest::Changed)
}

enum WorkspaceRequest<'a> {
    Changed,
    Reconcile,
    Discard {
        library: &'a Arc<Library>,
        cleanup_pending: bool,
    },
}

pub fn refresh_discard(
    app: &AppHandle,
    state: &Arc<AppState>,
    library: &Arc<Library>,
    variant_id: &str,
    cleanup_pending: bool,
) -> AppResult<WorkspaceStatus> {
    schedule(
        app,
        state,
        variant_id,
        WorkspaceRequest::Discard {
            library,
            cleanup_pending,
        },
    )
}

fn schedule(
    app: &AppHandle,
    state: &Arc<AppState>,
    variant_id: &str,
    reason: WorkspaceRequest<'_>,
) -> AppResult<WorkspaceStatus> {
    let _lifecycle = lock(&state.lifecycle_gate)?;
    state.ensure_running()?;
    let library = state.library()?;
    if let WorkspaceRequest::Discard { library: owner, .. } = &reason
        && !Arc::ptr_eq(owner, &library)
    {
        return Err(AppError::new(
            "library_changed",
            "La bibliothèque a changé.",
        ));
    }
    // Validate identifiers before admitting background work or allocating status entries.
    library.workspace_path(variant_id)?;
    let discard_state = library.discard_status(variant_id)?;
    let generation = state.watcher_generation.load(Ordering::SeqCst);
    let status = {
        let mut workspaces = lock(&state.workspaces)?;
        let entry = workspaces
            .entry(variant_id.into())
            .or_insert_with(|| WorkspaceEntry::new(variant_id));
        // Reconciliation must not perpetually invalidate a conversion slower than its poll interval.
        if !entry.schedule(!matches!(reason, WorkspaceRequest::Reconcile)) {
            return Ok(entry.status.clone());
        }
        entry.status.discard_state = discard_state;
        if let WorkspaceRequest::Discard {
            cleanup_pending, ..
        } = reason
        {
            entry.status.discard_cleanup_pending |= cleanup_pending;
        }
        entry.status.clone()
    };
    emit(app, &status);
    if !state.workspace_worker.swap(true, Ordering::SeqCst) {
        state.pending_syncs.fetch_add(1, Ordering::SeqCst);
        let app = app.clone();
        let state = state.clone();
        tauri::async_runtime::spawn_blocking(move || {
            loop {
                let variant = {
                    let Ok(workspaces) = state.workspaces.lock() else {
                        break;
                    };
                    let next = workspaces
                        .values()
                        .filter(|entry| entry.running)
                        .min_by_key(|entry| entry.requested)
                        .map(|entry| entry.status.variant_id.clone());
                    if next.is_none() || state.closing.load(Ordering::SeqCst) {
                        state.workspace_worker.store(false, Ordering::SeqCst);
                        break;
                    }
                    next
                };
                let Some(variant) = variant else { break };
                if state.watcher_generation.load(Ordering::SeqCst) != generation {
                    state.workspace_worker.store(false, Ordering::SeqCst);
                    break;
                }
                synchronize(&app, &state, &library, generation, &variant);
            }
            state.pending_syncs.fetch_sub(1, Ordering::SeqCst);
            // A replacement library may have enqueued work just as the old generation exited.
            let pending = state.workspaces.lock().ok().and_then(|entries| {
                entries
                    .values()
                    .find(|entry| entry.running)
                    .map(|entry| entry.status.variant_id.clone())
            });
            if let Some(variant_id) = pending {
                let _ = request(&app, &state, &variant_id);
            }
        });
    }
    Ok(status)
}

fn synchronize(
    app: &AppHandle,
    state: &Arc<AppState>,
    library: &Arc<Library>,
    generation: u64,
    variant_id: &str,
) {
    loop {
        if state.closing.load(Ordering::SeqCst)
            || state.watcher_generation.load(Ordering::SeqCst) != generation
        {
            break;
        }
        let pending = lock(&state.workspaces).ok().and_then(|workspaces| {
            workspaces
                .get(variant_id)
                .map(|entry| (entry.sequence, entry.requested))
        });
        let Some((sequence, requested)) = pending else {
            break;
        };
        if requested.elapsed() < SAVE_DELAY {
            std::thread::sleep(Duration::from_millis(50));
            continue;
        }
        let result = (|| -> AppResult<_> {
            let _gate = lock(&state.operation_gate)?;
            state.ensure_running()?;
            if generation != state.watcher_generation.load(Ordering::SeqCst) {
                return Err(AppError::new(
                    "library_changed",
                    "La bibliothèque a changé.",
                ));
            }
            {
                let mut workspaces = lock(&state.workspaces)?;
                let entry = workspaces
                    .get_mut(variant_id)
                    .ok_or_else(|| AppError::new("not_found", "Projet de travail absent."))?;
                // A save may have arrived while this task waited for the operation gate.
                if entry.sequence != sequence || entry.requested.elapsed() < SAVE_DELAY {
                    return Ok(None);
                }
                entry.status.revision += 1;
                entry.status.state = "syncing".into();
                emit(app, &entry.status);
            }
            let context = OperationContext {
                cancelled: state.sync_cancelled.clone(),
                ..Default::default()
            };
            let previous = lock(&state.workspaces)?
                .get(variant_id)
                .and_then(|entry| entry.status.artifact_key.clone());
            let checked = check_project(library, variant_id, previous.as_deref(), &context)?;
            Ok(Some(checked))
        })();
        if matches!(result, Ok(None)) {
            continue;
        }
        let mut again = false;
        if let Ok(mut workspaces) = state.workspaces.lock()
            && let Some(entry) = workspaces.get_mut(variant_id)
        {
            if entry.sequence != sequence {
                again = true;
            } else {
                entry.running = false;
                entry.status.revision += 1;
                match result {
                    Ok(Some(checked)) => {
                        entry.status.state = "ready".into();
                        entry.status.source_changed = checked.changes.has_source_changes();
                        entry.status.can_create_snapshot = checked.changes.has_source_changes();
                        entry.status.artifact_key = Some(checked.fingerprint);
                        entry.status.error = None;
                    }
                    Err(error) => {
                        entry.status.state = if error.code == "invalid_project" {
                            "nonconforming"
                        } else {
                            "failed"
                        }
                        .into();
                        entry.status.can_create_snapshot = false;
                        entry.status.error = Some(error);
                    }
                    Ok(None) => unreachable!(),
                }
                emit(app, &entry.status);
            }
        }
        if !again {
            break;
        }
    }
}

fn check_project(
    library: &Library,
    variant_id: &str,
    previous_fingerprint: Option<&str>,
    context: &OperationContext,
) -> AppResult<edge_lock_core::WorkspaceCheck> {
    let files = library.get_variant(variant_id)?.files;
    sync::validate_project(&files).map_err(|error| AppError::new("invalid_project", error))?;
    let checked = library.check_workspace(variant_id, context)?;
    if previous_fingerprint.map_or_else(
        || checked.changes.has_source_changes(),
        |key| key != checked.fingerprint,
    ) {
        library.mark_workspace_changed(variant_id)?;
    }
    Ok(checked)
}

fn variants(library: &Library) -> AppResult<Vec<String>> {
    let mut offset = 0;
    let mut result = Vec::new();
    // Both archived and visible projects may still be open in Blender.
    for archived in [false, true] {
        loop {
            let page = library.list_assets(AssetFilter {
                archived,
                offset,
                limit: 100,
                ..Default::default()
            })?;
            for asset in &page.items {
                result.extend(
                    library
                        .get_asset(&asset.id)?
                        .variants
                        .into_iter()
                        .map(|v| v.id),
                );
            }
            offset += page.items.len() as u32;
            if u64::from(offset) >= page.total || page.items.is_empty() {
                break;
            }
        }
        offset = 0;
    }
    Ok(result)
}

pub fn reconcile(app: &AppHandle, state: &Arc<AppState>) {
    if let Ok(library) = state.library()
        && let Ok(ids) = variants(&library)
    {
        for id in ids {
            let _ = schedule(app, state, &id, WorkspaceRequest::Reconcile);
        }
    }
}

struct DiscardRetry {
    library: Arc<Library>,
    reports: Vec<WorkspaceDiscardReport>,
    still_pending: bool,
}

fn retry_discards(state: &AppState, generation: u64) -> AppResult<Option<DiscardRetry>> {
    let Ok(_gate) = state.operation_gate.try_lock() else {
        return Ok(None);
    };
    let library = {
        let _lifecycle = lock(&state.lifecycle_gate)?;
        if state.closing.load(Ordering::SeqCst)
            || state.watcher_generation.load(Ordering::SeqCst) != generation
        {
            return Ok(None);
        }
        let library = state.library()?;
        if !library.has_pending_discards()? {
            return Ok(Some(DiscardRetry {
                library,
                reports: Vec::new(),
                still_pending: false,
            }));
        }
        state.pending_syncs.fetch_add(1, Ordering::SeqCst);
        library
    };
    let context = OperationContext {
        cancelled: state.sync_cancelled.clone(),
        ..Default::default()
    };
    let result = (|| -> AppResult<_> {
        let reports = library.retry_pending_discards(&context)?;
        let still_pending = library.has_pending_discards()?;
        Ok(Some(DiscardRetry {
            library,
            reports,
            still_pending,
        }))
    })();
    state.pending_syncs.fetch_sub(1, Ordering::SeqCst);
    result
}

fn schedule_discard_retries(app: &AppHandle, state: &Arc<AppState>, generation: u64) {
    let app = app.clone();
    let weak = Arc::downgrade(state);
    std::thread::spawn(move || {
        let mut backoff = DISCARD_RETRY_MIN;
        while let Some(state) = weak.upgrade() {
            if state.closing.load(Ordering::SeqCst)
                || state.watcher_generation.load(Ordering::SeqCst) != generation
            {
                break;
            }
            let delay = match retry_discards(&state, generation) {
                Ok(Some(retry)) => {
                    for report in retry.reports {
                        let _ = refresh_discard(
                            &app,
                            &state,
                            &retry.library,
                            &report.variant_id,
                            report.cleanup_pending,
                        );
                    }
                    if retry.still_pending {
                        let delay = backoff;
                        backoff = (backoff * 2).min(DISCARD_RETRY_MAX);
                        delay
                    } else {
                        backoff = DISCARD_RETRY_MIN;
                        DISCARD_RETRY_MIN
                    }
                }
                Ok(None) => DISCARD_RETRY_MIN,
                Err(_) => DISCARD_RETRY_MAX,
            };
            drop(state);
            std::thread::sleep(delay);
        }
    });
}

fn affected(root: &Path, paths: &[std::path::PathBuf]) -> HashSet<String> {
    paths
        .iter()
        .filter_map(|path| {
            dunce::simplified(path)
                .strip_prefix(dunce::simplified(root))
                .ok()
        })
        .filter_map(|relative| relative.components().nth(1))
        .filter_map(|part| part.as_os_str().to_str().map(str::to_owned))
        .collect()
}

pub fn install(app: &AppHandle, state: &Arc<AppState>) {
    let generation = state.watcher_generation.fetch_add(1, Ordering::SeqCst) + 1;
    if let Ok(mut watcher) = state.watcher.lock() {
        *watcher = None;
    }
    if let Ok(mut workspaces) = state.workspaces.lock() {
        *workspaces = HashMap::new();
    }
    let Ok(library) = state.library() else { return };
    let root = library.root().join("workspaces");
    let watch_root = root.clone();
    let weak = Arc::downgrade(state);
    let handle = app.clone();
    let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let Some(state) = weak.upgrade() else { return };
        if generation != state.watcher_generation.load(Ordering::SeqCst) {
            return;
        }
        match event {
            Ok(event) if !matches!(event.kind, notify::EventKind::Access(_)) => {
                let ids = affected(&watch_root, &event.paths);
                if ids.is_empty() || event.need_rescan() {
                    reconcile(&handle, &state);
                    return;
                }
                for id in ids {
                    let _ = handle.emit(
                        "edge-lock://workspace-changed",
                        serde_json::json!({"variantId": id}),
                    );
                    let _ = request(&handle, &state, &id);
                }
            }
            Err(_) => reconcile(&handle, &state),
            _ => {}
        }
    });
    let installed = watcher.and_then(|mut watcher| {
        watcher.watch(&root, notify::RecursiveMode::Recursive)?;
        Ok(watcher)
    });
    match installed {
        Ok(watcher) => {
            if let Ok(mut current) = state.watcher.lock() {
                *current = Some(watcher);
            }
        }
        Err(error) => {
            if let Ok(mut startup) = state.startup_error.lock() {
                *startup = Some(format!(
                    "Surveillance indisponible : {error}. Vérification périodique activée."
                ));
            }
            let weak = Arc::downgrade(state);
            let app = app.clone();
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_secs(5));
                    let Some(state) = weak.upgrade() else { break };
                    if state.closing.load(Ordering::SeqCst)
                        || state.watcher_generation.load(Ordering::SeqCst) != generation
                    {
                        break;
                    }
                    reconcile(&app, &state);
                }
            });
        }
    }
    schedule_discard_retries(app, state, generation);
    reconcile(app, state);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn discard_retry_respects_the_gate_and_library_generation() {
        use std::{fs, os::windows::fs::OpenOptionsExt};

        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("main.blend"), b"saved").unwrap();
        let library = Arc::new(Library::open(root.path().join("library")).unwrap());
        let context = OperationContext::default();
        let asset = library
            .import_folder(
                &source,
                "Discard retry",
                edge_lock_core::ExportConfig::default(),
                &context,
            )
            .unwrap();
        let variant = &asset.variants[0];
        let workspace = Path::new(&variant.workspace_path);
        fs::write(workspace.join("main.blend"), b"edited").unwrap();
        let handle = fs::OpenOptions::new()
            .read(true)
            .share_mode(3)
            .custom_flags(0x02000000)
            .open(workspace)
            .unwrap();
        let queued = library
            .discard_changes(&variant.id, &variant.head_snapshot_id, &context)
            .unwrap();
        assert!(queued.pending);

        let state = test_state(library);
        let gate = state.operation_gate.lock().unwrap();
        assert!(retry_discards(&state, 0).unwrap().is_none());
        drop(gate);
        let waiting = retry_discards(&state, 0).unwrap().unwrap();
        assert!(waiting.still_pending);
        assert!(waiting.reports.is_empty());
        assert!(!state.busy().unwrap());

        drop(handle);
        assert!(retry_discards(&state, 1).unwrap().is_none());
        assert_eq!(fs::read(workspace.join("main.blend")).unwrap(), b"edited");
        let completed = retry_discards(&state, 0).unwrap().unwrap();
        assert!(!completed.still_pending);
        assert_eq!(completed.reports.len(), 1);
        assert_eq!(completed.reports[0].variant_id, variant.id);
        assert_eq!(fs::read(workspace.join("main.blend")).unwrap(), b"saved");
        assert!(!state.busy().unwrap());
    }

    #[test]
    fn first_scan_invalidates_offline_edits_but_preserves_unchanged_metadata() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("main.blend"), b"saved").unwrap();
        let library_path = root.path().join("library");
        let library = Library::open(&library_path).unwrap();
        let ctx = OperationContext::default();
        let asset = library
            .import_folder(
                &source,
                "Offline",
                edge_lock_core::ExportConfig::default(),
                &ctx,
            )
            .unwrap();
        let variant_id = asset.variants[0].id.clone();
        let blend = Path::new(&asset.variants[0].workspace_path).join("main.blend");
        library.set_geometry(&variant_id, 123).unwrap();
        let updated_at = asset.asset.updated_at;
        let clean = check_project(&library, &variant_id, None, &ctx).unwrap();
        assert!(!clean.changes.has_source_changes());
        assert_eq!(
            library.get_variant(&variant_id).unwrap().variant.triangles,
            Some(123)
        );
        assert_eq!(
            library.get_asset(&asset.asset.id).unwrap().asset.updated_at,
            updated_at
        );
        drop(library);
        std::fs::write(blend, b"saved while application was closed").unwrap();
        let library = Library::open(&library_path).unwrap();
        let dirty = check_project(&library, &variant_id, None, &ctx).unwrap();
        assert!(dirty.changes.has_source_changes());
        assert_eq!(
            library.get_variant(&variant_id).unwrap().variant.triangles,
            None
        );
        let refreshed_at = library.get_asset(&asset.asset.id).unwrap().asset.updated_at;
        assert_ne!(refreshed_at, updated_at);
        library.set_geometry(&variant_id, 456).unwrap();
        check_project(&library, &variant_id, Some(&dirty.fingerprint), &ctx).unwrap();
        assert_eq!(
            library.get_variant(&variant_id).unwrap().variant.triangles,
            Some(456)
        );
        assert_eq!(
            library.get_asset(&asset.asset.id).unwrap().asset.updated_at,
            refreshed_at
        );
    }

    #[test]
    fn all_glb_events_including_former_outputs_resolve_the_variant() {
        let root = Path::new("workspaces");
        for model in ["main.glb", "other.glb", ".edge-lock-secondary.glb"] {
            assert_eq!(
                affected(root, &[root.join("asset/variant").join(model)]),
                HashSet::from(["variant".into()])
            );
        }
    }

    #[test]
    fn repeated_saves_keep_one_worker_and_invalidate_snapshot_readiness() {
        let mut entry = WorkspaceEntry::new("variant");
        assert!(entry.request());
        entry.status.can_create_snapshot = true;
        assert!(!entry.request());
        assert_eq!(entry.sequence, 2);
        assert_eq!(entry.status.revision, 2);
        assert!(!entry.status.can_create_snapshot);
        assert_eq!(entry.status.state, "checking");
    }

    #[test]
    fn events_resolve_all_affected_variants_without_selection() {
        let root = Path::new("workspaces");
        let ids = affected(
            root,
            &[
                root.join("asset/first/main.blend"),
                root.join("asset/second/model.glb"),
                root.join("asset/first/main.blend1"),
            ],
        );
        assert_eq!(ids, HashSet::from(["first".into(), "second".into()]));
    }

    #[test]
    fn periodic_reconciliation_cannot_starve_a_slow_conversion() {
        let mut entry = WorkspaceEntry::new("large-project");
        assert!(entry.schedule(false));
        let revision = entry.status.revision;
        let requested = entry.requested;
        for _ in 0..10 {
            assert!(!entry.schedule(false));
        }
        assert_eq!(entry.sequence, 1);
        assert_eq!(entry.status.revision, revision);
        assert_eq!(entry.requested, requested);
        // A real new save still supersedes the in-flight result.
        assert!(entry.schedule(true));
        assert_eq!(entry.sequence, 2);
    }
}
