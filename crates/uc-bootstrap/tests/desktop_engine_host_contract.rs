use std::path::Path;

use uc_bootstrap::{
    prepare_desktop_engine_host, prepare_desktop_engine_host_for_profile, DesktopEngineHost,
    DesktopHostFileHandles, DesktopRuntimeProfileConfig,
};
use uc_engine::HostFileAccess;

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct ScopedEnv {
    values: Vec<(&'static str, Option<String>)>,
}

impl ScopedEnv {
    fn set(values: &[(&'static str, &'static str)]) -> Self {
        let previous = values
            .iter()
            .map(|(name, value)| {
                let previous = std::env::var(name).ok();
                std::env::set_var(name, value);
                (*name, previous)
            })
            .collect();
        Self { values: previous }
    }
}

impl Drop for ScopedEnv {
    fn drop(&mut self) {
        for (name, value) in self.values.drain(..).rev() {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
    }
}

#[test]
fn desktop_engine_host_has_a_single_preparation_entry() {
    let _prepare: fn() -> uc_bootstrap::WiringResult<DesktopEngineHost> =
        prepare_desktop_engine_host;
}

#[test]
fn desktop_engine_host_preparation_does_not_assemble_the_core() {
    let source = include_str!("../src/wiring/desktop_host.rs");
    assert!(!source.contains("wire_dependencies("));
    assert!(!source.contains("Engine::start("));
}

#[test]
fn explicit_profile_hosts_isolate_every_persistent_boundary_from_ambient_profile() {
    let _env = ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _scoped_env = ScopedEnv::set(&[
        ("UC_PROFILE", "ambient-must-not-leak"),
        ("UC_DISABLE_SYSTEM_CLIPBOARD", "1"),
    ]);

    let temporary = tempfile::tempdir().unwrap();
    let profile_config = |profile_id: &str| {
        DesktopRuntimeProfileConfig::new(
            profile_id,
            temporary.path().join(profile_id).join("data"),
            temporary.path().join(profile_id).join("cache"),
            temporary.path().join(profile_id).join("logs"),
        )
        .unwrap()
    };
    let config_a = profile_config("019d-profile-a");
    let config_b = profile_config("019d-profile-b");
    assert_ne!(
        config_a.secure_storage_namespace(),
        config_b.secure_storage_namespace()
    );
    let profile_a = prepare_desktop_engine_host_for_profile(config_a).unwrap();
    let profile_b = prepare_desktop_engine_host_for_profile(config_b).unwrap();

    let expected_a = temporary.path().join("019d-profile-a");
    let expected_b = temporary.path().join("019d-profile-b");
    assert_eq!(
        profile_a.process_paths().app_data_root(),
        expected_a.join("data")
    );
    assert_eq!(
        profile_b.process_paths().app_data_root(),
        expected_b.join("data")
    );

    let (engine_a, capabilities_a) = profile_a.into_engine_start();
    let (engine_b, capabilities_b) = profile_b.into_engine_start();
    assert_eq!(engine_a.profile_id(), "019d-profile-a");
    assert_eq!(engine_b.profile_id(), "019d-profile-b");

    let directories_a = capabilities_a.directories();
    let directories_b = capabilities_b.directories();
    assert_eq!(directories_a.private_data(), expected_a.join("data"));
    assert_eq!(directories_b.private_data(), expected_b.join("data"));
    assert_eq!(directories_a.cache(), expected_a.join("cache"));
    assert_eq!(directories_b.cache(), expected_b.join("cache"));
    assert_eq!(directories_a.logs(), expected_a.join("logs"));
    assert_eq!(directories_b.logs(), expected_b.join("logs"));
    assert_eq!(
        directories_a.temporary(),
        expected_a.join("cache/engine-tmp")
    );
    assert_eq!(
        directories_b.temporary(),
        expected_b.join("cache/engine-tmp")
    );

    for relative in [
        Path::new("uniclipboard.db"),
        Path::new("vault/blobs"),
        Path::new("iroh-identity"),
    ] {
        assert_ne!(
            directories_a.private_data().join(relative),
            directories_b.private_data().join(relative),
            "persistent boundary must be profile-isolated: {}",
            relative.display()
        );
    }
}

#[test]
fn daemon_runtime_connects_host_analytics_end_to_end() {
    let desktop_host = include_str!("../src/wiring/desktop_host.rs");
    let daemon_host = include_str!("../../../apps/daemon/src/daemon/host.rs");

    assert!(
        desktop_host.contains(".with_analytics("),
        "desktop host capabilities must inject the host-owned analytics sink and identity"
    );
    assert!(
        daemon_host.contains("initialize_analytics_context("),
        "daemon startup must install the analytics event context before events are emitted"
    );
    assert!(
        daemon_host.contains(".with_analytics(analytics_sink)"),
        "daemon HTTP analytics events must use the same authoritative sink"
    );
}

#[test]
fn desktop_host_file_handles_share_opaque_input_and_output_paths() {
    let temp = tempfile::tempdir().unwrap();
    let input_path = temp.path().join("private-input.txt");
    let output_path = temp.path().join("private-output.txt");
    std::fs::write(&input_path, b"input bytes").unwrap();

    let handles = DesktopHostFileHandles::default();
    let input = handles.register_input(input_path.clone()).unwrap();
    let output = handles.register_output(output_path.clone()).unwrap();

    assert_eq!(handles.read_chunk(&input, 0, 64).unwrap(), b"input bytes");
    handles.write_chunk(&output, 0, b"output ").unwrap();
    handles.write_chunk(&output, 7, b"bytes").unwrap();
    handles.finish_write(&output).unwrap();
    assert_eq!(std::fs::read(&output_path).unwrap(), b"output bytes");

    for handle in [input, output] {
        let debug = format!("{handle:?}");
        assert!(!debug.contains("private-input.txt"));
        assert!(!debug.contains("private-output.txt"));
        assert!(!debug.contains(temp.path().to_string_lossy().as_ref()));
    }
}
