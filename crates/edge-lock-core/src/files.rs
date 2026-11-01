use crate::{CoreError, ManifestEntry, OperationContext, Result};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone)]
pub(crate) struct FileStore {
    pub objects: PathBuf,
    pub staging: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Fingerprint {
    directory: bool,
    size: u64,
    modified: Option<SystemTime>,
}

pub(crate) fn safe_relative(value: &str) -> Result<PathBuf> {
    if value.is_empty() || value.contains('\\') || value.contains(':') || value.contains('\0') {
        return Err(CoreError::Invalid(format!(
            "Chemin relatif invalide : {value}"
        )));
    }
    let path = Path::new(value);
    if path
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(CoreError::Invalid(format!(
            "Le chemin sort du dossier : {value}"
        )));
    }
    Ok(path.to_path_buf())
}

fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

pub(crate) fn ensure_real_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || is_link(&metadata) {
        return Err(CoreError::Invalid(format!(
            "Dossier absent ou lien non pris en charge : {}",
            path.display()
        )));
    }
    Ok(())
}

fn inventory(root: &Path, ctx: &OperationContext) -> Result<BTreeMap<String, Fingerprint>> {
    let root_meta = fs::symlink_metadata(root)?;
    if !root_meta.is_dir() || is_link(&root_meta) {
        return Err(CoreError::Invalid(
            "Le dossier doit être un dossier réel, sans lien symbolique ou junction.".into(),
        ));
    }
    let mut result = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        ctx.check_cancelled()?;
        for item in fs::read_dir(directory)? {
            let item = item?;
            let path = item.path();
            let metadata = fs::symlink_metadata(&path)?;
            if is_link(&metadata) || (!metadata.is_file() && !metadata.is_dir()) {
                return Err(CoreError::Invalid(format!(
                    "Lien ou fichier spécial non pris en charge : {}",
                    path.display()
                )));
            }
            let relative = path
                .strip_prefix(root)
                .map_err(|_| CoreError::Invalid("Chemin hors du dossier.".into()))?;
            let value = relative
                .to_str()
                .ok_or_else(|| {
                    CoreError::Invalid("Nom de fichier non Unicode non pris en charge.".into())
                })?
                .replace('\\', "/");
            safe_relative(&value)?;
            if metadata.is_dir() {
                stack.push(path);
            }
            result.insert(
                value,
                Fingerprint {
                    directory: metadata.is_dir(),
                    size: if metadata.is_dir() { 0 } else { metadata.len() },
                    modified: if metadata.is_dir() {
                        None
                    } else {
                        metadata.modified().ok()
                    },
                },
            );
        }
    }
    Ok(result)
}

