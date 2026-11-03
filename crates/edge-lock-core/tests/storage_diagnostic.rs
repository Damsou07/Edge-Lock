use edge_lock_core::{CoreError, ExportConfig, Library, OperationContext, StorageDiagnostic};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::SystemTime,
};

fn detail(report: &StorageDiagnostic, key: &str) -> (u64, u64) {
    let value = report.details.iter().find(|d| d.key == key).unwrap();
    (value.count, value.bytes)
}

fn write(root: &Path, path: &str, bytes: &[u8]) {
    let destination = root.join(path);
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    fs::write(destination, bytes).unwrap();
}

fn object(root: &Path, byte: u8, compressed: bool, bytes: &[u8]) -> String {
    let hash = format!("{byte:02x}").repeat(32);
    write(
        root,
        &format!(
            "objects/{}/{hash}{}",
            &hash[..2],
            if compressed { ".zst" } else { "" }
        ),
        bytes,
    );
    hash
}

fn manifest(root: &Path) -> BTreeMap<PathBuf, (u64, String, SystemTime)> {
    let mut files = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            let meta = fs::symlink_metadata(&path).unwrap();
            if meta.is_dir() {
                pending.push(path);
            } else if meta.is_file() {
                // Windows range locks make the open catalog and library lock unreadable
                // through a second file handle. Compare their sizes and mtimes; hash
                // every asset, object, cache, backup and journal payload normally.
                let locked_metadata = path.parent() == Some(root)
                    && path.file_name().is_some_and(|name| {
                        name == "library.lock"
                            || name.to_string_lossy().starts_with("library.sqlite3")
                    });
                files.insert(
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    (
                        meta.len(),
                        match fs::read(&path) {
                            // Reading a new WAL snapshot updates SQLite's shared-memory
                            // read marks. They are not persistent catalog/user content.
                            Ok(_)
                                if path
                                    .file_name()
                                    .is_some_and(|name| name == "library.sqlite3-shm") =>
                            {
                                "volatile SQLite read marks".into()
                            }
                            Ok(bytes) => blake3::hash(&bytes).to_hex().to_string(),
                            Err(error) if locked_metadata && error.raw_os_error() == Some(33) => {
                                "locked Windows range".into()
                            }
                            Err(error) => panic!("{}: {error}", path.display()),
                        },
                        meta.modified().unwrap(),
                    ),
                );
            }
        }
    }
    files
}

