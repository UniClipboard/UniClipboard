use std::path::PathBuf;

use uc_app_paths::{DesktopRuntimeProfileConfig, DesktopRuntimeProfileConfigError};

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct ScopedEnv {
    name: &'static str,
    previous: Option<String>,
}

impl ScopedEnv {
    fn set(name: &'static str, value: &'static str) -> Self {
        let previous = std::env::var(name).ok();
        std::env::set_var(name, value);
        Self { name, previous }
    }
}

impl Drop for ScopedEnv {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(value) => std::env::set_var(self.name, value),
            None => std::env::remove_var(self.name),
        }
    }
}

#[test]
fn explicit_profile_roots_are_isolated_from_each_other_and_ambient_profile() {
    let _env = ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _scoped_env = ScopedEnv::set("UC_PROFILE", "ambient-must-not-leak");

    let temporary = tempfile::tempdir().unwrap();
    let profile_a = DesktopRuntimeProfileConfig::new(
        "019d-profile-a",
        temporary.path().join("data-a"),
        temporary.path().join("cache-a"),
        temporary.path().join("logs-a"),
    )
    .unwrap();
    let profile_b = DesktopRuntimeProfileConfig::new(
        "019d-profile-b",
        temporary.path().join("data-b"),
        temporary.path().join("cache-b"),
        temporary.path().join("logs-b"),
    )
    .unwrap();

    assert_eq!(profile_a.profile_id(), "019d-profile-a");
    assert_eq!(profile_b.profile_id(), "019d-profile-b");
    assert_ne!(profile_a.data_root(), profile_b.data_root());
    assert_ne!(profile_a.cache_root(), profile_b.cache_root());
    assert_ne!(profile_a.log_dir(), profile_b.log_dir());
    assert!(profile_a.data_root().starts_with(temporary.path()));
    assert!(profile_b.data_root().starts_with(temporary.path()));
}

#[test]
fn explicit_profile_rejects_relative_roots() {
    let temporary = tempfile::tempdir().unwrap();

    for (data_root, cache_root, log_dir, expected_root) in [
        (
            PathBuf::from("relative-data"),
            temporary.path().join("cache"),
            temporary.path().join("logs"),
            "data_root",
        ),
        (
            temporary.path().join("data"),
            PathBuf::from("relative-cache"),
            temporary.path().join("logs"),
            "cache_root",
        ),
        (
            temporary.path().join("data"),
            temporary.path().join("cache"),
            PathBuf::from("relative-logs"),
            "log_dir",
        ),
    ] {
        assert_eq!(
            DesktopRuntimeProfileConfig::new("019d-safe-profile", data_root, cache_root, log_dir,),
            Err(DesktopRuntimeProfileConfigError::RootMustBeAbsolute(
                expected_root,
            ))
        );
    }
}

#[test]
fn explicit_profile_rejects_traversal_and_windows_reserved_names() {
    let temporary = tempfile::tempdir().unwrap();

    for profile_id in [
        "../escape",
        "nested/name",
        "nested\\name",
        ".",
        "CON",
        "con",
        "PRN",
        "AUX",
        "NUL",
        "COM1",
        "LPT9",
    ] {
        assert_eq!(
            DesktopRuntimeProfileConfig::new(
                profile_id,
                temporary.path().join("data"),
                temporary.path().join("cache"),
                temporary.path().join("logs"),
            ),
            Err(DesktopRuntimeProfileConfigError::InvalidProfileId),
            "unsafe profile id must be rejected: {profile_id}"
        );
    }
}

#[cfg(windows)]
#[test]
fn explicit_profile_rejects_traversal_and_reserved_components_in_absolute_roots() {
    let temporary = tempfile::tempdir().unwrap();

    for (data_root, cache_root, log_dir, expected_root) in [
        (
            temporary.path().join("profiles/../escape"),
            temporary.path().join("cache"),
            temporary.path().join("logs"),
            "data_root",
        ),
        (
            temporary.path().join("data"),
            temporary.path().join("CON/profile"),
            temporary.path().join("logs"),
            "cache_root",
        ),
        (
            temporary.path().join("data"),
            temporary.path().join("cache"),
            temporary.path().join("LPT1.txt/profile"),
            "log_dir",
        ),
    ] {
        assert_eq!(
            DesktopRuntimeProfileConfig::new("019d-safe-profile", data_root, cache_root, log_dir,),
            Err(DesktopRuntimeProfileConfigError::InvalidRoot(expected_root,))
        );
    }
}

#[cfg(windows)]
#[test]
fn explicit_profile_accepts_only_filesystem_windows_prefixes() {
    let temporary = tempfile::tempdir().unwrap();
    let safe_cache = temporary.path().join("cache");
    let safe_logs = temporary.path().join("logs");

    for allowed in [
        PathBuf::from(r"C:\profiles\alpha"),
        PathBuf::from(r"\\server\share\profiles\alpha"),
        PathBuf::from(r"\\?\C:\profiles\alpha"),
        PathBuf::from(r"\\?\UNC\server\share\profiles\alpha"),
    ] {
        assert!(
            DesktopRuntimeProfileConfig::new(
                "019d-safe-profile",
                allowed.clone(),
                safe_cache.clone(),
                safe_logs.clone(),
            )
            .is_ok(),
            "filesystem prefix must be accepted: {}",
            allowed.display()
        );
    }

    for rejected in [
        PathBuf::from(r"\\.\PhysicalDrive0"),
        PathBuf::from(r"\\?\GLOBALROOT\Device\HarddiskVolume1"),
    ] {
        assert_eq!(
            DesktopRuntimeProfileConfig::new(
                "019d-safe-profile",
                rejected.clone(),
                safe_cache.clone(),
                safe_logs.clone(),
            ),
            Err(DesktopRuntimeProfileConfigError::InvalidRoot("data_root")),
            "non-filesystem prefix must be rejected: {}",
            rejected.display()
        );
    }
}

#[cfg(windows)]
#[test]
fn explicit_profile_rejects_superscript_windows_device_names() {
    let temporary = tempfile::tempdir().unwrap();

    for reserved in ["COM¹", "COM².txt", "COM³", "LPT¹", "LPT².log", "LPT³"] {
        let root = temporary.path().join(reserved).join("profile");
        assert_eq!(
            DesktopRuntimeProfileConfig::new(
                "019d-safe-profile",
                root,
                temporary.path().join("cache"),
                temporary.path().join("logs"),
            ),
            Err(DesktopRuntimeProfileConfigError::InvalidRoot("data_root")),
            "reserved Windows device name must be rejected: {reserved}"
        );
    }
}