impl FileStore {
    pub fn object_path(&self, hash: &str) -> Result<PathBuf> {
        if hash.len() != 64
            || !hash
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(CoreError::Inconsistent("Hash d’objet invalide.".into()));
        }
        ensure_real_directory(&self.objects)?;
        let parent = self.objects.join(&hash[..2]);
        if fs::symlink_metadata(&parent).is_ok() {
            ensure_real_directory(&parent)?;
        }
        Ok(parent.join(hash))
    }

    pub fn capture(
        &self,
        root: &Path,
        persist: bool,
        ctx: &OperationContext,
    ) -> Result<Vec<ManifestEntry>> {
        // Retrying detects normal Blender saves; it cannot freeze arbitrary external writers.
        let mut last_error = None;
        for _ in 0..2 {
            match self.capture_once(root, persist, ctx) {
                Err(error @ CoreError::Inconsistent(_)) => last_error = Some(error),
                result => return result,
            }
        }
        Err(last_error.expect("capture retry error"))
    }

    fn capture_once(
        &self,
        root: &Path,
        persist: bool,
        ctx: &OperationContext,
    ) -> Result<Vec<ManifestEntry>> {
        ctx.check_cancelled()?;
        if persist {
            ensure_real_directory(&self.staging)?;
        }
        let before = inventory(root, ctx)?;
        let total: u64 = before.values().map(|item| item.size).sum();
        let mut completed = 0;
        let mut entries = Vec::with_capacity(before.len());
        for (relative, fingerprint) in &before {
            ctx.check_cancelled()?;
            if fingerprint.directory {
                entries.push(ManifestEntry {
                    path: relative.clone(),
                    kind: "directory".into(),
                    hash: None,
                    size: 0,
                });
                continue;
            }
            let path = root.join(safe_relative(relative)?);
            let metadata = fs::symlink_metadata(&path)?;
            if is_link(&metadata)
                || metadata.len() != fingerprint.size
                || metadata.modified().ok() != fingerprint.modified
            {
                return Err(CoreError::Inconsistent(format!(
                    "Fichier modifié pendant la capture : {relative}. Enregistrez et réessayez."
                )));
            }
            let mut input = File::open(&path)?;
            let mut temp = if persist {
                Some(tempfile::NamedTempFile::new_in(&self.staging)?)
            } else {
                None
            };
            let mut hasher = blake3::Hasher::new();
            let mut buffer = vec![0u8; 1024 * 1024];
            let mut size = 0u64;
            loop {
                ctx.check_cancelled()?;
                let count = input.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                hasher.update(&buffer[..count]);
                if let Some(output) = &mut temp {
                    output.write_all(&buffer[..count])?;
                }
                size += count as u64;
                completed += count as u64;
                ctx.report("capture", completed, Some(total));
            }
            let after = fs::symlink_metadata(&path)?;
            if is_link(&after)
                || size != fingerprint.size
                || after.len() != fingerprint.size
                || after.modified().ok() != fingerprint.modified
            {
                return Err(CoreError::Inconsistent(format!(
                    "Fichier modifié pendant la capture : {relative}. Enregistrez et réessayez."
                )));
            }
            let hash = hasher.finalize().to_hex().to_string();
            if let Some(temp) = temp {
                temp.as_file().sync_all()?;
                let destination = self.object_path(&hash)?;
                fs::create_dir_all(destination.parent().expect("object parent"))?;
                if destination.exists() {
                    self.verify_object(&hash, size, ctx)?;
                } else {
                    temp.persist_noclobber(&destination)
                        .map_err(|error| CoreError::Io(error.error))?;
                }
            }
            entries.push(ManifestEntry {
                path: relative.clone(),
                kind: "file".into(),
                hash: Some(hash),
                size,
            });
        }
        if before != inventory(root, ctx)? {
            return Err(CoreError::Inconsistent(
                "Le dossier a changé pendant la capture. Enregistrez dans Blender puis réessayez."
                    .into(),
            ));
        }
        ctx.report("capture", total, Some(total));
        Ok(entries)
    }

    fn verify_object(&self, hash: &str, expected_size: u64, ctx: &OperationContext) -> Result<()> {
        let path = self.object_path(hash)?;
        let metadata = fs::symlink_metadata(&path)?;
        if is_link(&metadata) || !metadata.is_file() || metadata.len() != expected_size {
            return Err(CoreError::Inconsistent(format!(
                "Objet absent ou corrompu : {hash}"
            )));
        }
        let mut input = File::open(path)?;
        let mut hasher = blake3::Hasher::new();
        let mut buffer = vec![0u8; 1024 * 1024];
        loop {
            ctx.check_cancelled()?;
            let count = input.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }
        if hasher.finalize().to_hex().as_str() != hash {
            return Err(CoreError::Inconsistent(format!("Objet corrompu : {hash}")));
        }
        Ok(())
    }

    pub fn materialize(
        &self,
        entries: &[ManifestEntry],
        destination: &Path,
        ctx: &OperationContext,
    ) -> Result<()> {
        ctx.check_cancelled()?;
        if fs::symlink_metadata(destination).is_ok() {
            return Err(CoreError::Invalid(
                "Le dossier de destination existe déjà ; choisissez un nouveau dossier.".into(),
            ));
        }
        let parent = destination
            .parent()
            .ok_or_else(|| CoreError::Invalid("Destination invalide.".into()))?;
        fs::create_dir_all(parent)?;
        let staging = tempfile::Builder::new()
            .prefix(".edge-lock-restore-")
            .tempdir_in(parent)?;
        let total: u64 = entries.iter().map(|entry| entry.size).sum();
        let mut completed = 0;
        for entry in entries {
            ctx.check_cancelled()?;
            let path = staging.path().join(safe_relative(&entry.path)?);
            match entry.kind.as_str() {
                "directory" => fs::create_dir_all(path)?,
                "file" => {
                    let hash = entry
                        .hash
                        .as_deref()
                        .ok_or_else(|| CoreError::Inconsistent("Fichier sans hash.".into()))?;
                    let source = self.object_path(hash)?;
                    let meta = fs::symlink_metadata(&source)?;
                    if is_link(&meta) || !meta.is_file() {
                        return Err(CoreError::Inconsistent("Objet invalide.".into()));
                    }
                    fs::create_dir_all(path.parent().expect("file parent"))?;
                    let mut input = File::open(source)?;
                    let mut output = File::create(path)?;
                    let mut hasher = blake3::Hasher::new();
                    let mut buffer = vec![0u8; 1024 * 1024];
                    let mut size = 0u64;
                    loop {
                        ctx.check_cancelled()?;
                        let count = input.read(&mut buffer)?;
                        if count == 0 {
                            break;
                        }
                        output.write_all(&buffer[..count])?;
                        hasher.update(&buffer[..count]);
                        size += count as u64;
                        completed += count as u64;
                        ctx.report("materialize", completed, Some(total));
                    }
                    output.sync_all()?;
                    if size != entry.size || hasher.finalize().to_hex().as_str() != hash {
                        return Err(CoreError::Inconsistent(format!("Objet corrompu : {hash}")));
                    }
                }
                _ => return Err(CoreError::Inconsistent("Type d’entrée inconnu.".into())),
            }
        }
        ctx.check_cancelled()?;
        if fs::symlink_metadata(destination).is_ok() {
            return Err(CoreError::Invalid(
                "La destination vient d’être créée par un autre processus.".into(),
            ));
        }
        fs::rename(staging.path(), destination).map_err(|error| {
            CoreError::External(format!(
                "Publication du dossier {} impossible : {error}",
                destination.display()
            ))
        })?;
        ctx.report("materialize", total, Some(total));
        Ok(())
    }
}

pub(crate) fn directory_bytes(root: &Path) -> Result<u64> {
    let mut total = 0u64;
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let metadata = fs::symlink_metadata(entry.path())?;
            if is_link(&metadata) {
                continue;
            }
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if metadata.is_file() {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    Ok(total)
}

pub(crate) fn list_files(root: &Path) -> Result<Vec<ManifestEntry>> {
    Ok(inventory(root, &OperationContext::default())?
        .into_iter()
        .map(|(path, item)| ManifestEntry {
            path,
            kind: if item.directory { "directory" } else { "file" }.into(),
            hash: None,
            size: item.size,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_paths_cannot_escape_or_address_windows_streams() {
        for path in [
            "",
            "../secret",
            "/absolute",
            "C:/secret",
            "file:stream",
            "a\\..\\b",
            "a/../../b",
        ] {
            assert!(safe_relative(path).is_err(), "accepted unsafe path: {path}");
        }
        assert!(safe_relative("textures/café.png").is_ok());
    }
}
