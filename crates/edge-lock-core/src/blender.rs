//! Blender integration runs a bundled worker in a separate process and private configuration.
pub use crate::glb::GeometryStats;
use crate::{CoreError, ExportConfig, OperationContext, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const WORKER: &str = include_str!("../../../blender/edge_lock.py");
const LOG_LIMIT: usize = 64 * 1024;
const RESPONSE_LIMIT: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlenderVersion {
    pub version: String,
    pub executable: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dependency {
    pub path: String,
    pub status: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub render_relevant: bool,
    /// Logical source location (possibly a UDIM/sequence pattern), never a temporary copy.
    #[serde(default)]
    pub source_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Inspection {
    pub geometry: GeometryStats,
    pub scenes: Vec<String>,
    pub collections: Vec<String>,
    pub dependencies: Vec<Dependency>,
    pub warnings: Vec<String>,
    pub blender_version: String,
}

#[derive(Debug, Clone)]
pub struct BlenderRunner {
    executable: PathBuf,
    timeout: Duration,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Request<'a> {
    protocol_version: u8,
    operation: &'a str,
    #[serde(serialize_with = "serialize_blender_path")]
    workspace: &'a Path,
    #[serde(serialize_with = "serialize_blender_path")]
    blend_file: &'a Path,
    config: &'a ExportConfig,
    #[serde(serialize_with = "serialize_optional_blender_path")]
    destination: Option<&'a Path>,
    #[serde(serialize_with = "serialize_optional_blender_path")]
    logical_workspace: Option<&'a Path>,
}

fn blender_path(path: &Path) -> std::result::Result<String, &'static str> {
    let path = path
        .to_str()
        .ok_or("Le chemin Blender n'est pas Unicode valide.")?;
    // fs::canonicalize uses Windows verbatim paths. Blender's glTF exporter appends
    // a forward slash, which is invalid in that namespace; pass the ordinary path.
    #[cfg(windows)]
    {
        if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
            return Ok(format!(r"\\{rest}"));
        }
        if let Some(rest) = path.strip_prefix(r"\\?\") {
            return Ok(rest.to_owned());
        }
    }
    Ok(path.to_owned())
}

fn serialize_blender_path<S: serde::Serializer>(
    path: &Path,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    use serde::ser::Error;
    serializer.serialize_str(&blender_path(path).map_err(S::Error::custom)?)
}

fn serialize_optional_blender_path<S: serde::Serializer>(
    path: &Option<&Path>,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    use serde::ser::Error;
    path.map(blender_path)
        .transpose()
        .map_err(S::Error::custom)?
        .serialize(serializer)
}

fn absolute_logical_path(path: &Path) -> Result<&Path> {
    if !path.is_absolute() {
        return Err(CoreError::Invalid(
            "Le dossier logique de l'asset doit être absolu.".into(),
        ));
    }
    Ok(path)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Response {
    protocol_version: u8,
    result: Option<Inspection>,
    error: Option<String>,
}

fn hidden_command(executable: &Path) -> Command {
    let mut command = Command::new(executable);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW for background workers only.
    }
    command
}

fn capture(mut reader: impl Read + Send + 'static) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut tail = Vec::new();
        let mut chunk = [0; 4096];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(length) => {
                    let discard = tail.len().saturating_add(length).saturating_sub(LOG_LIMIT);
                    if discard > 0 {
                        tail.drain(..discard);
                    }
                    tail.extend_from_slice(&chunk[..length]);
                }
            }
        }
        tail
    })
}

