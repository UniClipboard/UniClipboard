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
