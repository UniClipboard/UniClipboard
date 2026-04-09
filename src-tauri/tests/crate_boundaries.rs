use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn cargo_metadata() -> Value {
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1"])
        .current_dir(workspace_root())
        .output()
        .expect("cargo metadata should run");

    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    serde_json::from_slice(&output.stdout).expect("cargo metadata should decode")
}

fn package_id_by_name(metadata: &Value, name: &str) -> String {
    metadata["packages"]
        .as_array()
        .expect("packages should be an array")
        .iter()
        .find(|package| package["name"].as_str() == Some(name))
        .unwrap_or_else(|| panic!("package {name} should exist"))["id"]
        .as_str()
        .expect("package id should be a string")
        .to_string()
}

fn has_package(metadata: &Value, name: &str) -> bool {
    metadata["packages"]
        .as_array()
        .expect("packages should be an array")
        .iter()
        .any(|package| package["name"].as_str() == Some(name))
}

fn declares_dependency(metadata: &Value, package_name: &str, dependency_name: &str) -> bool {
    metadata["packages"]
        .as_array()
        .expect("packages should be an array")
        .iter()
        .find(|package| package["name"].as_str() == Some(package_name))
        .unwrap_or_else(|| panic!("package {package_name} should exist"))["dependencies"]
        .as_array()
        .expect("package dependencies should be an array")
        .iter()
        .any(|dependency| dependency["name"].as_str() == Some(dependency_name))
}

fn depends_on(metadata: &Value, from: &str, target: &str) -> bool {
    let package_ids = metadata["packages"]
        .as_array()
        .expect("packages should be an array")
        .iter()
        .map(|package| {
            (
                package["id"]
                    .as_str()
                    .expect("package id should be a string")
                    .to_string(),
                package["name"]
                    .as_str()
                    .expect("package name should be a string")
                    .to_string(),
            )
        })
        .collect::<HashMap<_, _>>();
    let graph = metadata["resolve"]["nodes"]
        .as_array()
        .expect("resolve nodes should be an array")
        .iter()
        .map(|node| {
            (
                node["id"]
                    .as_str()
                    .expect("node id should be a string")
                    .to_string(),
                node["dependencies"]
                    .as_array()
                    .expect("node dependencies should be an array")
                    .iter()
                    .map(|dependency| {
                        dependency
                            .as_str()
                            .expect("dependency id should be a string")
                            .to_string()
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<HashMap<_, _>>();

    let start = package_id_by_name(metadata, from);
    let target = package_id_by_name(metadata, target);

    let mut seen = HashSet::new();
    let mut queue = VecDeque::from([start]);

    while let Some(current) = queue.pop_front() {
        if !seen.insert(current.clone()) {
            continue;
        }

        if current == target {
            return true;
        }

        if let Some(next) = graph.get(&current) {
            queue.extend(next.iter().cloned());
        } else if let Some(name) = package_ids.get(&current) {
            panic!("missing resolve node for package {name}");
        }
    }

    false
}

#[test]
fn uc_tauri_is_fully_detached_from_uc_daemon() {
    let metadata = cargo_metadata();
    assert!(
        !depends_on(&metadata, "uc-tauri", "uc-daemon"),
        "uc-tauri still depends on uc-daemon"
    );
}

#[test]
fn uc_daemon_client_is_fully_detached_from_uc_daemon() {
    let metadata = cargo_metadata();
    assert!(
        !depends_on(&metadata, "uc-daemon-client", "uc-daemon"),
        "uc-daemon-client still depends on uc-daemon"
    );
}

#[test]
fn daemon_shared_has_been_replaced_by_contract_and_local_crates() {
    let metadata = cargo_metadata();

    assert!(
        !has_package(&metadata, "uc-daemon-shared"),
        "legacy uc-daemon-shared crate should not remain in the workspace"
    );
    assert!(
        has_package(&metadata, "uc-daemon-contract"),
        "uc-daemon-contract should exist in the workspace"
    );
    assert!(
        has_package(&metadata, "uc-daemon-local"),
        "uc-daemon-local should exist in the workspace"
    );
}

#[test]
fn uc_tauri_uses_contract_and_local_layers() {
    let metadata = cargo_metadata();

    assert!(
        depends_on(&metadata, "uc-tauri", "uc-daemon-contract"),
        "uc-tauri should depend on uc-daemon-contract"
    );
    assert!(
        depends_on(&metadata, "uc-tauri", "uc-daemon-local"),
        "uc-tauri should depend on uc-daemon-local"
    );
}

#[test]
fn uc_daemon_client_uses_contract_and_local_layers() {
    let metadata = cargo_metadata();

    assert!(
        depends_on(&metadata, "uc-daemon-client", "uc-daemon-contract"),
        "uc-daemon-client should depend on uc-daemon-contract"
    );
    assert!(
        depends_on(&metadata, "uc-daemon-client", "uc-daemon-local"),
        "uc-daemon-client should depend on uc-daemon-local"
    );
}

#[test]
fn uc_daemon_client_does_not_own_sidecar_process_management() {
    let metadata = cargo_metadata();

    assert!(
        !declares_dependency(&metadata, "uc-daemon-client", "tauri-plugin-shell"),
        "uc-daemon-client should not depend on tauri-plugin-shell once process management moves to uc-daemon-local"
    );
}