struct RunningChild(Child);
impl Drop for RunningChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn run_process(
    mut command: Command,
    timeout: Duration,
    context: &OperationContext,
) -> Result<(bool, String)> {
    context.check_cancelled()?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = command
        .spawn()
        .map_err(|error| CoreError::External(format!("Impossible de lancer Blender : {error}")))?;
    let mut child = RunningChild(child);
    let stdout = capture(
        child
            .0
            .stdout
            .take()
            .ok_or_else(|| CoreError::External("Sortie Blender indisponible".into()))?,
    );
    let stderr = capture(
        child
            .0
            .stderr
            .take()
            .ok_or_else(|| CoreError::External("Erreurs Blender indisponibles".into()))?,
    );
    let start = Instant::now();
    let outcome = loop {
        if let Err(error) = context.check_cancelled() {
            let _ = child.0.kill();
            let _ = child.0.wait();
            break Err(error);
        }
        if start.elapsed() >= timeout {
            let _ = child.0.kill();
            let _ = child.0.wait();
            break Err(CoreError::External(
                "Blender a dépassé le délai autorisé ; le processus a été arrêté.".into(),
            ));
        }
        match child.0.try_wait() {
            Ok(Some(status)) => break Ok(status.success()),
            Ok(None) => thread::sleep(Duration::from_millis(40)),
            Err(error) => {
                let _ = child.0.kill();
                let _ = child.0.wait();
                break Err(CoreError::Io(error));
            }
        }
    };
    let mut log = stdout.join().unwrap_or_default();
    log.extend(stderr.join().unwrap_or_default());
    let log = String::from_utf8_lossy(&log).into_owned();
    Ok((outcome?, log))
}

impl BlenderRunner {
    pub fn new(executable: PathBuf) -> Self {
        Self {
            executable,
            timeout: Duration::from_secs(600),
        }
    }
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    pub fn probe(&self) -> Result<BlenderVersion> {
        let mut command = hidden_command(&self.executable);
        command.arg("--version");
        let (success, output) = run_process(
            command,
            self.timeout.min(Duration::from_secs(15)),
            &OperationContext::default(),
        )?;
        let version = output
            .lines()
            .find_map(|line| line.strip_prefix("Blender "))
            .map(str::trim)
            .filter(|version| version.chars().next().is_some_and(|c| c.is_ascii_digit()));
        match (success, version) {
            (true, Some(version)) => Ok(BlenderVersion {
                version: version.to_owned(),
                executable: self.executable.clone(),
            }),
            _ => Err(CoreError::External(
                "Cet exécutable ne renvoie pas une version de Blender reconnue.".into(),
            )),
        }
    }

    pub fn create_project(&self, destination: &Path, context: &OperationContext) -> Result<()> {
        context.check_cancelled()?;
        fs::create_dir_all(destination)?;
        let workspace = fs::canonicalize(destination)?;
        let blend_file = workspace.join("main.blend");
        if blend_file.exists() {
            return Err(CoreError::Invalid(
                "Le projet contient déjà main.blend.".into(),
            ));
        }
        let config = ExportConfig::default();
        let request = Request {
            protocol_version: 1,
            operation: "create",
            workspace: &workspace,
            blend_file: &blend_file,
            config: &config,
            destination: None,
            logical_workspace: None,
        };
        self.run(&request, context)?;
        if !blend_file.is_file() {
            return Err(CoreError::External(
                "Blender n'a pas créé main.blend.".into(),
            ));
        }
        Ok(())
    }

    pub fn inspect(
        &self,
        workspace: &Path,
        config: &ExportConfig,
        context: &OperationContext,
    ) -> Result<Inspection> {
        context.check_cancelled()?;
        let workspace = fs::canonicalize(workspace)?;
        let blend_file = primary_blend(&workspace, config)?;
        let request = Request {
            protocol_version: 1,
            operation: "inspect",
            workspace: &workspace,
            blend_file: &blend_file,
            config,
            destination: None,
            logical_workspace: None,
        };
        self.run(&request, context)?
            .ok_or_else(|| CoreError::External("Inspection Blender absente.".into()))
    }

    pub fn export_glb(
        &self,
        workspace: &Path,
        config: &ExportConfig,
        destination: &Path,
        context: &OperationContext,
    ) -> Result<Inspection> {
        self.export_operation("export", workspace, None, config, destination, context)
    }

