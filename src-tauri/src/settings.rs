use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::Path};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub library_path: Option<String>,
    pub blender_path: Option<String>,
}

pub fn load(path: &Path) -> Result<Settings, String> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|e| format!("Réglages illisibles, original conservé : {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn save(path: &Path, settings: &Settings) -> Result<(), String> {
    let parent = path.parent().ok_or("Dossier de réglages invalide")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    if path.exists() && load(path).is_err() {
        let backup = path.with_extension(format!("invalid-{}.json", uuid::Uuid::new_v4()));
        fs::copy(path, backup).map_err(|e| e.to_string())?;
    }
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn detect_blender() -> Option<String> {
    let base = std::env::var_os("ProgramFiles").map(std::path::PathBuf::from)?;
    let mut candidates: Vec<_> = fs::read_dir(base.join("Blender Foundation"))
        .ok()?
        .flatten()
        .map(|entry| entry.path().join("blender.exe"))
        .filter(|p| p.is_file())
        .collect();
    candidates.sort();
    candidates.pop().map(|p| p.to_string_lossy().into_owned())
}

pub fn open_previous_library(path: &Path) -> Result<edge_lock_core::Library, String> {
    if !path.join(".edge-lock-library").is_file() && !path.join(".edge-lock-initializing").is_file()
    {
        return Err("La bibliothèque enregistrée est introuvable. Reconnectez son disque ou sélectionnez son nouvel emplacement ; aucune bibliothèque vide n’a été créée.".into());
    }
    edge_lock_core::Library::open(path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_round_trip_and_replace_preserve_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert_eq!(load(&path).unwrap(), Settings::default());
        let mut settings = Settings {
            library_path: Some("C:\\Assets é".into()),
            blender_path: None,
        };
        save(&path, &settings).unwrap();
        settings.blender_path = Some("C:\\Blender\\blender.exe".into());
        save(&path, &settings).unwrap();
        assert_eq!(load(&path).unwrap(), settings);
    }
    #[test]
    fn malformed_settings_are_not_silently_discarded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, b"broken user settings").unwrap();
        assert!(load(&path).is_err());
        save(&path, &Settings::default()).unwrap();
        let backups: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains("invalid-"))
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(
            fs::read(backups[0].path()).unwrap(),
            b"broken user settings"
        );
    }
    #[test]
    fn missing_previous_library_is_never_replaced_with_an_empty_database() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("missing-library");
        assert!(open_previous_library(&missing).is_err());
        assert!(!missing.exists());
    }
}
