use crate::{preview_resources::PreviewResources, settings::Settings};
use edge_lock_core::{CoreError, Library};
use serde::Serialize;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

#[derive(Debug, Clone, Serialize)]
pub struct AppError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}
impl AppError {
    pub fn new(code: &str, message: impl ToString) -> Self {
        Self {
            code: code.into(),
            message: message.to_string(),
            details: None,
        }
    }
}
impl From<CoreError> for AppError {
    fn from(error: CoreError) -> Self {
        let code = match &error {
            CoreError::Cancelled => "cancelled",
            CoreError::Busy(_) => "busy",
            CoreError::NotFound(_) => "not_found",
            CoreError::Invalid(_) => "invalid",
            CoreError::Inconsistent(_) => "integrity",
            _ => "operation_failed",
        };
        Self::new(code, error)
    }
}
impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        Self::new("io", e)
    }
}
impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        Self::new("json", e)
    }
}
pub type AppResult<T> = Result<T, AppError>;
pub fn lock<T>(mutex: &Mutex<T>) -> AppResult<MutexGuard<'_, T>> {
    mutex.lock().map_err(|_| {
        AppError::new(
            "internal",
            "État occupé après une erreur interne. Redémarrez Edge Lock.",
        )
    })
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub kind: String,
    pub status: String,
    pub phase: String,
    pub completed: u64,
    pub total: Option<u64>,
    pub result: serde_json::Value,
    pub error: Option<AppError>,
}
pub struct JobEntry {
    pub job: Job,
    pub cancelled: Arc<AtomicBool>,
}
pub struct AppState {
    pub settings: Mutex<Settings>,
    pub settings_file: PathBuf,
    pub startup_error: Mutex<Option<String>>,
    pub library: Mutex<Option<Arc<Library>>>,
    pub jobs: Mutex<HashMap<String, JobEntry>>,
    pub operation_gate: Mutex<()>,
    pub previews: Mutex<PreviewResources>,
    pub watcher: Mutex<Option<notify::RecommendedWatcher>>,
    pub watcher_generation: std::sync::atomic::AtomicU64,
    pub workspaces: Mutex<HashMap<String, crate::workspace::WorkspaceEntry>>,
    pub pending_syncs: AtomicUsize,
    pub workspace_worker: AtomicBool,
    pub sync_cancelled: Arc<AtomicBool>,
    // Synchronizes close and library changes with admission of new work.
    pub lifecycle_gate: Mutex<()>,
    pub pending_config_writes: AtomicUsize,
    pub closing: AtomicBool,
}

pub enum CloseDecision {
    Ready,
    Wait,
    Waiting,
}

pub struct PendingConfigWrite(Arc<AppState>);

impl Drop for PendingConfigWrite {
    fn drop(&mut self) {
        self.0.pending_config_writes.fetch_sub(1, Ordering::SeqCst);
    }
}

#[derive(Clone)]
pub struct ManagedState(pub Arc<AppState>);
impl AppState {
    pub fn library(&self) -> AppResult<Arc<Library>> {
        lock(&self.library)?
            .clone()
            .ok_or_else(|| AppError::new("no_library", "Ouvrez une bibliothèque."))
    }
    pub fn busy(&self) -> AppResult<bool> {
        Ok(self.pending_config_writes.load(Ordering::SeqCst) > 0
            || self.pending_syncs.load(Ordering::SeqCst) > 0
            || lock(&self.jobs)?
                .values()
                .any(|entry| matches!(entry.job.status.as_str(), "queued" | "running")))
    }

    pub fn register_config_write(self: &Arc<Self>) -> AppResult<PendingConfigWrite> {
        let _lifecycle = lock(&self.lifecycle_gate)?;
        self.ensure_running()?;
        self.pending_config_writes.fetch_add(1, Ordering::SeqCst);
        Ok(PendingConfigWrite(self.clone()))
    }

    pub fn ensure_running(&self) -> AppResult<()> {
        if self.closing.load(Ordering::SeqCst) {
            return Err(AppError::new(
                "closing",
                "Edge Lock termine les tâches avant de se fermer.",
            ));
        }
        Ok(())
    }

    pub fn request_close(&self) -> AppResult<CloseDecision> {
        let _lifecycle = lock(&self.lifecycle_gate)?;
        let already_closing = self.closing.swap(true, Ordering::SeqCst);
        self.sync_cancelled.store(true, Ordering::Relaxed);
        if !self.busy()? {
            Ok(CloseDecision::Ready)
        } else if already_closing {
            Ok(CloseDecision::Waiting)
        } else {
            Ok(CloseDecision::Wait)
        }
    }
}

#[cfg(test)]
pub fn test_state(library: Arc<Library>) -> AppState {
    AppState {
        settings: Mutex::new(Settings::default()),
        settings_file: library.root().join("test-settings.json"),
        startup_error: Mutex::new(None),
        library: Mutex::new(Some(library)),
        jobs: Mutex::new(HashMap::new()),
        operation_gate: Mutex::new(()),
        previews: Mutex::new(PreviewResources::default()),
        watcher: Mutex::new(None),
        watcher_generation: std::sync::atomic::AtomicU64::new(0),
        workspaces: Mutex::new(HashMap::new()),
        pending_syncs: AtomicUsize::new(0),
        workspace_worker: AtomicBool::new(false),
        sync_cancelled: Arc::new(AtomicBool::new(false)),
        lifecycle_gate: Mutex::new(()),
        pending_config_writes: AtomicUsize::new(0),
        closing: AtomicBool::new(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submitted_config_writes_keep_close_waiting_until_the_guard_is_released() {
        let root = tempfile::tempdir().unwrap();
        let state = Arc::new(test_state(Arc::new(Library::open(root.path()).unwrap())));
        let pending = state.register_config_write().unwrap();
        assert!(state.busy().unwrap());
        assert!(matches!(
            state.request_close().unwrap(),
            CloseDecision::Wait
        ));
        assert!(matches!(
            state.request_close().unwrap(),
            CloseDecision::Waiting
        ));
        assert!(matches!(state.register_config_write(), Err(error) if error.code == "closing"));
        drop(pending);
        assert!(!state.busy().unwrap());
        assert!(matches!(
            state.request_close().unwrap(),
            CloseDecision::Ready
        ));
    }

    #[test]
    fn a_failed_operation_releases_its_pending_write_and_idle_close_rejects_new_writes() {
        let root = tempfile::tempdir().unwrap();
        let state = Arc::new(test_state(Arc::new(Library::open(root.path()).unwrap())));
        let result: AppResult<()> = (|| {
            let _pending = state.register_config_write()?;
            Err(AppError::new("test", "Échec injecté"))
        })();
        assert!(result.is_err());
        assert!(!state.busy().unwrap());
        assert!(matches!(
            state.request_close().unwrap(),
            CloseDecision::Ready
        ));
        assert!(matches!(state.register_config_write(), Err(error) if error.code == "closing"));
    }
}
