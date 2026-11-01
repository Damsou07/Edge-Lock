use edge_lock_core::{ExportConfig, Library, OperationContext};
use std::{
    fs,
    path::Path,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

/// The helper exits without destructors at precise publication boundaries. Only a
/// parent-created disposable directory is passed to the child process.
#[test]
#[ignore = "subprocess helper invoked exclusively by the crash regression"]
fn crash_helper() {
    let Ok(root) = std::env::var("EDGE_LOCK_CRASH_FIXTURE") else {
        return;
    };
    let library = Library::open(root).unwrap();
    if let Ok(variant_id) = std::env::var("EDGE_LOCK_CRASH_VARIANT") {
        let workspace = library.workspace_path(&variant_id).unwrap();
        fs::write(workspace.join("value.txt"), b"new working content").unwrap();
        let reports = Arc::new(AtomicUsize::new(0));
        let ctx = OperationContext {
            progress: Some(Arc::new(move |event| {
                // First report occurs during streaming, second after blob publication.
                if event.phase == "capture" && reports.fetch_add(1, Ordering::Relaxed) == 1 {
                    std::process::exit(91);
                }
            })),
            ..Default::default()
        };
        library
            .create_snapshot(&variant_id, "Interrupted snapshot", &ctx)
            .unwrap();
    } else {
        let ctx = OperationContext {
            progress: Some(Arc::new(move |event| {
                if event.phase == "materialize" {
                    std::process::exit(91);
                }
            })),
            ..Default::default()
        };
        library
            .create_empty("Interrupted import", ExportConfig::default(), &ctx)
            .unwrap();
    }
    panic!("The requested crash boundary was not reached");
}

fn crash(root: &Path, variant: Option<&str>) {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--ignored", "--exact", "crash_helper", "--nocapture"])
        .env("EDGE_LOCK_CRASH_FIXTURE", root);
    if let Some(id) = variant {
        command.env("EDGE_LOCK_CRASH_VARIANT", id);
    } else {
        command.env_remove("EDGE_LOCK_CRASH_VARIANT");
    }
    let output = command.output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(91),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn abrupt_process_exit_preserves_published_history_and_recovers_unpublished_workspace() {
    let temp = tempfile::tempdir().unwrap();
    let interrupted = temp.path().join("interrupted");
    crash(&interrupted, None);
    let recovered = Library::open(&interrupted).unwrap();
    assert_eq!(recovered.info().unwrap().asset_count, 0);
    assert_eq!(recovered.info().unwrap().recovery_notices.len(), 1);
    assert_eq!(
        fs::read_dir(interrupted.join("workspaces"))
            .unwrap()
            .count(),
        1
    );

    let root = temp.path().join("existing");
    let input = temp.path().join("source");
    fs::create_dir(&input).unwrap();
    fs::write(input.join("value.txt"), b"original").unwrap();
    let library = Library::open(&root).unwrap();
    let asset = library
        .import_folder(
            &input,
            "Existing",
            ExportConfig::default(),
            &OperationContext::default(),
        )
        .unwrap();
    let variant = asset.variants[0].clone();
    drop(library);
    crash(&root, Some(&variant.id));
    let recovered = Library::open(&root).unwrap();
    let detail = recovered.get_variant(&variant.id).unwrap();
    assert_eq!(detail.variant.head_snapshot_id, variant.head_snapshot_id);
    assert_eq!(detail.snapshots.len(), 1);
    let output = temp.path().join("restored");
    recovered
        .materialize(
            &variant.head_snapshot_id,
            &output,
            &OperationContext::default(),
        )
        .unwrap();
    assert_eq!(fs::read(output.join("value.txt")).unwrap(), b"original");
    assert_eq!(
        fs::read(Path::new(&variant.workspace_path).join("value.txt")).unwrap(),
        b"new working content"
    );
}