    /// Generate the canonical GLB from the unique source, using its complete active scene.
    /// Available external resources are resolved from the logical source location.
    /// Missing render inputs and modifier/shape-key incompatibilities remain fatal.
    pub fn generate_glb(
        &self,
        workspace: &Path,
        logical_workspace: &Path,
        config: &ExportConfig,
        destination: &Path,
        context: &OperationContext,
    ) -> Result<Inspection> {
        self.export_operation(
            "generate",
            workspace,
            Some(logical_workspace),
            config,
            destination,
            context,
        )
    }

    /// Discover preview dependencies without evaluating mesh geometry.
    /// Paths resolve against the logical asset location rather than its temporary copy.
    pub fn inspect_preview(
        &self,
        workspace: &Path,
        logical_workspace: &Path,
        config: &ExportConfig,
        context: &OperationContext,
    ) -> Result<Inspection> {
        context.check_cancelled()?;
        let workspace = fs::canonicalize(workspace)?;
        let logical_workspace = absolute_logical_path(logical_workspace)?;
        let blend_file = primary_blend(&workspace, config)?;
        self.run(
            &Request {
                protocol_version: 1,
                operation: "inspectPreview",
                workspace: &workspace,
                blend_file: &blend_file,
                config,
                destination: None,
                logical_workspace: Some(logical_workspace),
            },
            context,
        )?
        .ok_or_else(|| CoreError::External("Inspection de l'aperçu Blender absente.".into()))
    }

    /// Generate a best-effort preview without relaxing the strict export contract.
    pub fn preview_glb(
        &self,
        workspace: &Path,
        logical_workspace: &Path,
        config: &ExportConfig,
        destination: &Path,
        context: &OperationContext,
    ) -> Result<Inspection> {
        self.export_operation(
            "preview",
            workspace,
            Some(logical_workspace),
            config,
            destination,
            context,
        )
    }

