//! Persisted viewing/export choices, independent from versioned asset configuration.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSelections {
    pub preview_file: Option<String>,
    pub export_file: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FileSelectionKind {
    Preview,
    Export,
}

fn validate_scope(db: &Connection, variant_id: &str, snapshot_id: Option<&str>) -> Result<()> {
    if let Some(snapshot_id) = snapshot_id {
        require_history_snapshot(db, variant_id, snapshot_id)?;
    } else {
        require_active_variant(db, variant_id)?;
    }
    Ok(())
}

impl Library {
    pub fn get_file_selections(
        &self,
        variant_id: &str,
        snapshot_id: Option<&str>,
    ) -> Result<FileSelections> {
        let db = self.db()?;
        validate_scope(&db, variant_id, snapshot_id)?;
        Ok(db
            .query_row(
                "SELECT preview_file,export_file FROM file_selections WHERE variant_id=?1 AND context_key=?2",
                params![variant_id, snapshot_id.unwrap_or("current")],
                |row| Ok(FileSelections { preview_file: row.get(0)?, export_file: row.get(1)? }),
            )
            .optional()?
            .unwrap_or_default())
    }

    pub fn set_file_selection(
        &self,
        variant_id: &str,
        snapshot_id: Option<&str>,
        kind: FileSelectionKind,
        file: Option<&str>,
    ) -> Result<()> {
        let _guard = self.write_guard()?;
        let db = self.db()?;
        let variant = read_variant(&db, &self.root, variant_id)?;
        validate_scope(&db, variant_id, snapshot_id)?;
        if let Some(file) = file {
            safe_relative(file)?;
            let extension = Path::new(file).extension().and_then(|value| value.to_str());
            if !extension.is_some_and(|value| {
                value.eq_ignore_ascii_case("blend") || value.eq_ignore_ascii_case("glb")
            }) {
                return Err(CoreError::Invalid(
                    "La source doit être un fichier .blend ou .glb.".into(),
                ));
            }
            let entries = if let Some(snapshot_id) = snapshot_id {
                read_snapshot(&db, snapshot_id)?.entries
            } else {
                list_files(Path::new(&variant.workspace_path))?
            };
            if !entries
                .iter()
                .any(|entry| entry.kind == "file" && entry.path == file)
            {
                return Err(CoreError::Invalid(
                    "Le fichier sélectionné n’est plus disponible.".into(),
                ));
            }
        }
        let statement = match kind {
            FileSelectionKind::Preview => {
                "INSERT INTO file_selections(variant_id,context_key,preview_file) VALUES(?1,?2,?3) ON CONFLICT(variant_id,context_key) DO UPDATE SET preview_file=excluded.preview_file"
            }
            FileSelectionKind::Export => {
                "INSERT INTO file_selections(variant_id,context_key,export_file) VALUES(?1,?2,?3) ON CONFLICT(variant_id,context_key) DO UPDATE SET export_file=excluded.export_file"
            }
        };
        db.execute(
            statement,
            params![variant_id, snapshot_id.unwrap_or("current"), file],
        )?;
        Ok(())
    }
}
