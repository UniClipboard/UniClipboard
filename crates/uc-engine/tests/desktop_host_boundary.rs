use std::path::{Path, PathBuf};

#[test]
fn daemon_does_not_export_legacy_runtime_fields() {
    let daemon_source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/daemon/src");
    let violations = rust_sources(&daemon_source)
        .into_iter()
        .flat_map(|path| {
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            source
                .lines()
                .enumerate()
                .filter_map(move |(index, line)| {
                    let line = line.trim_start();
                    let exports_app_facade = line.starts_with("pub app_facade:");
                    let exports_app_deps = line.starts_with("pub ") && line.contains("AppDeps");
                    (exports_app_facade || exports_app_deps)
                        .then(|| format!("{}:{}: {line}", path.display(), index + 1))
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert!(
        violations.is_empty(),
        "daemon host must not export legacy runtime fields:\n{}",
        violations.join("\n")
    );
}

#[test]
fn daemon_run_loop_does_not_depend_on_legacy_application_runtime() {
    let run_loop =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/daemon/src/daemon/run_loop.rs");
    let source = std::fs::read_to_string(&run_loop)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", run_loop.display()));

    assert!(
        !source.contains("AppFacade") && !source.contains("AppDeps"),
        "daemon run loop must only own process run and shutdown ordering"
    );
}

#[test]
fn daemon_library_does_not_export_in_process_legacy_assembly() {
    let library = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/daemon/src/lib.rs");
    let source = std::fs::read_to_string(&library)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", library.display()));

    assert!(
        !source.contains("ProcessRuntimeHandles")
            && !source.contains("start_in_process")
            && !source.contains("ProcessRuntimeContext")
            && !source.contains("build_process_runtime"),
        "daemon library must not export legacy in-process assembly"
    );
}

#[test]
fn daemon_process_runtime_does_not_expose_task_registry() {
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/daemon/src/daemon/process_runtime.rs");
    let source = std::fs::read_to_string(&runtime)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", runtime.display()));

    assert!(
        !source.contains("fn task_registry("),
        "daemon process runtime must own background task registration"
    );
}

#[test]
fn daemon_process_runtime_does_not_expose_app_facade() {
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/daemon/src/daemon/process_runtime.rs");
    let source = std::fs::read_to_string(&runtime)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", runtime.display()));

    assert!(
        !source.contains("fn app_facade("),
        "daemon process runtime must expose behavior instead of the legacy facade"
    );
}

#[test]
fn daemon_startup_recovery_delegates_business_orchestration_to_engine() {
    let recovery = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/daemon/src/daemon/startup_recovery.rs");
    let source = std::fs::read_to_string(&recovery)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", recovery.display()));
    let forbidden = [
        "recover_encryption_session",
        "SpaceSetupFacade",
        "try_resume_session",
        "refresh_presence",
        "input.receive_readiness.ensure_receive_ready",
    ];
    let violations = forbidden
        .into_iter()
        .filter(|token| source.contains(token))
        .collect::<Vec<_>>();

    assert!(
        violations.is_empty(),
        "daemon startup recovery must leave business recovery to uc-engine; found: {}",
        violations.join(", ")
    );
    assert!(
        source.contains("execute_recover_session("),
        "daemon startup recovery must invoke the uc-engine recovery implementation"
    );
}

fn rust_sources(root: &Path) -> Vec<PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    let mut sources = Vec::new();

    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", directory.display()))
        {
            let path = entry
                .unwrap_or_else(|error| panic!("failed to inspect directory entry: {error}"))
                .path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                sources.push(path);
            }
        }
    }

    sources
}
