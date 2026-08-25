use std::path::PathBuf;

use uc_app_paths::{DesktopRuntimeProfileConfig, DesktopRuntimeProfileConfigError};

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn explicit_profile_roots_are_isolated_from_each_other_and_ambient_profile() {
    let _env = ENV_LOCK.lock().unwrap();
    let previous = std::env::var("UC_PROFILE").ok();
    std::env::set_var("UC_PROFILE", "ambient-must-not-leak");

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

    match previous {
        Some(value) => std::env::set_var("UC_PROFILE", value),
        None => std::env::remove_var("UC_PROFILE"),
    }

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