fn catalog_content(root: &Path) -> String {
    let db = rusqlite::Connection::open_with_flags(
        root.join("library.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let mut hasher = blake3::Hasher::new();
    for table in [
        "assets",
        "variants",
        "snapshots",
        "snapshot_entries",
        "garbage_candidates",
    ] {
        let mut statement = db
            .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
            .unwrap();
        let columns = statement.column_count();
        let mut rows = statement.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            for column in 0..columns {
                hasher.update(format!("{:?}", row.get_ref(column).unwrap()).as_bytes());
            }
        }
    }
    hasher.finalize().to_hex().to_string()
}

#[test]
fn inventories_shared_raw_zstd_missing_orphan_and_unknown_objects_without_writes() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("library");
    let library = Library::open(&root).unwrap();
    let source = temp.path().join("source");
    fs::create_dir(&source).unwrap();
    let blend = vec![7; 256 * 1024];
    fs::write(source.join("main.blend"), &blend).unwrap();
    fs::write(source.join("notes.txt"), b"tiny").unwrap();
    for name in ["First", "Second"] {
        library
            .import_folder(
                &source,
                name,
                ExportConfig::default(),
                &OperationContext::default(),
            )
            .unwrap();
    }
    let hash = blake3::hash(&blend).to_hex().to_string();
    let raw = format!("objects/{}/{hash}", &hash[..2]);
    // Retain both representations as can happen after an interrupted recompression.
    fs::write(root.join(&raw), &blend).unwrap();
    let encoded_size = fs::metadata(root.join(format!("{raw}.zst"))).unwrap().len();
    let missing = blake3::hash(b"tiny").to_hex().to_string();
    fs::remove_file(root.join(format!("objects/{}/{missing}", &missing[..2]))).unwrap();
    object(&root, 0x12, false, b"orphan");
    let candidate = object(&root, 0x34, true, b"candidate");
    write(&root, "objects/unknown.bin", b"unknown");
    let db = rusqlite::Connection::open(root.join("library.sqlite3")).unwrap();
    db.execute("INSERT INTO garbage_candidates VALUES(?1,9)", [&candidate])
        .unwrap();
    // A referenced candidate must never be reported as reclaimable.
    db.execute("INSERT INTO garbage_candidates VALUES(?1,262144)", [&hash])
        .unwrap();
    drop(db);
    let before_catalog = catalog_content(&root);
    let before = manifest(&root);
    let report = library
        .storage_diagnostic(&OperationContext::default())
        .unwrap();
    let after = manifest(&root);
    assert_eq!(before_catalog, catalog_content(&root));
    assert_eq!(
        before, after,
        "diagnostic modified a library file or its mtime"
    );
    assert!(report.complete);
    assert!(report.purge_available);
    assert_eq!(report.logical_object_bytes, blend.len() as u64 + 4);
    assert_eq!(
        detail(&report, "referencedObjects"),
        (1, blend.len() as u64 + encoded_size)
    );
    assert_eq!(
        detail(&report, "dualRepresentations"),
        (1, blend.len() as u64 + encoded_size)
    );
    assert_eq!(detail(&report, "unreferencedObjects"), (1, 6));
    assert_eq!(detail(&report, "purgeCandidates"), (1, 9));
    assert_eq!(detail(&report, "missingObjects"), (1, 0));
    assert_eq!(detail(&report, "unknownObjects"), (1, 7));
    assert_eq!(
        report.total.bytes,
        report.categories.iter().map(|c| c.bytes).sum::<u64>()
    );
    assert_eq!(
        report.total.file_count,
        report.categories.iter().map(|c| c.file_count).sum::<u64>()
    );
    assert_eq!(
        report.total.bytes,
        before.values().map(|(bytes, _, _)| bytes).sum::<u64>()
    );
}

#[test]
fn accounts_full_recovery_trees_cache_and_backups_and_remains_read_only_with_journals() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("library");
    let library = Library::open(&root).unwrap();
    for (path, bytes) in [
        ("cache/canonical/a.glb", 11),
        ("cache/thumbnails/a.png", 12),
        ("cache/previews/index-a.json", 13),
        ("cache/sync/a.json", 14),
        ("cache/unknown/nested/b.glb", 15),
        ("backups/catalog-schema-1-a.sqlite3", 16),
        ("backups/glb/a.glb", 17),
        ("backups/other.bin", 18),
        ("staging/import/operation.json", 19),
        ("staging/import/source/main.blend", 20),
        ("staging/restore-a/restore.json", 21),
        ("staging/restore-a/previous/main.blend", 22),
        ("staging/abandoned/source/main.blend", 23),
    ] {
        write(&root, path, &vec![1; bytes]);
    }
    let before = manifest(&root);
    let report = library
        .storage_diagnostic(&OperationContext::default())
        .unwrap();
    assert_eq!(before, manifest(&root));
    assert!(report.complete);
    assert!(!report.purge_available);
    assert!(report.purge_blocked_reason.is_some());
    assert!(!report.warnings.is_empty());
    assert_eq!(detail(&report, "cacheBudgeted"), (2, 23));
    assert_eq!(detail(&report, "cacheOther"), (3, 42));
    assert_eq!(detail(&report, "backupCatalogs"), (1, 16));
    assert_eq!(detail(&report, "backupGlbs"), (1, 17));
    assert_eq!(detail(&report, "backupOther"), (1, 18));
    assert_eq!(detail(&report, "publicationRecovery"), (2, 39));
    assert_eq!(detail(&report, "restorationRecovery"), (2, 43));
    assert_eq!(detail(&report, "stagingOther"), (1, 23));
}

