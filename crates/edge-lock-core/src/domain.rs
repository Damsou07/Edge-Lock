use serde::{Deserialize, Serialize};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("Erreur de fichier : {0}")]
    Io(#[from] std::io::Error),
    #[error("Erreur du catalogue : {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("Données invalides : {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Invalid(String),
    #[error("Introuvable : {0}")]
    NotFound(String),
    #[error("Ressource occupée : {0}")]
    Busy(String),
    #[error("Opération annulée")]
    Cancelled,
    #[error("État incohérent : {0}")]
    Inconsistent(String),
    #[error("{0}")]
    External(String),
}
pub type Result<T> = std::result::Result<T, CoreError>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationProgress {
    pub phase: String,
    pub completed: u64,
    pub total: Option<u64>,
}

#[derive(Clone, Default)]
pub struct OperationContext {
    pub cancelled: Arc<AtomicBool>,
    pub progress: Option<Arc<dyn Fn(OperationProgress) + Send + Sync>>,
}
impl OperationContext {
    pub fn check_cancelled(&self) -> Result<()> {
        if self.cancelled.load(Ordering::Relaxed) {
            Err(CoreError::Cancelled)
        } else {
            Ok(())
        }
    }
    pub fn report(&self, phase: &str, completed: u64, total: Option<u64>) {
        if let Some(progress) = &self.progress {
            progress(OperationProgress {
                phase: phase.into(),
                completed,
                total,
            });
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExportConfig {
    pub primary_file: Option<String>,
    pub scene: Option<String>,
    pub collection: Option<String>,
    #[serde(default)]
    pub apply_modifiers: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub id: String,
    pub name: String,
    pub category_id: Option<String>,
    pub tags: Vec<String>,
    pub game_ids: Vec<String>,
    pub archived: bool,
    pub created_at: String,
    pub updated_at: String,
    pub working_bytes: u64,
    pub history_bytes: u64,
    pub triangles: Option<u64>,
    pub variant_count: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Variant {
    pub id: String,
    pub asset_id: String,
    pub name: String,
    pub workspace_path: String,
    pub head_snapshot_id: String,
    pub config: ExportConfig,
    pub working_bytes: u64,
    pub triangles: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManifestEntry {
    pub path: String,
    pub kind: String,
    pub hash: Option<String>,
    pub size: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub id: String,
    pub variant_id: String,
    pub parent_id: Option<String>,
    pub created_at: String,
    pub message: String,
    pub logical_bytes: u64,
    pub config: ExportConfig,
    pub entries: Vec<ManifestEntry>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetDetail {
    pub asset: Asset,
    pub variants: Vec<Variant>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariantDetail {
    pub variant: Variant,
    pub snapshots: Vec<Snapshot>,
    pub files: Vec<ManifestEntry>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeSet {
    pub added: Vec<String>,
    pub modified: Vec<String>,
    pub deleted: Vec<String>,
    pub config_changed: bool,
    pub working_bytes: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    pub id: String,
    pub name: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Taxonomy {
    pub categories: Vec<Category>,
    pub games: Vec<Game>,
    pub tags: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryInfo {
    pub root: String,
    pub asset_count: u64,
    pub physical_bytes: u64,
    pub recovery_notices: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AssetFilter {
    pub search: String,
    pub category_id: Option<String>,
    pub game_id: Option<String>,
    pub tag: Option<String>,
    pub sort: String,
    pub archived: bool,
    pub offset: u32,
    pub limit: u32,
}
impl Default for AssetFilter {
    fn default() -> Self {
        Self {
            search: String::new(),
            category_id: None,
            game_id: None,
            tag: None,
            sort: "updated".into(),
            archived: false,
            offset: 0,
            limit: 100,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetPage {
    pub items: Vec<Asset>,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotPage {
    pub items: Vec<Snapshot>,
    pub total: u64,
}

pub(crate) fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("RFC3339 date")
}
pub(crate) fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
pub(crate) fn checked_name(name: &str) -> Result<String> {
    let value = name.trim();
    if value.is_empty() || value.len() > 240 || value.chars().any(char::is_control) {
        return Err(CoreError::Invalid(
            "Le nom doit contenir entre 1 et 240 octets sans caractère de contrôle.".into(),
        ));
    }
    Ok(value.into())
}
