//! Metadata-only ownership evidence for delayed workspace deletion.
use super::*;
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct WorkspaceIdentity {
    volume: u64,
    index: u64,
    created: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct WorkspaceEntry {
    pub(super) path: String,
    pub(super) directory: bool,
    pub(super) size: u64,
    modified: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct WorkspaceEvidence {
    identity: WorkspaceIdentity,
    pub(super) entries: Vec<WorkspaceEntry>,
}

impl WorkspaceEvidence {
    pub(super) fn capture(path: &Path) -> Result<Self> {
        let identity = identity(path)?;
        let mut entries = Vec::new();
        for entry in list_files(path)? {
            let entry_path = path.join(safe_relative(&entry.path)?);
            let metadata = fs::symlink_metadata(&entry_path)?;
            let directory = entry.kind == "directory";
            if directory {
                ensure_real_directory(&entry_path)?;
            } else if !ordinary_file(&entry_path)? {
                return Err(CoreError::Busy(
                    "Un fichier du workspace a disparu pendant son analyse.".into(),
                ));
            }
            if metadata.is_dir() != directory || (!directory && metadata.len() != entry.size) {
                return Err(CoreError::Busy(
                    "Le workspace a changé pendant son analyse.".into(),
                ));
            }
            entries.push(WorkspaceEntry {
                path: entry.path,
                directory,
                size: entry.size,
                modified: if directory {
                    None
                } else {
                    metadata
                        .modified()
                        .ok()
                        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                        .and_then(|duration| duration.as_nanos().try_into().ok())
                },
            });
        }
        if identity != self::identity(path)? {
            return Err(CoreError::Busy(
                "Le dossier de travail a changé pendant son analyse.".into(),
            ));
        }
        Ok(Self { identity, entries })
    }

    pub(super) fn bytes(&self) -> u64 {
        self.entries
            .iter()
            .fold(0, |sum, entry| sum.saturating_add(entry.size))
    }

    pub(super) fn remove_file(&self, root: &Path, entry: &WorkspaceEntry) -> Result<u64> {
        if identity(root)? != self.identity {
            return Err(CoreError::Inconsistent(
                "Le dossier de suppression a été remplacé ; fichiers préservés.".into(),
            ));
        }
        let relative = safe_relative(&entry.path)?;
        let mut parent = relative.parent();
        while let Some(path) = parent {
            ensure_real_directory(&root.join(path))?;
            parent = path.parent();
        }
        let path = root.join(relative);
        if !ordinary_file(&path)? {
            return Ok(0);
        }
        let metadata = fs::symlink_metadata(&path)?;
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .and_then(|duration| duration.as_nanos().try_into().ok());
        if metadata.len() != entry.size || modified != entry.modified {
            return Err(CoreError::Inconsistent(format!(
                "Fichier modifié pendant le nettoyage ; fichier préservé : {}",
                path.display()
            )));
        }
        fs::remove_file(path)?;
        Ok(metadata.len())
    }

    pub(super) fn validate(&self) -> Result<()> {
        let mut previous: Option<&str> = None;
        for entry in &self.entries {
            safe_relative(&entry.path)?;
            if previous.is_some_and(|path| path >= entry.path.as_str())
                || (entry.directory && (entry.size != 0 || entry.modified.is_some()))
            {
                return Err(CoreError::Inconsistent(
                    "Inventaire de suppression invalide.".into(),
                ));
            }
            previous = Some(&entry.path);
        }
        Ok(())
    }

    /// Missing entries are allowed only in staging, after a partial cleanup.
    pub(super) fn verify(&self, path: &Path, partial: bool) -> Result<Self> {
        let current = Self::capture(path)?;
        let matches = self.identity == current.identity
            && if partial {
                current.entries.iter().all(|entry| {
                    self.entries
                        .binary_search_by(|expected| expected.path.cmp(&entry.path))
                        .is_ok_and(|index| self.entries[index] == *entry)
                })
            } else {
                self.entries == current.entries
            };
        if !matches {
            return Err(CoreError::Inconsistent(format!(
                "Le workspace a été remplacé ou modifié ; fichiers conservés pour récupération : {}",
                path.display()
            )));
        }
        Ok(current)
    }
}

#[cfg(windows)]
fn identity(path: &Path) -> Result<WorkspaceIdentity> {
    use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileInformationByHandle,
    };
    ensure_real_directory(path)?;
    let file = OpenOptions::new()
        .access_mode(0)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    // The File owns a valid handle and the output points to an initialized structure.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
    {
        return Err(CoreError::Invalid(
            "Lien interdit dans le workspace à supprimer.".into(),
        ));
    }
    let index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
    let created = (u64::from(info.ftCreationTime.dwHighDateTime) << 32)
        | u64::from(info.ftCreationTime.dwLowDateTime);
    if index == 0 || created == 0 {
        return Err(CoreError::Invalid(
            "Identité durable du workspace indisponible ; fichiers préservés.".into(),
        ));
    }
    Ok(WorkspaceIdentity {
        volume: info.dwVolumeSerialNumber.into(),
        index,
        created: Some(created),
    })
}

#[cfg(unix)]
fn identity(path: &Path) -> Result<WorkspaceIdentity> {
    use std::os::unix::fs::MetadataExt;
    ensure_real_directory(path)?;
    let metadata = fs::symlink_metadata(path)?;
    Ok(WorkspaceIdentity {
        volume: metadata.dev(),
        index: metadata.ino(),
        created: metadata
            .created()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .and_then(|duration| duration.as_nanos().try_into().ok()),
    })
}

#[cfg(not(any(windows, unix)))]
fn identity(_path: &Path) -> Result<WorkspaceIdentity> {
    Err(CoreError::Invalid(
        "Identité du workspace indisponible sur cette plateforme.".into(),
    ))
}