#[test]
fn supports_empty_libraries_and_bounds_examples_without_losing_totals() {
    let temp = tempfile::tempdir().unwrap();
    let library = Library::open(temp.path().join("library")).unwrap();
    let empty = library
        .storage_diagnostic(&OperationContext::default())
        .unwrap();
    assert!(empty.complete);
    assert_eq!(empty.logical_object_bytes, 0);
    assert_eq!(detail(&empty, "referencedObjects"), (0, 0));
    for index in 1..=31 {
        object(library.root(), index, false, &vec![0; index as usize]);
    }
    let report = library
        .storage_diagnostic(&OperationContext::default())
        .unwrap();
    assert_eq!(detail(&report, "unreferencedObjects"), (31, (1..=31).sum()));
    let examples = &report
        .details
        .iter()
        .find(|d| d.key == "unreferencedObjects")
        .unwrap()
        .examples;
    assert_eq!(examples.len(), 20);
    assert_eq!(examples.first().unwrap().size, 31);
    assert_eq!(examples.last().unwrap().size, 12);
}

#[test]
fn cancellation_during_inventory_does_not_modify_library_files() {
    let temp = tempfile::tempdir().unwrap();
    let library = Library::open(temp.path().join("library")).unwrap();
    write(library.root(), "workspaces/a/valuable.txt", b"valuable");
    let before = manifest(library.root());
    let cancelled = Arc::new(AtomicBool::new(false));
    let flag = cancelled.clone();
    let context = OperationContext {
        cancelled,
        progress: Some(Arc::new(move |progress| {
            if progress.completed >= 1 {
                flag.store(true, Ordering::Relaxed);
            }
        })),
    };
    assert!(matches!(
        library.storage_diagnostic(&context),
        Err(CoreError::Cancelled)
    ));
    assert_eq!(before, manifest(library.root()));
}

#[test]
fn a_directory_disappearing_during_inventory_is_reported_as_partial() {
    let temp = tempfile::tempdir().unwrap();
    let library = Library::open(temp.path().join("library")).unwrap();
    write(library.root(), "workspaces/valuable.txt", b"keep me");
    let original = library.root().join("workspaces");
    let moved = temp.path().join("moved-workspaces");
    let original_in_progress = original.clone();
    let moved_in_progress = moved.clone();
    let moved_once = Arc::new(AtomicBool::new(false));
    let flag = moved_once.clone();
    let context = OperationContext {
        cancelled: Arc::new(AtomicBool::new(false)),
        progress: Some(Arc::new(move |progress| {
            if progress.phase == "Diagnostic : inventaire des fichiers"
                && !flag.swap(true, Ordering::Relaxed)
            {
                // Root entries have been enumerated, but no child directory is open yet.
                // Simulate an external editor moving a workspace while the metadata scan runs.
                fs::rename(&original_in_progress, &moved_in_progress).unwrap();
            }
        })),
    };
    let result = library.storage_diagnostic(&context);
    fs::rename(&moved, &original).unwrap();
    let report = result.unwrap();
    assert!(moved_once.load(Ordering::Relaxed));
    assert!(!report.complete);
    assert!(!report.purge_available);
    assert!(detail(&report, "readErrors").0 >= 1);
    assert!(report.purge_blocked_reason.is_some());
    assert_eq!(fs::read(original.join("valuable.txt")).unwrap(), b"keep me");
}

#[cfg(windows)]
#[test]
#[ignore = "requires Windows junction creation permission; disposable fixture only"]
fn ignores_junction_targets_and_reports_partial_totals() {
    use std::os::windows::process::CommandExt;
    let temp = tempfile::tempdir().unwrap();
    let library = Library::open(temp.path().join("library")).unwrap();
    let external = temp.path().join("outside");
    fs::create_dir(&external).unwrap();
    fs::write(external.join("valuable.txt"), vec![1; 2048]).unwrap();
    let junction = library.root().join("workspaces/junction");
    let output = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&junction)
        .arg(&external)
        .creation_flags(0x08000000)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = library
        .storage_diagnostic(&OperationContext::default())
        .unwrap();
    assert!(!report.complete);
    assert!(!report.purge_available);
    assert_eq!(detail(&report, "ignoredPaths"), (1, 0));
    assert_eq!(
        report
            .categories
            .iter()
            .find(|c| c.key == "workspaces")
            .unwrap()
            .bytes,
        0
    );
    assert_eq!(
        fs::read(external.join("valuable.txt")).unwrap(),
        vec![1; 2048]
    );
    // Remove only the junction itself before TempDir recursively removes the fixture.
    fs::remove_dir(junction).unwrap();
}
