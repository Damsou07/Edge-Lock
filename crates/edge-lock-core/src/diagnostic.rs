//! Metadata-only inventory. No directory creation, recovery, hashing or cleanup.
use super::*;

const EXAMPLE_LIMIT: usize = 20;
const WARNING_LIMIT: usize = 20;
const CATEGORIES: &[(&str, &str)] = &[
    ("workspaces", "Dossiers de travail"),
    ("objects", "Objets de l’historique"),
    ("cache", "Cache"),
    ("backups", "Sauvegardes préservées"),
    ("staging", "Préparation et récupération"),
    ("catalog", "Catalogue et fichiers SQLite auxiliaires"),
    ("other", "Autres fichiers"),
];
const DETAILS: &[(&str, &str)] = &[
    ("referencedObjects", "Objets référencés par les versions"),
    (
        "purgeCandidates",
        "Objets sans référence déjà candidats à la purge",
    ),
    ("unreferencedObjects", "Objets sans référence à examiner"),
    ("dualRepresentations", "Objets présents en brut et en Zstd"),
    (
        "missingObjects",
        "Objets référencés non observés sur disque",
    ),
    ("unknownObjects", "Fichiers non reconnus dans objects"),
    (
        "cacheBudgeted",
        "GLB et PNG couverts par le budget du cache",
    ),
    ("cacheOther", "Autres fichiers du cache"),
    ("backupCatalogs", "Catalogues sauvegardés avant migration"),
    ("backupGlbs", "GLB préservés avant remplacement"),
    ("backupOther", "Autres sauvegardes"),
    (
        "publicationRecovery",
        "Fichiers associés à un journal de publication",
    ),
    (
        "restorationRecovery",
        "Fichiers associés à un journal de restauration",
    ),
    (
        "discardRecovery",
        "Fichiers associés à un abandon de modifications",
    ),
    ("stagingOther", "Autres fichiers temporaires à examiner"),
    ("ignoredPaths", "Liens et fichiers spéciaux ignorés"),
    ("readErrors", "Chemins non lisibles"),
];

#[derive(Default)]
struct Presence {
    raw: Option<StorageDiagnosticExample>,
    zstd: Option<StorageDiagnosticExample>,
}

struct Scan<'a> {
    root: &'a Path,
    ctx: &'a OperationContext,
    report: StorageDiagnostic,
    objects: BTreeMap<String, Presence>,
    visited: u64,
    last_progress: std::time::Instant,
}

