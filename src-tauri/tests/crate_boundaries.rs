use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

/// Returns the workspace root path for the current crate.
///
/// The path is derived from the crate's `CARGO_MANIFEST_DIR` at compile time.
///
/// # Examples
///
/// ```
/// let _root = workspace_root();
/// ```
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Load and parse this workspace's Cargo metadata into a JSON value.
///
/// # Returns
///
/// The decoded `serde_json::Value` produced by `cargo metadata`.
///
/// # Examples
///
/// ```
/// let meta = cargo_metadata();
/// assert!(meta.get("packages").is_some());
/// ```
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

/// Get the Cargo package ID for the package with the given manifest name.
///
/// # Panics
///
/// Panics if `metadata["packages"]` is not an array, if no package with the given `name` exists,
/// or if the selected package's `"id"` is not a string.
///
/// # Examples
///
/// ```
/// use serde_json::json;
/// let meta = json!({
///     "packages": [ { "name": "foo", "id": "foo 0.1.0 (path+file:///workspace/foo)" } ]
/// });
/// let id = package_id_by_name(&meta, "foo");
/// assert_eq!(id, "foo 0.1.0 (path+file:///workspace/foo)");
/// ```
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

/// Checks whether the provided Cargo metadata contains a package with the given name.
///
/// Returns `true` if `metadata["packages"]` includes an object whose `"name"` field equals `name`, `false` otherwise.
///
/// # Panics
///
/// Panics if `metadata["packages"]` is not a JSON array.
///
/// # Examples
///
/// ```
/// use serde_json::json;
/// let metadata = json!({ "packages": [ { "name": "foo" } ] });
/// assert!(has_package(&metadata, "foo"));
/// assert!(!has_package(&metadata, "bar"));
/// ```
fn has_package(metadata: &Value, name: &str) -> bool {
    metadata["packages"]
        .as_array()
        .expect("packages should be an array")
        .iter()
        .any(|package| package["name"].as_str() == Some(name))
}

/// Checks whether the workspace package named `package_name` lists `dependency_name` as a direct dependency.
///
/// # Returns
///
/// `true` if package `package_name` has a declared, direct dependency on `dependency_name`, `false` otherwise.
///
/// # Examples
///
/// ```
/// let metadata = cargo_metadata();
/// assert!(declares_dependency(&metadata, "uc-tauri", "uc-daemon-contract"));
/// ```
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

/// Determines whether the package named `from` (by package name) transitively depends on the package named `target` according to `cargo metadata`.
///
/// The `metadata` value must be the JSON produced by `cargo metadata --format-version 1`. The function resolves package names to package IDs and performs a breadth-first search over the resolved dependency graph.
///
/// # Parameters
///
/// - `metadata`: Cargo workspace metadata as parsed JSON.
/// - `from`: The package name to start the dependency search from.
/// - `target`: The package name to search for as a transitive dependency.
///
/// # Returns
///
/// `true` if `from` depends on `target` (directly or transitively), `false` otherwise.
///
/// # Panics
///
/// - If a package named `from` or `target` does not exist in `metadata`.
/// - If a package listed in `metadata["packages"]` has no corresponding resolve node in `metadata["resolve"]["nodes"]`.
///
/// # Examples
///
/// ```
/// use serde_json::json;
///
/// // Minimal metadata where `a` depends on `b`.
/// let metadata = json!({
///     "packages": [
///         { "id": "a", "name": "a" },
///         { "id": "b", "name": "b" }
///     ],
///     "resolve": {
///         "nodes": [
///             { "id": "a", "dependencies": ["b"] },
///             { "id": "b", "dependencies": [] }
///         ]
///     }
/// });
///
/// assert!(depends_on(&metadata, "a", "b"));
/// assert!(!depends_on(&metadata, "b", "a"));
/// ```
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

/// Verifies that the workspace crate `uc-tauri` depends on both `uc-daemon-contract` and `uc-daemon-local`.
///
/// # Examples
///
/// ```
/// let metadata = cargo_metadata();
/// assert!(depends_on(&metadata, "uc-tauri", "uc-daemon-contract"));
/// assert!(depends_on(&metadata, "uc-tauri", "uc-daemon-local"));
/// ```
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
