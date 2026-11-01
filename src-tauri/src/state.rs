use crate::settings::Settings;
use edge_lock_core::{CoreError, Library};
use serde::Serialize;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard, atomic::AtomicBool},
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
    pub previews: Mutex<HashMap<String, PathBuf>>,
    pub watcher: Mutex<Option<notify::RecommendedWatcher>>,
    pub watcher_generation: std::sync::atomic::AtomicU64,
    pub closing: AtomicBool,
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
        Ok(lock(&self.jobs)?
            .values()
            .any(|entry| matches!(entry.job.status.as_str(), "queued" | "running")))
    }
}
