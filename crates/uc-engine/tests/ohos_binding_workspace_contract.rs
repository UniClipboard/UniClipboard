use std::collections::BTreeSet;
use std::path::Path;

use cargo_metadata::{CrateType, DependencyKind, MetadataCommand};

#[test]
fn ohos_binding_is_a_workspace_member_with_a_public_engine_boundary() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
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