fn linked(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_type().is_symlink() || meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl Scan<'_> {
    fn relative(&self, path: &Path) -> String {
        path.strip_prefix(self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }

    fn warning(&mut self, text: String) {
        if self.report.warnings.len() < WARNING_LIMIT {
            self.report.warnings.push(text);
        }
    }

    fn detail(&mut self, key: &str, path: String, bytes: u64) {
        let detail = self
            .report
            .details
            .iter_mut()
            .find(|d| d.key == key)
            .expect("diagnostic detail");
        detail.count += 1;
        detail.bytes = detail.bytes.saturating_add(bytes);
        detail
            .examples
            .push(StorageDiagnosticExample { path, size: bytes });
        detail
            .examples
            .sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.path.cmp(&b.path)));
        detail.examples.truncate(EXAMPLE_LIMIT);
    }

    fn read_error(&mut self, path: &Path, error: impl std::fmt::Display) {
        self.report.complete = false;
        let relative = self.relative(path);
        self.detail("readErrors", relative.clone(), 0);
        self.warning(format!("{relative} : {error}"));
    }

    fn recovery(&mut self, path: &Path) -> Option<&'static str> {
        for (marker, key) in [
            ("restore.json", "restorationRecovery"),
            ("discard.json", "discardRecovery"),
            ("operation.json", "publicationRecovery"),
        ] {
            let marker = path.join(marker);
            match fs::symlink_metadata(&marker) {
                Ok(meta) if meta.is_file() && !linked(&meta) => return Some(key),
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => self.read_error(&marker, error),
            }
        }
        None
    }

    fn walk(&mut self) -> Result<()> {
        let mut stack = vec![(self.root.to_path_buf(), None)];
        while let Some((path, mut recovery)) = stack.pop() {
            self.ctx.check_cancelled()?;
            let meta = match fs::symlink_metadata(&path) {
                Ok(meta) => meta,
                Err(error) => {
                    self.read_error(&path, error);
                    continue;
                }
            };
            if linked(&meta) || (!meta.is_file() && !meta.is_dir()) {
                self.report.complete = false;
                self.detail("ignoredPaths", self.relative(&path), 0);
                continue;
            }
            if meta.is_dir() {
                if path.parent() == Some(self.root.join("staging").as_path()) {
                    recovery = self.recovery(&path);
                }
                match fs::read_dir(&path) {
                    Ok(entries) => {
                        for entry in entries {
                            self.ctx.check_cancelled()?;
                            match entry {
                                Ok(entry) => {
                                    let child = entry.path();
                                    match fs::symlink_metadata(&child) {
                                        Ok(meta) if meta.is_dir() && !linked(&meta) => {
                                            stack.push((child, recovery))
                                        }
                                        Ok(meta) => self.file(&child, &meta, recovery),
                                        Err(error) => self.read_error(&child, error),
                                    }
                                }
                                Err(error) => self.read_error(&path, error),
                            }
                        }
                    }
                    Err(error) => self.read_error(&path, error),
                }
                continue;
            }
            self.file(&path, &meta, recovery);
        }
        Ok(())
    }

    fn file(&mut self, path: &Path, meta: &fs::Metadata, recovery: Option<&'static str>) {
        if linked(meta) || !meta.is_file() {
            self.report.complete = false;
            self.detail("ignoredPaths", self.relative(path), 0);
            return;
        }
        self.visited += 1;
        if self.visited == 1
            || self.last_progress.elapsed() >= std::time::Duration::from_millis(100)
        {
            self.ctx
                .report("Diagnostic : inventaire des fichiers", self.visited, None);
            self.last_progress = std::time::Instant::now();
        }
        let relative = self.relative(path);
        let parts: Vec<_> = relative.split('/').collect();
        let category = match parts[0] {
            "workspaces" | "objects" | "cache" | "backups" | "staging" if parts.len() > 1 => {
                parts[0]
            }
            "library.sqlite3"
            | "library.sqlite3-wal"
            | "library.sqlite3-shm"
            | "library.sqlite3-journal"
                if parts.len() == 1 =>
            {
                "catalog"
            }
            _ => "other",
        };
        let bytes = meta.len();
        let group = self
            .report
            .categories
            .iter_mut()
            .find(|c| c.key == category)
            .expect("diagnostic category");
        group.file_count += 1;
        group.bytes = group.bytes.saturating_add(bytes);
        self.report.total.file_count += 1;
        self.report.total.bytes = self.report.total.bytes.saturating_add(bytes);
        match category {
            "objects" => {
                let name = parts.last().expect("file name");
                let hash = name.strip_suffix(".zst").unwrap_or(name);
                if parts.len() == 3 && valid_hash(hash) && parts[1] == &hash[..2] {
                    let object = self.objects.entry(hash.into()).or_default();
                    let compressed = name.ends_with(".zst");
                    let example = StorageDiagnosticExample {
                        path: relative,
                        size: bytes,
                    };
                    if compressed {
                        object.zstd = Some(example);
                    } else {
                        object.raw = Some(example);
                    }
                } else {
                    self.detail("unknownObjects", relative, bytes);
                }
            }
            "cache" => {
                let eligible = (parts.len() == 2
                    || (parts.len() == 3
                        && ["previews", "exports", "thumbnails", "canonical"].contains(&parts[1])))
                    && path
                        .extension()
                        .is_some_and(|ext| ext == "glb" || ext == "png");
                self.detail(
                    if eligible {
                        "cacheBudgeted"
                    } else {
                        "cacheOther"
                    },
                    relative,
                    bytes,
                );
            }
            "backups" => {
                let key = if parts.len() == 2
                    && parts[1].starts_with("catalog-schema-")
                    && parts[1].ends_with(".sqlite3")
                {
                    "backupCatalogs"
                } else if parts.len() == 3 && parts[1] == "glb" && parts[2].ends_with(".glb") {
                    "backupGlbs"
                } else {
                    "backupOther"
                };
                self.detail(key, relative, bytes);
            }
            "staging" => self.detail(recovery.unwrap_or("stagingOther"), relative, bytes),
            _ => {}
        }
    }
}