    fn export_operation(
        &self,
        operation: &str,
        workspace: &Path,
        logical_workspace: Option<&Path>,
        config: &ExportConfig,
        destination: &Path,
        context: &OperationContext,
    ) -> Result<Inspection> {
        context.check_cancelled()?;
        if destination.exists() {
            return Err(CoreError::Invalid(
                "La destination de l'export existe déjà.".into(),
            ));
        }
        if !destination
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("glb"))
        {
            return Err(CoreError::Invalid(
                "L'export doit porter l'extension .glb.".into(),
            ));
        }
        let workspace = fs::canonicalize(workspace)?;
        let logical_workspace = logical_workspace.map(absolute_logical_path).transpose()?;
        let blend_file = primary_blend(&workspace, config)?;
        let parent = destination
            .parent()
            .ok_or_else(|| CoreError::Invalid("Destination d'export invalide.".into()))?;
        fs::create_dir_all(parent)?;
        let destination = fs::canonicalize(parent)?.join(
            destination
                .file_name()
                .ok_or_else(|| CoreError::Invalid("Nom d'export invalide.".into()))?,
        );
        let request = Request {
            protocol_version: 1,
            operation,
            workspace: &workspace,
            blend_file: &blend_file,
            config,
            destination: Some(&destination),
            logical_workspace,
        };
        let result = self.run(&request, context).and_then(|inspection| {
            context.check_cancelled()?;
            crate::glb::validate_glb(&destination)?;
            inspection
                .ok_or_else(|| CoreError::External("Résultat d'export Blender absent.".into()))
        });
        if result.is_err() {
            let _ = fs::remove_file(&destination);
        }
        result
    }

    fn run(&self, request: &Request<'_>, context: &OperationContext) -> Result<Option<Inspection>> {
        context.check_cancelled()?;
        let directory = tempfile::Builder::new()
            .prefix("edge-lock-blender-")
            .tempdir()?;
        let worker = directory.path().join("worker.py");
        let input = directory.path().join("request.json");
        let output = directory.path().join("response.json");
        fs::write(&worker, WORKER)?;
        fs::write(&input, serde_json::to_vec(request)?)?;
        for name in ["config", "scripts", "extensions", "temp"] {
            fs::create_dir(directory.path().join(name))?;
        }
        let mut command = hidden_command(&self.executable);
        command
            .current_dir(directory.path())
            .env("BLENDER_USER_CONFIG", directory.path().join("config"))
            .env("BLENDER_USER_SCRIPTS", directory.path().join("scripts"))
            .env("TEMP", directory.path().join("temp"))
            .env("TMP", directory.path().join("temp"))
            .env("TMPDIR", directory.path().join("temp"))
            .env(
                "BLENDER_USER_EXTENSIONS",
                directory.path().join("extensions"),
            )
            .env_remove("PYTHONPATH")
            .env_remove("PYTHONHOME")
            .args([
                "--background",
                "--factory-startup",
                "--disable-autoexec",
                "--offline-mode",
                "--python-exit-code",
                "1",
                "--python",
            ])
            .arg(worker)
            .arg("--")
            .arg(input)
            .arg(&output);
        context.report("blender", 0, None);
        let (success, log) = run_process(command, self.timeout, context)?;
        context.check_cancelled()?;
        if !output.is_file() {
            let tail = log
                .chars()
                .rev()
                .take(2000)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>();
            return Err(CoreError::External(format!(
                "Blender n'a pas renvoyé de résultat. {tail}"
            )));
        }
        if output.metadata()?.len() > RESPONSE_LIMIT {
            return Err(CoreError::External(
                "Réponse Blender trop volumineuse.".into(),
            ));
        }
        let response: Response = serde_json::from_slice(&fs::read(output)?)?;
        if response.protocol_version != 1 {
            return Err(CoreError::External(
                "Protocole Blender incompatible.".into(),
            ));
        }
        if let Some(error) = response.error {
            return Err(CoreError::External(error));
        }
        if !success {
            return Err(CoreError::External(
                "Blender s'est arrêté avec une erreur.".into(),
            ));
        }
        context.report("blender", 1, Some(1));
        Ok(response.result)
    }
}

fn primary_blend(workspace: &Path, config: &ExportConfig) -> Result<PathBuf> {
    if let Some(relative) = config.primary_file.as_deref() {
        let path = Path::new(relative);
        if path.is_absolute()
            || path
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err(CoreError::Invalid(
                "Le fichier principal doit être relatif au dossier de l'asset.".into(),
            ));
        }
        let path = fs::canonicalize(workspace.join(path))?;
        if !path.starts_with(workspace)
            || !path.is_file()
            || !path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("blend"))
        {
            return Err(CoreError::Invalid(
                "Fichier .blend principal invalide ou extérieur à l'asset.".into(),
            ));
        }
        return Ok(path);
    }
    let mut directories = vec![(workspace.to_path_buf(), 0u8)];
    let mut blends = Vec::new();
    let mut visited = 0usize;
    while let Some((directory, depth)) = directories.pop() {
        if depth > 64 {
            return Err(CoreError::Invalid(
                "Arborescence de l'asset trop profonde.".into(),
            ));
        }
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            visited += 1;
            if visited > 200_000 {
                return Err(CoreError::Invalid(
                    "Trop de fichiers pour rechercher le .blend principal.".into(),
                ));
            }
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                directories.push((entry.path(), depth + 1));
            } else if kind.is_file()
                && entry
                    .path()
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("blend"))
            {
                blends.push(entry.path());
            }
        }
    }
    match blends.len() {
        0 => Err(CoreError::NotFound(
            "Aucun fichier .blend dans ce dossier.".into(),
        )),
        1 => Ok(blends.remove(0)),
        _ => Err(CoreError::Invalid(
            "Plusieurs fichiers .blend : choisissez le fichier principal.".into(),
        )),
    }
}
