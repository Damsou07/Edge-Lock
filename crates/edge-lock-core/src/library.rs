use crate::{
    domain::{checked_name, new_id, now},
    files::{FileStore, directory_bytes, ensure_real_directory, list_files, safe_relative},
    *,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};

const FORMAT: &str = "edge-lock-library-v1\n";
const SCHEMA_VERSION: i64 = 1;
const SCHEMA: &str = r#"
CREATE TABLE categories(id TEXT PRIMARY KEY, name TEXT NOT NULL, parent_id TEXT REFERENCES categories(id));
CREATE TABLE games(id TEXT PRIMARY KEY, name TEXT NOT NULL COLLATE NOCASE UNIQUE);
CREATE TABLE assets(id TEXT PRIMARY KEY, name TEXT NOT NULL, category_id TEXT REFERENCES categories(id), archived INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE TABLE tags(name TEXT PRIMARY KEY);
CREATE TABLE asset_tags(asset_id TEXT NOT NULL REFERENCES assets(id), tag TEXT NOT NULL REFERENCES tags(name), PRIMARY KEY(asset_id,tag));
CREATE TABLE asset_games(asset_id TEXT NOT NULL REFERENCES assets(id), game_id TEXT NOT NULL REFERENCES games(id), PRIMARY KEY(asset_id,game_id));
CREATE TABLE variants(id TEXT PRIMARY KEY, asset_id TEXT NOT NULL REFERENCES assets(id), name TEXT NOT NULL, workspace TEXT NOT NULL UNIQUE, head_snapshot_id TEXT NOT NULL, config TEXT NOT NULL, working_bytes INTEGER NOT NULL DEFAULT 0, triangles INTEGER, created_at TEXT NOT NULL, UNIQUE(asset_id,name));
CREATE TABLE snapshots(id TEXT PRIMARY KEY, variant_id TEXT NOT NULL REFERENCES variants(id), parent_id TEXT REFERENCES snapshots(id), created_at TEXT NOT NULL, message TEXT NOT NULL, logical_bytes INTEGER NOT NULL, config TEXT NOT NULL);
CREATE TABLE snapshot_entries(snapshot_id TEXT NOT NULL REFERENCES snapshots(id), path TEXT NOT NULL, kind TEXT NOT NULL CHECK(kind IN ('file','directory')), hash TEXT, size INTEGER NOT NULL, PRIMARY KEY(snapshot_id,path));
CREATE INDEX variants_asset ON variants(asset_id);
CREATE INDEX snapshots_variant ON snapshots(variant_id,created_at);
CREATE INDEX snapshot_entries_hash ON snapshot_entries(hash);
CREATE INDEX assets_category ON assets(category_id);
PRAGMA user_version=1;
"#;

pub struct Library {
    root: PathBuf,
    store: FileStore,
    connection: Mutex<Connection>,
    mutation: Mutex<()>,
    _lock: File,
    recovery_notices: Vec<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct PublishJournal {
    variant_id: String,
    workspace: String,
}

impl Library {
    /// Root of the managed library. Does not scan the filesystem or acquire a DB lock.
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let input = root.as_ref();
        if input.exists() && !input.is_dir() {
            return Err(CoreError::Invalid(
                "La bibliothèque doit être un dossier.".into(),
            ));
        }
        fs::create_dir_all(input)?;
        let root = fs::canonicalize(input)?;
        let marker = root.join(".edge-lock-library");
        let initializing = root.join(".edge-lock-initializing");
        let new_library = !marker.exists();
        if !marker.exists() {
            if initializing.is_file() && fs::read_to_string(&initializing)? == FORMAT {
                let expected = [
                    ".edge-lock-initializing",
                    "library.lock",
                    "library.sqlite3",
                    "library.sqlite3-wal",
                    "library.sqlite3-shm",
                    "objects",
                    "staging",
                    "workspaces",
                ];
                for entry in fs::read_dir(&root)? {
                    let entry = entry?;
                    if !expected.iter().any(|name| entry.file_name() == *name) {
                        return Err(CoreError::Invalid("Initialisation interrompue : des fichiers non reconnus sont présents. Leur contenu a été préservé.".into()));
                    }
                }
            } else if fs::read_dir(&root)?.next().is_some() {
                return Err(CoreError::Invalid("Ce dossier contient déjà des fichiers. Choisissez un dossier vide ou une bibliothèque Edge Lock existante.".into()));
            } else {
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&initializing)?;
                file.write_all(FORMAT.as_bytes())?;
                file.sync_all()?;
            }
        } else if fs::read_to_string(&marker)? != FORMAT {
            return Err(CoreError::Invalid(
                "Format de bibliothèque inconnu ou plus récent.".into(),
            ));
        }
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("library.lock"))?;
        fs2::FileExt::try_lock_exclusive(&lock).map_err(|_| {
            CoreError::Busy("Cette bibliothèque est déjà ouverte dans une autre instance.".into())
        })?;
        if !new_library && !root.join("library.sqlite3").is_file() {
            return Err(CoreError::Inconsistent("Le catalogue de la bibliothèque est absent. Les fichiers ont été préservés ; restaurez votre sauvegarde.".into()));
        }
        for folder in ["objects", "staging", "workspaces"] {
            let path = root.join(folder);
            if new_library && !path.exists() {
                fs::create_dir(&path)?;
            }
            ensure_real_directory(&path)?;
        }
        let mut connection = Connection::open(root.join("library.sqlite3"))?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if !(0..=SCHEMA_VERSION).contains(&version) {
            return Err(CoreError::Invalid(
                "Cette bibliothèque a été créée avec une version plus récente d’Edge Lock.".into(),
            ));
        }
        connection.execute_batch(
            "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;",
        )?;
        apply_migrations(&mut connection, &root, version, !new_library)?;
        let check: String = connection.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
        if check != "ok" {
            return Err(CoreError::Inconsistent(format!(
                "Catalogue endommagé : {check}"
            )));
        }
        let foreign_key_error = connection.prepare("PRAGMA foreign_key_check")?.exists([])?;
        let invalid_head: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM variants v LEFT JOIN snapshots s ON s.id=v.head_snapshot_id WHERE s.id IS NULL OR s.variant_id<>v.id)", [], |row| row.get(0))?;
        if foreign_key_error || invalid_head {
            return Err(CoreError::Inconsistent("Le catalogue contient des références manquantes. Restaurez une sauvegarde de la bibliothèque.".into()));
        }
        let mut notices = Vec::new();
        for entry in fs::read_dir(root.join("staging"))? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let journal_path = entry.path().join("operation.json");
            if !journal_path.is_file() {
                continue;
            }
            match fs::read(&journal_path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<PublishJournal>(&bytes).ok())
            {
                Some(journal) => {
                    let exists: bool = connection.query_row(
                        "SELECT EXISTS(SELECT 1 FROM variants WHERE id=?1)",
                        [&journal.variant_id],
                        |row| row.get(0),
                    )?;
                    if exists {
                        fs::remove_file(journal_path)?;
                        let _ = fs::remove_dir(entry.path());
                    } else {
                        notices.push(format!("Opération interrompue : fichiers préservés dans {} (espace de travail {}).", entry.path().display(), journal.workspace));
                    }
                }
                None => notices.push(format!(
                    "Journal de récupération illisible préservé : {}",
                    journal_path.display()
                )),
            }
        }
        if new_library {
            let mut prepared_marker = tempfile::NamedTempFile::new_in(root.join("staging"))?;
            prepared_marker.write_all(FORMAT.as_bytes())?;
            prepared_marker.as_file().sync_all()?;
            prepared_marker
                .persist_noclobber(&marker)
                .map_err(|error| CoreError::Io(error.error))?;
        }
        if initializing.is_file()
            && fs::read_to_string(&initializing).is_ok_and(|value| value == FORMAT)
        {
            let _ = fs::remove_file(initializing);
        }
        Ok(Self {
            store: FileStore {
                objects: root.join("objects"),
                staging: root.join("staging"),
            },
            root,
            connection: Mutex::new(connection),
            mutation: Mutex::new(()),
            _lock: lock,
            recovery_notices: notices,
        })
    }

    fn db(&self) -> Result<MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|_| CoreError::Inconsistent("Verrou du catalogue indisponible.".into()))
    }
    fn write_guard(&self) -> Result<MutexGuard<'_, ()>> {
        self.mutation
            .lock()
            .map_err(|_| CoreError::Inconsistent("Verrou d’écriture indisponible.".into()))
    }

    pub fn info(&self) -> Result<LibraryInfo> {
        let asset_count = self
            .db()?
            .query_row("SELECT COUNT(*) FROM assets", [], |row| sql_u64(row, 0))?;
        Ok(LibraryInfo {
            root: dunce::simplified(&self.root).to_string_lossy().into_owned(),
            asset_count,
            physical_bytes: directory_bytes(&self.root)?,
            recovery_notices: self.recovery_notices.clone(),
        })
    }

    pub fn list_assets(&self, filter: AssetFilter) -> Result<AssetPage> {
        let db = self.db()?;
        let sort = match filter.sort.as_str() {
            "name" => "a.name COLLATE NOCASE ASC",
            "size" => "working_bytes DESC",
            "triangles" => "triangles DESC",
            "updated" | "" => "a.updated_at DESC",
            _ => return Err(CoreError::Invalid("Tri inconnu.".into())),
        };
        let condition = "a.archived=?1 AND (?2='' OR instr(lower(a.name),lower(?2))>0 OR EXISTS(SELECT 1 FROM asset_tags t WHERE t.asset_id=a.id AND instr(lower(t.tag),lower(?2))>0)) AND (?3 IS NULL OR a.category_id IN (WITH RECURSIVE cats(id) AS (SELECT id FROM categories WHERE id=?3 UNION SELECT c.id FROM categories c JOIN cats ON c.parent_id=cats.id) SELECT id FROM cats)) AND (?4 IS NULL OR EXISTS(SELECT 1 FROM asset_games g WHERE g.asset_id=a.id AND g.game_id=?4)) AND (?5 IS NULL OR EXISTS(SELECT 1 FROM asset_tags t WHERE t.asset_id=a.id AND t.tag=?5))";
        let total = db.query_row(
            &format!("SELECT COUNT(*) FROM assets a WHERE {condition}"),
            params![
                filter.archived,
                filter.search,
                filter.category_id,
                filter.game_id,
                filter.tag
            ],
            |row| sql_u64(row, 0),
        )?;
        let mut statement = db.prepare(&format!("SELECT a.id, (SELECT COALESCE(SUM(working_bytes),0) FROM variants WHERE asset_id=a.id) AS working_bytes, (SELECT triangles FROM variants WHERE asset_id=a.id AND triangles IS NOT NULL ORDER BY created_at LIMIT 1) AS triangles FROM assets a WHERE {condition} ORDER BY {sort},a.id LIMIT ?6 OFFSET ?7"))?;
        let ids = statement
            .query_map(
                params![
                    filter.archived,
                    filter.search,
                    filter.category_id,
                    filter.game_id,
                    filter.tag,
                    filter.limit.clamp(1, 500),
                    filter.offset
                ],
                |row| row.get::<_, String>(0),
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let items = ids
            .iter()
            .map(|id| read_asset(&db, id))
            .collect::<Result<Vec<_>>>()?;
        Ok(AssetPage { items, total })
    }

    pub fn get_asset(&self, id: &str) -> Result<AssetDetail> {
        let db = self.db()?;
        let asset = read_asset(&db, id)?;
        let mut statement =
            db.prepare("SELECT id FROM variants WHERE asset_id=?1 ORDER BY created_at,id")?;
        let ids = statement
            .query_map([id], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let variants = ids
            .iter()
            .map(|id| read_variant(&db, &self.root, id))
            .collect::<Result<Vec<_>>>()?;
        Ok(AssetDetail { asset, variants })
    }

    pub fn get_variant(&self, id: &str) -> Result<VariantDetail> {
        let db = self.db()?;
        let variant = read_variant(&db, &self.root, id)?;
        // The detail view contains the newest 100 lightweight summaries. A selected
        // version is loaded separately through get_snapshot, including its full manifest.
        let snapshots = read_snapshot_page(&db, id, 0, 100)?.items;
        drop(db);
        let files = list_files(Path::new(&variant.workspace_path))?;
        Ok(VariantDetail {
            variant,
            snapshots,
            files,
        })
    }

    pub fn get_snapshot(&self, id: &str) -> Result<Snapshot> {
        read_snapshot(&*self.db()?, id)
    }

    /// Paginated summaries, including versions inherited from the fork's origin.
    /// A selected version's complete manifest is returned by get_snapshot.
    pub fn list_snapshots(
        &self,
        variant_id: &str,
        offset: u32,
        limit: u32,
    ) -> Result<SnapshotPage> {
        read_snapshot_page(&*self.db()?, variant_id, offset, limit)
    }
    pub fn workspace_path(&self, variant_id: &str) -> Result<PathBuf> {
        Ok(PathBuf::from(
            read_variant(&*self.db()?, &self.root, variant_id)?.workspace_path,
        ))
    }

    pub fn import_folder(
        &self,
        source: &Path,
        name: &str,
        config: ExportConfig,
        ctx: &OperationContext,
    ) -> Result<AssetDetail> {
        let _guard = self.write_guard()?;
        let name = checked_name(name)?;
        ensure_real_directory(source)?;
        let source = fs::canonicalize(source)?;
        if source.starts_with(&self.root) || self.root.starts_with(&source) {
            return Err(CoreError::Invalid(
                "Le dossier importé doit être extérieur à la bibliothèque et ne pas la contenir."
                    .into(),
            ));
        }
        // Reject an invalid primary-file choice before reading and storing large inputs.
        let config = resolve_config(config, &list_files(&source)?)?;
        let entries = self.store.capture(&source, true, ctx)?;
        validate_config(&config, &entries)?;
        self.publish_asset(&name, config, entries, ctx)
    }

    pub fn create_empty(
        &self,
        name: &str,
        config: ExportConfig,
        ctx: &OperationContext,
    ) -> Result<AssetDetail> {
        let _guard = self.write_guard()?;
        let name = checked_name(name)?;
        let entries = ["models", "textures", "references", "reports", "screenshots"]
            .iter()
            .map(|name| ManifestEntry {
                path: (*name).into(),
                kind: "directory".into(),
                hash: None,
                size: 0,
            })
            .collect::<Vec<_>>();
        validate_config(&config, &entries)?;
        self.publish_asset(&name, config, entries, ctx)
    }

    /// A staged, canonical project prepared by the synchronization service.
    pub(crate) fn import_prepared(
        &self,
        source: &Path,
        name: &str,
        config: ExportConfig,
        ctx: &OperationContext,
    ) -> Result<AssetDetail> {
        let _guard = self.write_guard()?;
        let name = checked_name(name)?;
        let entries = self.store.capture(source, true, ctx)?;
        let layout = crate::sync::validate_project(&entries)?;
        if !entries
            .iter()
            .any(|entry| entry.kind == "file" && entry.path == layout.glb_file)
        {
            return Err(CoreError::Inconsistent(
                "Le GLB du projet préparé est absent.".into(),
            ));
        }
        validate_config(&config, &entries)?;
        self.publish_asset(&name, config, entries, ctx)
    }

    fn publish_asset(
        &self,
        name: &str,
        config: ExportConfig,
        entries: Vec<ManifestEntry>,
        ctx: &OperationContext,
    ) -> Result<AssetDetail> {
        ctx.check_cancelled()?;
        let asset_id = new_id();
        let variant_id = new_id();
        let snapshot_id = new_id();
        let date = now();
        let workspace = format!("workspaces/{asset_id}/{variant_id}");
        let journal = self.prepare_workspace(&variant_id, &workspace, &entries, ctx)?;
        let snapshot = Snapshot {
            id: snapshot_id.clone(),
            variant_id: variant_id.clone(),
            parent_id: None,
            created_at: date.clone(),
            message: "Version initiale".into(),
            logical_bytes: entries.iter().map(|entry| entry.size).sum(),
            config: config.clone(),
            entries,
        };
        let result = (|| -> Result<()> {
            ctx.check_cancelled()?;
            let mut db = self.db()?;
            let transaction = db.transaction()?;
            transaction.execute(
                "INSERT INTO assets(id,name,created_at,updated_at) VALUES(?1,?2,?3,?3)",
                params![asset_id, name, date],
            )?;
            transaction.execute("INSERT INTO variants(id,asset_id,name,workspace,head_snapshot_id,config,working_bytes,created_at) VALUES(?1,?2,'Principale',?3,?4,?5,?6,?7)", params![variant_id,asset_id,workspace,snapshot_id,serde_json::to_string(&config)?,sql_size(snapshot.logical_bytes)?,date])?;
            insert_snapshot(&transaction, &snapshot)?;
            transaction.commit()?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                cleanup_journal(&journal);
                self.get_asset(&asset_id)
            }
            Err(error) => Err(error), // Journal + published workspace are retained for recovery.
        }
    }

    fn prepare_workspace(
        &self,
        variant_id: &str,
        workspace: &str,
        entries: &[ManifestEntry],
        ctx: &OperationContext,
    ) -> Result<PathBuf> {
        let journal_dir = self.root.join("staging").join(new_id());
        fs::create_dir(&journal_dir)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(journal_dir.join("operation.json"))?;
        file.write_all(&serde_json::to_vec(&PublishJournal {
            variant_id: variant_id.into(),
            workspace: workspace.into(),
        })?)?;
        file.sync_all()?;
        let destination = self.root.join(safe_relative(workspace)?);
        self.store.materialize(entries, &destination, ctx)?;
        Ok(journal_dir)
    }

    pub fn create_snapshot(
        &self,
        variant_id: &str,
        message: &str,
        ctx: &OperationContext,
    ) -> Result<Snapshot> {
        self.create_snapshot_inner(variant_id, message, None, ctx)
    }

    pub fn create_snapshot_checked(
        &self,
        variant_id: &str,
        message: &str,
        receipt: &crate::sync::SyncReceipt,
        ctx: &OperationContext,
    ) -> Result<Snapshot> {
        self.create_snapshot_inner(variant_id, message, Some(receipt), ctx)
    }

    fn create_snapshot_inner(
        &self,
        variant_id: &str,
        message: &str,
        receipt: Option<&crate::sync::SyncReceipt>,
        ctx: &OperationContext,
    ) -> Result<Snapshot> {
        let _guard = self.write_guard()?;
        let message = checked_name(message)?;
        let mut variant = read_variant(&*self.db()?, &self.root, variant_id)?;
        let previous = self.get_snapshot(&variant.head_snapshot_id)?;
        let entries = self
            .store
            .capture(Path::new(&variant.workspace_path), true, ctx)?;
        variant.config = resolve_config(variant.config, &entries)?;
        if let Some(receipt) = receipt {
            receipt.validate_capture(
                dunce::simplified(Path::new(&variant.workspace_path)),
                &entries,
                ctx,
            )?;
            variant.config = crate::sync::validate_project(&entries)?.config();
        }
        if !crate::sync::source_changed(&previous.entries, &entries) {
            return Err(CoreError::Invalid(
                "Aucune modification à versionner.".into(),
            ));
        }
        let snapshot = Snapshot {
            id: new_id(),
            variant_id: variant.id.clone(),
            parent_id: Some(previous.id),
            created_at: now(),
            message,
            logical_bytes: entries.iter().map(|entry| entry.size).sum(),
            config: variant.config,
            entries,
        };
        ctx.check_cancelled()?;
        let mut db = self.db()?;
        let transaction = db.transaction()?;
        insert_snapshot(&transaction, &snapshot)?;
        transaction.execute(
            "UPDATE variants SET head_snapshot_id=?1,working_bytes=?2,triangles=NULL,config=?4 WHERE id=?3",
            params![snapshot.id, sql_size(snapshot.logical_bytes)?, variant.id,serde_json::to_string(&snapshot.config)?],
        )?;
        transaction.execute(
            "UPDATE assets SET updated_at=?1 WHERE id=?2",
            params![snapshot.created_at, variant.asset_id],
        )?;
        transaction.commit()?;
        Ok(snapshot)
    }

    pub fn create_variant(
        &self,
        snapshot_id: &str,
        name: &str,
        ctx: &OperationContext,
    ) -> Result<Variant> {
        let _guard = self.write_guard()?;
        let name = checked_name(name)?;
        let base = self.get_snapshot(snapshot_id)?;
        self.publish_variant(base.clone(), &name, base.entries, base.config, ctx)
    }

    pub(crate) fn create_variant_prepared(
        &self,
        snapshot_id: &str,
        name: &str,
        source: &Path,
        config: ExportConfig,
        ctx: &OperationContext,
    ) -> Result<Variant> {
        let _guard = self.write_guard()?;
        let name = checked_name(name)?;
        let base = self.get_snapshot(snapshot_id)?;
        let entries = self.store.capture(source, true, ctx)?;
        crate::sync::validate_project(&entries)?;
        validate_config(&config, &entries)?;
        self.publish_variant(base, &name, entries, config, ctx)
    }

    fn publish_variant(
        &self,
        base: Snapshot,
        name: &str,
        entries: Vec<ManifestEntry>,
        config: ExportConfig,
        ctx: &OperationContext,
    ) -> Result<Variant> {
        let source = read_variant(&*self.db()?, &self.root, &base.variant_id)?;
        let variant_id = new_id();
        let workspace = format!("workspaces/{}/{variant_id}", source.asset_id);
        {
            let db = self.db()?;
            let duplicate: bool = db.query_row(
                "SELECT EXISTS(SELECT 1 FROM variants WHERE asset_id=?1 AND name=?2)",
                params![source.asset_id, name],
                |row| row.get(0),
            )?;
            if duplicate {
                return Err(CoreError::Invalid("Une variante porte déjà ce nom.".into()));
            }
        }
        let journal = self.prepare_workspace(&variant_id, &workspace, &entries, ctx)?;
        let snapshot = Snapshot {
            id: new_id(),
            variant_id: variant_id.clone(),
            parent_id: Some(base.id),
            created_at: now(),
            message: format!("Création de la variante {name}"),
            config,
            logical_bytes: entries.iter().map(|entry| entry.size).sum(),
            entries,
        };
        ctx.check_cancelled()?;
        let mut db = self.db()?;
        let transaction = db.transaction()?;
        transaction.execute("INSERT INTO variants(id,asset_id,name,workspace,head_snapshot_id,config,working_bytes,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)", params![variant_id,source.asset_id,name,workspace,snapshot.id,serde_json::to_string(&snapshot.config)?,sql_size(snapshot.logical_bytes)?,snapshot.created_at])?;
        insert_snapshot(&transaction, &snapshot)?;
        transaction.execute(
            "UPDATE assets SET updated_at=?1 WHERE id=?2",
            params![snapshot.created_at, source.asset_id],
        )?;
        transaction.commit()?;
        cleanup_journal(&journal);
        read_variant(&db, &self.root, &variant_id)
    }

    pub fn changes(&self, variant_id: &str, ctx: &OperationContext) -> Result<ChangeSet> {
        self.changes_inner(variant_id, None, ctx)
    }

    pub(crate) fn changes_checked(
        &self,
        variant_id: &str,
        receipt: &crate::sync::SyncReceipt,
        ctx: &OperationContext,
    ) -> Result<ChangeSet> {
        self.changes_inner(variant_id, Some(receipt), ctx)
    }

    fn changes_inner(
        &self,
        variant_id: &str,
        receipt: Option<&crate::sync::SyncReceipt>,
        ctx: &OperationContext,
    ) -> Result<ChangeSet> {
        let _guard = self.write_guard()?;
        let variant = read_variant(&*self.db()?, &self.root, variant_id)?;
        let previous = self.get_snapshot(&variant.head_snapshot_id)?;
        let current = self
            .store
            .capture(Path::new(&variant.workspace_path), false, ctx)?;
        if let Some(receipt) = receipt {
            receipt.validate_capture(
                dunce::simplified(Path::new(&variant.workspace_path)),
                &current,
                ctx,
            )?;
        }
        let old = previous
            .entries
            .iter()
            .filter(|entry| entry.kind != "file" || !crate::sync::is_glb(&entry.path))
            .map(|entry| (&entry.path, entry))
            .collect::<BTreeMap<_, _>>();
        let new = current
            .iter()
            .filter(|entry| entry.kind != "file" || !crate::sync::is_glb(&entry.path))
            .map(|entry| (&entry.path, entry))
            .collect::<BTreeMap<_, _>>();
        let mut result = ChangeSet {
            added: Vec::new(),
            modified: Vec::new(),
            deleted: Vec::new(),
            config_changed: false,
            working_bytes: current.iter().map(|entry| entry.size).sum(),
        };
        for (path, entry) in &new {
            match old.get(path) {
                None => result.added.push((*path).clone()),
                Some(old) if old != entry => result.modified.push((*path).clone()),
                _ => {}
            }
        }
        for path in old.keys() {
            if !new.contains_key(path) {
                result.deleted.push((*path).clone());
            }
        }
        self.db()?.execute(
            "UPDATE variants SET working_bytes=?1 WHERE id=?2",
            params![sql_size(result.working_bytes)?, variant_id],
        )?;
        Ok(result)
    }

    pub(crate) fn workspace_manifest(
        &self,
        variant_id: &str,
        ctx: &OperationContext,
    ) -> Result<Vec<ManifestEntry>> {
        let _guard = self.write_guard()?;
        self.store
            .capture(&self.workspace_path(variant_id)?, false, ctx)
    }

    pub(crate) fn record_synchronized(
        &self,
        variant_id: &str,
        config: &ExportConfig,
        triangles: u64,
        artifact_changed: bool,
    ) -> Result<()> {
        let _guard = self.write_guard()?;
        let mut db = self.db()?;
        let transaction = db.transaction()?;
        let variant = read_variant(&transaction, &self.root, variant_id)?;
        transaction.execute(
            "UPDATE variants SET config=?1,triangles=?2 WHERE id=?3",
            params![
                serde_json::to_string(config)?,
                sql_size(triangles)?,
                variant_id
            ],
        )?;
        if artifact_changed {
            transaction.execute(
                "UPDATE assets SET updated_at=?1 WHERE id=?2",
                params![now(), variant.asset_id],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn update_config(&self, variant_id: &str, config: ExportConfig) -> Result<()> {
        let _guard = self.write_guard()?;
        let variant = read_variant(&*self.db()?, &self.root, variant_id)?;
        let config = resolve_config(config, &list_files(Path::new(&variant.workspace_path))?)?;
        if config == variant.config {
            return Ok(());
        }
        let mut db = self.db()?;
        let transaction = db.transaction()?;
        transaction.execute(
            "UPDATE variants SET config=?1,triangles=NULL WHERE id=?2",
            params![serde_json::to_string(&config)?, variant_id],
        )?;
        transaction.execute(
            "UPDATE assets SET updated_at=?1 WHERE id=?2",
            params![now(), variant.asset_id],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn materialize(
        &self,
        snapshot_id: &str,
        destination: &Path,
        ctx: &OperationContext,
    ) -> Result<()> {
        let snapshot = self.get_snapshot(snapshot_id)?;
        self.validate_destination(destination)?;
        self.store.materialize(&snapshot.entries, destination, ctx)
    }

    pub fn capture_working(
        &self,
        variant_id: &str,
        destination: &Path,
        ctx: &OperationContext,
    ) -> Result<Snapshot> {
        self.capture_working_inner(variant_id, destination, ctx, true)
    }

    pub(crate) fn capture_preview(
        &self,
        variant_id: &str,
        destination: &Path,
        ctx: &OperationContext,
    ) -> Result<Snapshot> {
        self.capture_working_inner(variant_id, destination, ctx, false)
    }

    fn capture_working_inner(
        &self,
        variant_id: &str,
        destination: &Path,
        ctx: &OperationContext,
        update_config: bool,
    ) -> Result<Snapshot> {
        let _guard = self.write_guard()?;
        self.validate_destination(destination)?;
        let variant = read_variant(&*self.db()?, &self.root, variant_id)?;
        let temp = tempfile::tempdir_in(&self.store.staging)?;
        let objects = temp.path().join("objects");
        fs::create_dir(&objects)?;
        let ephemeral = FileStore {
            objects,
            staging: temp.path().to_path_buf(),
        };
        let entries = ephemeral.capture(Path::new(&variant.workspace_path), true, ctx)?;
        let config = if update_config {
            resolve_config(variant.config.clone(), &entries)?
        } else {
            variant.config.clone()
        };
        ephemeral.materialize(&entries, destination, ctx)?;
        if update_config && config != variant.config {
            self.db()?.execute(
                "UPDATE variants SET config=?1,triangles=NULL WHERE id=?2",
                params![serde_json::to_string(&config)?, variant_id],
            )?;
        }
        Ok(Snapshot {
            id: new_id(),
            variant_id: variant.id,
            parent_id: Some(variant.head_snapshot_id),
            created_at: now(),
            message: "Copie de travail temporaire".into(),
            logical_bytes: entries.iter().map(|entry| entry.size).sum(),
            config,
            entries,
        })
    }

    fn validate_destination(&self, destination: &Path) -> Result<()> {
        if !destination.is_absolute() {
            return Err(CoreError::Invalid(
                "La destination doit être un chemin absolu.".into(),
            ));
        }
        let mut ancestor = destination;
        while !ancestor.exists() {
            ancestor = ancestor
                .parent()
                .ok_or_else(|| CoreError::Invalid("Destination invalide.".into()))?;
        }
        let canonical = fs::canonicalize(ancestor)?;
        if canonical.starts_with(self.root.join("objects"))
            || canonical.starts_with(self.root.join("workspaces"))
            || canonical == self.root
        {
            return Err(CoreError::Invalid("La destination ne peut pas remplacer le stockage interne ou un dossier de travail.".into()));
        }
        Ok(())
    }

    pub fn set_geometry(&self, variant_id: &str, triangles: u64) -> Result<()> {
        let triangles = i64::try_from(triangles)
            .map_err(|_| CoreError::Invalid("Nombre de triangles trop grand.".into()))?;
        let _guard = self.write_guard()?;
        let count = self.db()?.execute(
            "UPDATE variants SET triangles=?1 WHERE id=?2",
            params![triangles, variant_id],
        )?;
        if count == 0 {
            return Err(CoreError::NotFound(variant_id.into()));
        }
        Ok(())
    }

    pub fn taxonomy(&self) -> Result<Taxonomy> {
        let db = self.db()?;
        let categories = db
            .prepare("SELECT id,name,parent_id FROM categories ORDER BY name COLLATE NOCASE")?
            .query_map([], |row| {
                Ok(Category {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    parent_id: row.get(2)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let games = db
            .prepare("SELECT id,name FROM games ORDER BY name COLLATE NOCASE")?
            .query_map([], |row| {
                Ok(Game {
                    id: row.get(0)?,
                    name: row.get(1)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let tags = db
            .prepare("SELECT name FROM tags ORDER BY name COLLATE NOCASE")?
            .query_map([], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(Taxonomy {
            categories,
            games,
            tags,
        })
    }

    pub fn save_category(
        &self,
        id: Option<&str>,
        name: &str,
        parent_id: Option<&str>,
    ) -> Result<Category> {
        let _guard = self.write_guard()?;
        let name = checked_name(name)?;
        let key = id.map(str::to_owned).unwrap_or_else(new_id);
        let db = self.db()?;
        if let Some(existing) = id {
            require_id(&db, "categories", existing)?;
        }
        if let Some(parent) = parent_id {
            require_id(&db, "categories", parent)?;
            let cycle: bool = db.query_row("WITH RECURSIVE parents(id,parent_id) AS (SELECT id,parent_id FROM categories WHERE id=?1 UNION SELECT c.id,c.parent_id FROM categories c JOIN parents ON c.id=parents.parent_id) SELECT EXISTS(SELECT 1 FROM parents WHERE id=?2)", params![parent,key], |row| row.get(0))?;
            if cycle || parent == key {
                return Err(CoreError::Invalid(
                    "Une catégorie ne peut pas devenir son propre parent.".into(),
                ));
            }
        }
        let duplicate: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM categories WHERE lower(name)=lower(?1) AND parent_id IS ?2 AND id<>?3)", params![name,parent_id,key], |row| row.get(0))?;
        if duplicate {
            return Err(CoreError::Invalid(
                "Cette catégorie existe déjà à cet emplacement.".into(),
            ));
        }
        db.execute("INSERT INTO categories(id,name,parent_id) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET name=excluded.name,parent_id=excluded.parent_id", params![key,name,parent_id])?;
        Ok(Category {
            id: key,
            name,
            parent_id: parent_id.map(str::to_owned),
        })
    }

    pub fn save_game(&self, id: Option<&str>, name: &str) -> Result<Game> {
        let _guard = self.write_guard()?;
        let name = checked_name(name)?;
        let key = id.map(str::to_owned).unwrap_or_else(new_id);
        let db = self.db()?;
        if let Some(existing) = id {
            require_id(&db, "games", existing)?;
        }
        db.execute("INSERT INTO games(id,name) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET name=excluded.name", params![key,name])?;
        Ok(Game { id: key, name })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_asset(
        &self,
        id: &str,
        name: &str,
        category_id: Option<&str>,
        tags: Vec<String>,
        game_ids: Vec<String>,
        archived: bool,
    ) -> Result<()> {
        let _guard = self.write_guard()?;
        let name = checked_name(name)?;
        let tags = tags
            .iter()
            .map(|tag| checked_name(tag))
            .collect::<Result<BTreeSet<_>>>()?;
        let games = game_ids.into_iter().collect::<BTreeSet<_>>();
        let mut db = self.db()?;
        let transaction = db.transaction()?;
        require_id(&transaction, "assets", id)?;
        if let Some(category) = category_id {
            require_id(&transaction, "categories", category)?;
        }
        for game in &games {
            require_id(&transaction, "games", game)?;
        }
        transaction.execute(
            "UPDATE assets SET name=?1,category_id=?2,archived=?3,updated_at=?4 WHERE id=?5",
            params![name, category_id, archived, now(), id],
        )?;
        transaction.execute("DELETE FROM asset_tags WHERE asset_id=?1", [id])?;
        for tag in tags {
            transaction.execute("INSERT OR IGNORE INTO tags(name) VALUES(?1)", [&tag])?;
            transaction.execute(
                "INSERT INTO asset_tags(asset_id,tag) VALUES(?1,?2)",
                params![id, tag],
            )?;
        }
        transaction.execute("DELETE FROM asset_games WHERE asset_id=?1", [id])?;
        for game in games {
            transaction.execute(
                "INSERT INTO asset_games(asset_id,game_id) VALUES(?1,?2)",
                params![id, game],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }
}

fn require_id(db: &Connection, table: &str, id: &str) -> Result<()> {
    let exists: bool = db.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE id=?1)"),
        [id],
        |row| row.get(0),
    )?;
    if exists {
        Ok(())
    } else {
        Err(CoreError::NotFound(id.into()))
    }
}

fn apply_migrations(
    connection: &mut Connection,
    root: &Path,
    version: i64,
    existing_catalog: bool,
) -> Result<()> {
    if version == SCHEMA_VERSION {
        return Ok(());
    }
    if existing_catalog {
        let directory = root.join("backups");
        if !directory.exists() {
            fs::create_dir(&directory)?;
        }
        ensure_real_directory(&directory)?;
        let path = directory.join(format!("catalog-schema-{version}-{}.sqlite3", new_id()));
        let mut backup_connection = Connection::open(&path)?;
        {
            let backup = rusqlite::backup::Backup::new(connection, &mut backup_connection)?;
            backup.run_to_completion(128, std::time::Duration::from_millis(10), None)?;
        }
        drop(backup_connection);
        // FlushFileBuffers on Windows requires a handle opened for writing.
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)?
            .sync_all()?;
    }
    // The initial migration is deliberately additive. Future versions must append
    // explicit migrations here and retain the same pre-migration backup contract.
    let transaction = connection.transaction()?;
    if version == 0 {
        transaction.execute_batch(SCHEMA)?;
    }
    transaction.commit()?;
    Ok(())
}

fn cleanup_journal(path: &Path) {
    // Publication is already committed. A cleanup failure must not turn success into
    // a reported failure (which could cause the caller to import a duplicate).
    if fs::remove_file(path.join("operation.json")).is_ok() {
        let _ = fs::remove_dir(path);
    }
}

fn validate_config(config: &ExportConfig, entries: &[ManifestEntry]) -> Result<()> {
    if let Some(primary) = &config.primary_file {
        safe_relative(primary)?;
        if !entries
            .iter()
            .any(|entry| entry.path == *primary && entry.kind == "file")
        {
            return Err(CoreError::Invalid(
                "Le fichier principal n’existe pas dans ce dossier.".into(),
            ));
        }
        let extension = Path::new(primary)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("");
        if !extension.eq_ignore_ascii_case("blend") && !extension.eq_ignore_ascii_case("glb") {
            return Err(CoreError::Invalid(
                "Le fichier principal doit être un .blend ou .glb.".into(),
            ));
        }
    }
    for value in [&config.scene, &config.collection].into_iter().flatten() {
        checked_name(value)?;
    }
    Ok(())
}

fn resolve_config(mut config: ExportConfig, entries: &[ManifestEntry]) -> Result<ExportConfig> {
    if config.primary_file.is_none() {
        let candidates = |extension: &str| {
            entries
                .iter()
                .filter(|entry| {
                    entry.kind == "file"
                        && Path::new(&entry.path)
                            .extension()
                            .is_some_and(|value| value.eq_ignore_ascii_case(extension))
                })
                .collect::<Vec<_>>()
        };
        let blends = candidates("blend");
        if blends.len() == 1 {
            config.primary_file = Some(blends[0].path.clone());
        } else if blends.is_empty() {
            let glbs = candidates("glb");
            if glbs.len() == 1 {
                config.primary_file = Some(glbs[0].path.clone());
            }
        }
    }
    validate_config(&config, entries)?;
    Ok(config)
}

fn read_variant(db: &Connection, root: &Path, id: &str) -> Result<Variant> {
    let row = db
        .query_row(
            "SELECT id,asset_id,name,workspace,head_snapshot_id,config,working_bytes,triangles FROM variants WHERE id=?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    sql_u64(row,6)?,
                    sql_optional_u64(row,7)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| CoreError::NotFound(id.into()))?;
    let relative = safe_relative(&row.3)?;
    if !relative.starts_with("workspaces") {
        return Err(CoreError::Inconsistent(
            "Chemin d’espace de travail hors du stockage prévu.".into(),
        ));
    }
    let mut workspace = root.to_path_buf();
    for component in relative.components() {
        workspace.push(component);
        ensure_real_directory(&workspace)?;
    }
    let workspace_path = dunce::simplified(&workspace).to_string_lossy().into_owned();
    Ok(Variant {
        id: row.0,
        asset_id: row.1,
        name: row.2,
        workspace_path,
        head_snapshot_id: row.4,
        config: serde_json::from_str(&row.5)?,
        working_bytes: row.6,
        triangles: row.7,
    })
}

fn read_snapshot_summary(db: &Connection, id: &str) -> Result<Snapshot> {
    let mut snapshot = db.query_row("SELECT id,variant_id,parent_id,created_at,message,logical_bytes,config FROM snapshots WHERE id=?1", [id], |row| Ok((Snapshot { id:row.get(0)?,variant_id:row.get(1)?,parent_id:row.get(2)?,created_at:row.get(3)?,message:row.get(4)?,logical_bytes:sql_u64(row,5)?,config:ExportConfig::default(),entries:Vec::new() },row.get::<_,String>(6)?))).optional()?.ok_or_else(|| CoreError::NotFound(id.into()))?;
    snapshot.0.config = serde_json::from_str(&snapshot.1)?;
    Ok(snapshot.0)
}

fn read_snapshot(db: &Connection, id: &str) -> Result<Snapshot> {
    let mut snapshot = read_snapshot_summary(db, id)?;
    snapshot.entries = db
        .prepare(
            "SELECT path,kind,hash,size FROM snapshot_entries WHERE snapshot_id=?1 ORDER BY path",
        )?
        .query_map([id], |row| {
            Ok(ManifestEntry {
                path: row.get(0)?,
                kind: row.get(1)?,
                hash: row.get(2)?,
                size: sql_u64(row, 3)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(snapshot)
}

fn read_snapshot_page(
    db: &Connection,
    variant_id: &str,
    offset: u32,
    limit: u32,
) -> Result<SnapshotPage> {
    require_id(db, "variants", variant_id)?;
    // UNION (rather than UNION ALL) also terminates if a damaged catalog contains
    // a parent cycle. Snapshot manifests are not loaded for a history list.
    let chain = "WITH RECURSIVE history(id,parent_id) AS (SELECT s.id,s.parent_id FROM snapshots s JOIN variants v ON v.head_snapshot_id=s.id WHERE v.id=?1 UNION SELECT s.id,s.parent_id FROM snapshots s JOIN history h ON s.id=h.parent_id)";
    let total = db.query_row(
        &format!("{chain} SELECT COUNT(*) FROM history"),
        [variant_id],
        |row| sql_u64(row, 0),
    )?;
    let mut statement = db.prepare(&format!("{chain} SELECT s.id FROM history h JOIN snapshots s ON s.id=h.id ORDER BY s.created_at DESC,s.id DESC LIMIT ?2 OFFSET ?3"))?;
    let ids = statement
        .query_map(params![variant_id, limit.clamp(1, 500), offset], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let items = ids
        .iter()
        .map(|id| read_snapshot_summary(db, id))
        .collect::<Result<Vec<_>>>()?;
    Ok(SnapshotPage { items, total })
}

fn insert_snapshot(transaction: &Transaction<'_>, snapshot: &Snapshot) -> Result<()> {
    transaction.execute("INSERT INTO snapshots(id,variant_id,parent_id,created_at,message,logical_bytes,config) VALUES(?1,?2,?3,?4,?5,?6,?7)", params![snapshot.id,snapshot.variant_id,snapshot.parent_id,snapshot.created_at,snapshot.message,sql_size(snapshot.logical_bytes)?,serde_json::to_string(&snapshot.config)?])?;
    let mut statement = transaction.prepare(
        "INSERT INTO snapshot_entries(snapshot_id,path,kind,hash,size) VALUES(?1,?2,?3,?4,?5)",
    )?;
    for entry in &snapshot.entries {
        statement.execute(params![
            snapshot.id,
            entry.path,
            entry.kind,
            entry.hash,
            sql_size(entry.size)?
        ])?;
    }
    Ok(())
}

fn read_asset(db: &Connection, id: &str) -> Result<Asset> {
    let mut asset = db
        .query_row(
            "SELECT id,name,category_id,archived,created_at,updated_at FROM assets WHERE id=?1",
            [id],
            |row| {
                Ok(Asset {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    category_id: row.get(2)?,
                    archived: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                    tags: Vec::new(),
                    game_ids: Vec::new(),
                    working_bytes: 0,
                    history_bytes: 0,
                    triangles: None,
                    variant_count: 0,
                })
            },
        )
        .optional()?
        .ok_or_else(|| CoreError::NotFound(id.into()))?;
    asset.tags = db
        .prepare("SELECT tag FROM asset_tags WHERE asset_id=?1 ORDER BY tag")?
        .query_map([id], |row| row.get(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    asset.game_ids = db
        .prepare("SELECT game_id FROM asset_games WHERE asset_id=?1 ORDER BY game_id")?
        .query_map([id], |row| row.get(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    (asset.working_bytes, asset.variant_count) = db.query_row(
        "SELECT COALESCE(SUM(working_bytes),0),COUNT(*) FROM variants WHERE asset_id=?1",
        [id],
        |row| Ok((sql_u64(row, 0)?, sql_u64(row, 1)?)),
    )?;
    asset.triangles = db.query_row("SELECT triangles FROM variants WHERE asset_id=?1 AND triangles IS NOT NULL ORDER BY created_at LIMIT 1", [id], |row| sql_u64(row,0)).optional()?;
    asset.history_bytes = db.query_row("SELECT COALESCE(SUM(size),0) FROM (SELECT e.hash,MAX(e.size) AS size FROM snapshot_entries e JOIN snapshots s ON s.id=e.snapshot_id JOIN variants v ON v.id=s.variant_id WHERE v.asset_id=?1 AND e.kind='file' GROUP BY e.hash)", [id], |row| sql_u64(row,0))?;
    Ok(asset)
}

fn sql_size(value: u64) -> Result<i64> {
    i64::try_from(value)
        .map_err(|_| CoreError::Invalid("Taille supérieure à la capacité du catalogue.".into()))
}
fn sql_u64(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(column)?;
    u64::try_from(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })
}

fn sql_optional_u64(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<Option<u64>> {
    let value: Option<i64> = row.get(column)?;
    value
        .map(|value| {
            u64::try_from(value).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    column,
                    rusqlite::types::Type::Integer,
                    Box::new(error),
                )
            })
        })
        .transpose()
}