impl Library {
    /// Read-only metadata inventory of the already-open library. External editors may still write.
    pub fn storage_diagnostic(&self, ctx: &OperationContext) -> Result<StorageDiagnostic> {
        ctx.check_cancelled()?;
        let started_at = now();
        let _guard = self
            .mutation
            .lock()
            .map_err(|_| CoreError::Inconsistent("Verrou du catalogue indisponible.".into()))?;
        ctx.report("Diagnostic : lecture des références", 0, None);
        let (references, candidates) = {
            let db = self.db()?;
            let mut references = BTreeMap::<String, u64>::new();
            let mut statement = db.prepare(
                "SELECT hash,MAX(size) FROM snapshot_entries WHERE kind='file' GROUP BY hash",
            )?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                ctx.check_cancelled()?;
                references.insert(row.get(0)?, sql_u64(row, 1)?);
            }
            let mut candidates = BTreeSet::<String>::new();
            let mut statement = db.prepare("SELECT hash FROM garbage_candidates")?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                ctx.check_cancelled()?;
                candidates.insert(row.get(0)?);
            }
            (references, candidates)
        };
        let mut scan = Scan {
            root: &self.root,
            ctx,
            report: StorageDiagnostic {
                started_at,
                completed_at: String::new(),
                complete: true,
                total: StorageMeasure::default(),
                categories: CATEGORIES
                    .iter()
                    .map(|(key, label)| StorageDiagnosticCategory {
                        key: (*key).into(),
                        label: (*label).into(),
                        file_count: 0,
                        bytes: 0,
                    })
                    .collect(),
                details: DETAILS
                    .iter()
                    .map(|(key, label)| StorageDiagnosticDetail {
                        key: (*key).into(),
                        label: (*label).into(),
                        count: 0,
                        bytes: 0,
                        examples: Vec::new(),
                    })
                    .collect(),
                logical_object_bytes: references.values().copied().fold(0u64, u64::saturating_add),
                purge_available: false,
                purge_blocked_reason: None,
                warnings: Vec::new(),
            },
            objects: BTreeMap::new(),
            visited: 0,
            last_progress: std::time::Instant::now(),
        };
        scan.walk()?;
        ctx.check_cancelled()?;
        // The existing purge checks probe staging entries. Run them only after a complete
        // no-link inventory, so known junctions are never deliberately followed here.
        if scan.report.complete {
            match self.check_storage_integrity() {
                Ok(()) => scan.report.purge_available = true,
                Err(error) => {
                    let reason = error.to_string();
                    scan.report.purge_blocked_reason = Some(reason.clone());
                    scan.warning(reason);
                }
            }
        }
        // Compare against the inventory rather than reopening paths: a parent directory
        // could be a junction, and an unreadable subtree must never be traversed here.
        for hash in references.keys() {
            ctx.check_cancelled()?;
            if !scan.objects.contains_key(hash) {
                let path = if valid_hash(hash) {
                    format!("objects/{}/{hash}", &hash[..2])
                } else {
                    format!("objects/{hash}")
                };
                scan.detail("missingObjects", path, 0);
            }
        }
        for (hash, presence) in std::mem::take(&mut scan.objects) {
            ctx.check_cancelled()?;
            let bytes = presence
                .raw
                .as_ref()
                .map_or(0, |p| p.size)
                .saturating_add(presence.zstd.as_ref().map_or(0, |p| p.size));
            let path = presence
                .raw
                .as_ref()
                .or(presence.zstd.as_ref())
                .expect("observed object")
                .path
                .clone();
            let key = if references.contains_key(&hash) {
                "referencedObjects"
            } else if candidates.contains(&hash) {
                "purgeCandidates"
            } else {
                "unreferencedObjects"
            };
            scan.detail(key, path.clone(), bytes);
            if presence.raw.is_some() && presence.zstd.is_some() {
                scan.detail("dualRepresentations", path, bytes);
            }
        }
        if scan
            .report
            .details
            .iter()
            .any(|d| d.key == "missingObjects" && d.count > 0)
        {
            scan.warning("Certains objets référencés n’ont pas été observés sur disque. Ce diagnostic ne vérifie pas les contenus ni leurs empreintes.".into());
        }
        if scan
            .report
            .details
            .iter()
            .any(|d| d.key == "ignoredPaths" && d.count > 0)
        {
            scan.warning("Des liens ou fichiers spéciaux ont été ignorés ; leurs cibles ne sont pas incluses dans les tailles.".into());
        }
        if !scan.report.complete {
            scan.report.purge_available = false;
            if scan.report.purge_blocked_reason.is_none() {
                scan.report.purge_blocked_reason =
                    Some("Diagnostic partiel : des chemins n’ont pas pu être inventoriés.".into());
            }
        }
        ctx.check_cancelled()?;
        scan.report.completed_at = now();
        ctx.report("Diagnostic terminé", scan.visited, Some(scan.visited));
        Ok(scan.report)
    }
}
