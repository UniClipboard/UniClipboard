use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use cargo_metadata::{CrateType, DependencyKind, MetadataCommand};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(relative: &str) -> String {
    let path = workspace_root().join(relative);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
}

#[test]
fn ohos_binding_is_a_workspace_member_with_a_public_engine_boundary() {
    let workspace_root = workspace_root();
    let metadata = MetadataCommand::new()
        .manifest_path(workspace_root.join("Cargo.toml"))
        .no_deps()
        .exec()
        .expect("workspace metadata must be readable");
    let package = metadata
        .packages
        .iter()
        .find(|package| package.name == "uc-ohos-napi")
        .expect("uc-ohos-napi must be a workspace member");

    let library = package
        .targets
        .iter()
        .find(|target| target.name == "uc_ohos_napi")
        .expect("uc-ohos-napi must expose a library target");
    let crate_types = library.crate_types.iter().cloned().collect::<BTreeSet<_>>();
    assert_eq!(
        crate_types,
        BTreeSet::from([CrateType::CDyLib, CrateType::Lib])
    );

    let dependencies = package
        .dependencies
        .iter()
        .filter(|dependency| dependency.kind == DependencyKind::Normal)
        .map(|dependency| dependency.name.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        dependencies,
        BTreeSet::from(["napi", "napi-derive", "tokio", "uc-engine", "zeroize"])
    );
    for forbidden in ["uc-core", "uc-application", "uc-infra", "uc-bootstrap"] {
        assert!(
            !dependencies.contains(forbidden),
            "binding must not depend directly on {forbidden}"
        );
    }
}

#[test]
fn ohos_binding_registers_its_napi_exports_when_the_library_loads() {
    let binding = read("crates/uc-ohos-napi/src/lib.rs");

    assert!(binding.contains("cfg(target_env = \"ohos\")"));
    assert!(!binding.contains("cfg(target_os = \"ohos\")"));
    assert!(binding.contains("napi_module_register"));
    assert!(binding.contains("napi_register_module_v1"));
}

#[test]
fn ohos_binding_links_the_harmony_napi_runtime() {
    let build_script = read("crates/uc-ohos-napi/build.rs");

    assert!(build_script.contains("CARGO_CFG_TARGET_ENV"));
    assert!(build_script.contains("ace_napi.z"));
}
