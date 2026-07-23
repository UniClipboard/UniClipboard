use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use cargo_metadata::{DependencyKind, MetadataCommand};
use syn::visit::Visit;

const CONSUMER_PACKAGES: [&str; 9] = [
    "uc-bootstrap",
    "uc-cli",
    "uc-daemon",
    "uc-daemon-client",
    "uc-daemon-contract",
    "uc-desktop",
    "uc-platform",
    "uc-tauri",
    "uc-webserver",
];

const INTERNAL_PACKAGES: [&str; 4] = ["uc-core", "uc-application", "uc-infra", "uc-mobile-proto"];

const PRODUCTION_SOURCE_ROOTS: [&str; 9] = [
    "apps/cli/src",
    "apps/daemon/src",
    "crates/uc-bootstrap/src",
    "crates/uc-daemon-client/src",
    "crates/uc-daemon-contract/src",
    "crates/uc-desktop/src",
    "crates/uc-platform/src",
    "crates/uc-webserver/src/api",
    "src-tauri/crates/uc-tauri/src",
];

#[test]
fn desktop_consumers_do_not_declare_normal_dependencies_on_core_internals() {
    let metadata = MetadataCommand::new()
        .manifest_path(workspace_root().join("Cargo.toml"))
        .exec()
        .expect("workspace metadata must be readable");

    for package_name in CONSUMER_PACKAGES {
        let package = metadata
            .packages
            .iter()
            .find(|package| package.name == package_name)
            .unwrap_or_else(|| panic!("{package_name} must be a workspace package"));
        let violations = package
            .dependencies
            .iter()
            .filter(|dependency| dependency.kind == DependencyKind::Normal)
            .filter(|dependency| INTERNAL_PACKAGES.contains(&dependency.name.as_str()))
            .map(|dependency| dependency.name.as_str())
            .collect::<BTreeSet<_>>();

        assert!(
            violations.is_empty(),
            "{package_name} must use uc-engine instead of core internals: {violations:?}"
        );
    }
}

#[test]
fn desktop_production_sources_do_not_reference_core_internal_paths() {
    let root = workspace_root();
    let mut violations = Vec::new();

    for source_root in PRODUCTION_SOURCE_ROOTS {
        visit_rust_files(&root.join(source_root), &mut |path| {
            let source = fs::read_to_string(path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            let syntax = syn::parse_file(&source)
                .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()));
            let mut visitor = InternalPathVisitor::default();
            visitor.visit_file(&syntax);
            for internal in visitor.references {
                violations.push(format!(
                    "{} references {internal}",
                    path.strip_prefix(&root).unwrap_or(path).display()
                ));
            }
        });
    }

    assert!(
        violations.is_empty(),
        "desktop production sources must use uc-engine:\n{}",
        violations.join("\n")
    );
}

#[derive(Default)]
struct InternalPathVisitor {
    references: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for InternalPathVisitor {
    fn visit_path(&mut self, path: &'ast syn::Path) {
        if let Some(first) = path.segments.first() {
            let crate_name = first.ident.to_string();
            let package_name = crate_name.replace('_', "-");
            if INTERNAL_PACKAGES.contains(&package_name.as_str()) {
                self.references.insert(crate_name);
            }
        }
        syn::visit::visit_path(self, path);
    }
}

fn visit_rust_files(directory: &Path, visit: &mut impl FnMut(&Path)) {
    for entry in fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", directory.display()))
    {
        let path = entry.expect("directory entry must be readable").path();
        if path.is_dir() {
            visit_rust_files(&path, visit);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            visit(&path);
        }
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
