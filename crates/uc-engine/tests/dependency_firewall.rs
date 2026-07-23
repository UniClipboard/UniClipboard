use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;

use cargo_metadata::{DependencyKind, MetadataCommand, PackageId};

const CORE_PACKAGES: [&str; 3] = ["uc-application", "uc-infra", "uc-engine"];
const DESKTOP_ONLY_PACKAGES: [&str; 7] = [
    "uc-app-paths",
    "uc-bootstrap",
    "uc-observability",
    "uc-platform",
    "uc-webserver",
    "uc-daemon-local",
    "uc-desktop",
];

#[test]
fn core_packages_do_not_declare_desktop_path_or_observability_dependencies() {
    let metadata = workspace_metadata();

    for package_name in CORE_PACKAGES {
        let package = metadata
            .packages
            .iter()
            .find(|package| package.name == package_name)
            .unwrap_or_else(|| panic!("{package_name} must be a workspace package"));
        let violations = package
            .dependencies
            .iter()
            .filter(|dependency| {
                matches!(
                    dependency.name.as_str(),
                    "uc-app-paths" | "uc-observability"
                )
            })
            .map(|dependency| dependency.name.as_str())
            .collect::<BTreeSet<_>>();

        assert!(
            violations.is_empty(),
            "{package_name} must not depend on desktop-owned packages: {violations:?}"
        );
    }
}

#[test]
fn engine_normal_dependency_closure_excludes_desktop_packages() {
    let metadata = workspace_metadata();
    let resolve = metadata
        .resolve
        .expect("workspace dependency graph is required");
    let package_names = metadata
        .packages
        .iter()
        .map(|package| (package.id.clone(), package.name.as_str()))
        .collect::<HashMap<_, _>>();
    let nodes = resolve
        .nodes
        .iter()
        .map(|node| (node.id.clone(), node))
        .collect::<HashMap<_, _>>();
    let engine_id = package_names
        .iter()
        .find_map(|(id, name)| (*name == "uc-engine").then(|| id.clone()))
        .expect("uc-engine must be in the dependency graph");

    let mut pending = vec![engine_id];
    let mut visited = HashSet::<PackageId>::new();
    while let Some(package_id) = pending.pop() {
        if !visited.insert(package_id.clone()) {
            continue;
        }
        let node = nodes
            .get(&package_id)
            .unwrap_or_else(|| panic!("missing dependency node for {package_id}"));
        pending.extend(
            node.deps
                .iter()
                .filter(|dependency| {
                    dependency
                        .dep_kinds
                        .iter()
                        .any(|kind| kind.kind == DependencyKind::Normal)
                })
                .map(|dependency| dependency.pkg.clone()),
        );
    }

    let violations = visited
        .iter()
        .filter_map(|id| package_names.get(id).copied())
        .filter(|name| DESKTOP_ONLY_PACKAGES.contains(name))
        .collect::<BTreeSet<_>>();
    assert!(
        violations.is_empty(),
        "uc-engine normal dependency closure contains desktop packages: {violations:?}"
    );
}

#[test]
fn engine_default_dependency_contract_excludes_lan_compat_dependencies() {
    let metadata = workspace_metadata();
    let engine = package(&metadata, "uc-engine");
    let application = package(&metadata, "uc-application");
    let infra = package(&metadata, "uc-infra");

    assert_default_does_not_enable(engine, "lan-compat");
    assert_default_does_not_enable(application, "lan-compat");
    assert_default_does_not_enable(infra, "lan-compat");

    let application_dependency = normal_dependency(engine, "uc-application");
    let infra_dependency = normal_dependency(engine, "uc-infra");
    assert!(!application_dependency
        .features
        .contains(&"lan-compat".to_string()));
    assert!(!infra_dependency
        .features
        .contains(&"lan-compat".to_string()));

    let mobile_proto = normal_dependency(application, "uc-mobile-proto");
    assert!(
        mobile_proto.optional,
        "uc-mobile-proto must remain optional in uc-application"
    );
    let network_interface = normal_dependency(infra, "network-interface");
    assert!(
        network_interface.optional,
        "network-interface must remain optional in uc-infra"
    );

    assert_feature_enables(application, "lan-compat", "dep:uc-mobile-proto");
    assert_feature_enables(infra, "lan-compat", "dep:network-interface");
    assert_feature_enables(engine, "lan-compat", "dep:uc-mobile-proto");
    assert_feature_enables(engine, "lan-compat", "uc-application/lan-compat");
    assert_feature_enables(engine, "lan-compat", "uc-infra/lan-compat");
}

#[test]
fn engine_consumers_select_lan_compat_explicitly() {
    let metadata = workspace_metadata();

    for package_name in ["uc-engine-uniffi", "uc-ohos-napi", "uc-mobile-probe-core"] {
        let dependency = normal_dependency(package(&metadata, package_name), "uc-engine");
        assert!(
            !dependency.features.contains(&"lan-compat".to_string()),
            "{package_name} must not enable uc-engine/lan-compat"
        );
    }

    let webserver = normal_dependency(package(&metadata, "uc-webserver"), "uc-engine");
    assert!(
        webserver.features.contains(&"lan-compat".to_string()),
        "uc-webserver must enable uc-engine/lan-compat"
    );
}

fn package<'a>(
    metadata: &'a cargo_metadata::Metadata,
    package_name: &str,
) -> &'a cargo_metadata::Package {
    metadata
        .packages
        .iter()
        .find(|package| package.name == package_name)
        .unwrap_or_else(|| panic!("{package_name} must be a workspace package"))
}

fn normal_dependency<'a>(
    package: &'a cargo_metadata::Package,
    dependency_name: &str,
) -> &'a cargo_metadata::Dependency {
    package
        .dependencies
        .iter()
        .find(|dependency| {
            dependency.name == dependency_name && dependency.kind == DependencyKind::Normal
        })
        .unwrap_or_else(|| {
            panic!(
                "{} must declare a normal dependency on {dependency_name}",
                package.name
            )
        })
}

fn assert_default_does_not_enable(package: &cargo_metadata::Package, feature: &str) {
    let default_features = package.features.get("default");
    assert!(
        default_features.is_none_or(|features| !features.iter().any(|item| item == feature)),
        "{} default feature must not enable {feature}",
        package.name
    );
}

fn assert_feature_enables(package: &cargo_metadata::Package, feature: &str, expected: &str) {
    let enabled = package
        .features
        .get(feature)
        .unwrap_or_else(|| panic!("{} must declare feature {feature}", package.name));
    assert!(
        enabled.iter().any(|item| item == expected),
        "{}/{} must enable {expected}",
        package.name,
        feature
    );
}

fn workspace_metadata() -> cargo_metadata::Metadata {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    MetadataCommand::new()
        .manifest_path(workspace_root.join("Cargo.toml"))
        .exec()
        .expect("workspace metadata must be readable")
}
