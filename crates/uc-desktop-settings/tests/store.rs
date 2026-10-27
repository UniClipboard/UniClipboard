//! End-to-end checks of the product settings store against real files.
//!
//! Each case maps to a failure mode in
//! `docs/adr/adr-013-desktop-product-settings.md` (F1–F14, F18).

#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use uc_desktop_settings::{
    DesktopSettingsPatch, DesktopSettingsStore, GeneralSettingsPatch, SettingsSection, TrayIcon,
    WindowSize, FILE_NAME,
};

fn settings_path(dir: &tempfile::TempDir) -> PathBuf {
    dir.path().join(FILE_NAME)
}

fn tray(icon: TrayIcon) -> DesktopSettingsPatch {
    DesktopSettingsPatch {
        general: GeneralSettingsPatch {
            tray_icon: Some(icon),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[tokio::test]
async fn window_size_is_validated_persisted_and_notified_by_the_desktop_store() {
    let dir = tempfile::tempdir().unwrap();
    let path = settings_path(&dir);
    std::fs::write(&path, br#"{"general":{"futurePreference":true}}"#).unwrap();
    let store = DesktopSettingsStore::load(path.clone());
    let mut changes = store.subscribe();
    let patch = |size| DesktopSettingsPatch {
        general: GeneralSettingsPatch {
            main_window_size: Some(size),
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(
        store.snapshot().await.settings.general.main_window_size,
        WindowSize::default()
    );
    let size = WindowSize {
        width: 1234,
        height: 810,
    };
    store.apply(patch(size)).await.unwrap();
    assert_eq!(
        changes.recv().await.unwrap().sections,
        vec![SettingsSection::General]
    );
    assert_eq!(
        read_json(&path)["general"]["mainWindowSize"],
        json!({"width":1234,"height":810})
    );
    assert_eq!(read_json(&path)["general"]["futurePreference"], true);
    let bytes = std::fs::read(&path).unwrap();
    for invalid in [
        WindowSize {
            width: 0,
            height: 810,
        },
        WindowSize {
            width: 1234,
            height: u32::MAX,
        },
    ] {
        assert!(store.apply(patch(invalid)).await.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
    assert!(!store.apply(patch(size)).await.unwrap().changed);
    assert!(changes.try_recv().is_err());
    let restarted = DesktopSettingsStore::load(path);
    assert_eq!(
        restarted.snapshot().await.settings.general.main_window_size,
        size
    );
}

#[tokio::test]
async fn invalid_window_size_defaults_without_rewriting_other_preferences() {
    for invalid in [
        json!({"width":0,"height":810}),
        json!({"width":1200}),
        json!({"width":-1,"height":810}),
        json!({"width":1200.5,"height":810}),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = settings_path(&dir);
        let raw = json!({"general":{"mainWindowSize":invalid,"trayIcon":"cat"}});
        std::fs::write(&path, serde_json::to_vec(&raw).unwrap()).unwrap();
        let store = DesktopSettingsStore::load(path.clone());
        let general = store.snapshot().await.settings.general;
        assert_eq!(general.main_window_size, WindowSize::default());
        assert_eq!(general.tray_icon, TrayIcon::Cat);
        assert_eq!(read_json(&path), raw);
    }
}

// F1
#[tokio::test]
async fn missing_file_yields_defaults_and_is_not_created_on_load() {
    let dir = tempfile::tempdir().unwrap();
    let path = settings_path(&dir);

    let store = DesktopSettingsStore::load(path.clone());
    let snapshot = store.snapshot().await;

    assert_eq!(snapshot.settings.general.tray_icon, TrayIcon::Glyph);
    assert_eq!(snapshot.revision, 0);
    assert!(!path.exists());
}

// F2, F3, F4
#[tokio::test]
async fn unusable_files_yield_defaults_and_stay_untouched_until_an_explicit_write() {
    let mut oversized = br#"{"general":{"trayIcon":"cat"},"pad":""#.to_vec();
    oversized.extend(std::iter::repeat_n(b'x', 1024 * 1024));
    oversized.extend(br#""}"#);
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("corrupt", b"{\"general\": {\"trayIcon\": \"cat\"".to_vec()),
        ("array", b"[\"cat\"]".to_vec()),
        ("scalar", b"42".to_vec()),
        ("oversized", oversized),
    ];
    for (name, bytes) in cases {
        let dir = tempfile::tempdir().unwrap();
        let path = settings_path(&dir);
        std::fs::write(&path, &bytes).unwrap();

        let store = DesktopSettingsStore::load(path.clone());
        assert_eq!(
            store.snapshot().await.settings.general.tray_icon,
            TrayIcon::Glyph,
            "{name}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "{name}: load rewrote");

        let result = store.apply(tray(TrayIcon::Cat)).await.unwrap();
        assert!(result.changed, "{name}");
        let written = read_json(&path);
        assert_eq!(written["general"]["trayIcon"], "cat", "{name}");
        assert_eq!(written["version"], 1, "{name}");
    }
}

// F5, F7, F8, F14
#[tokio::test]
async fn invalid_and_unknown_values_survive_writes_to_other_paths() {
    let dir = tempfile::tempdir().unwrap();
    let path = settings_path(&dir);
    let original = json!({
        "version": 7,
        "import": { "fieldSet": 3, "sourceFingerprint": "abc", "futureKey": [1, 2] },
        "general": { "trayIcon": "neon", "futureGeneral": { "nested": true } },
        "futureSection": { "theme": "unknown-enum-value" }
    });
    std::fs::write(&path, serde_json::to_vec(&original).unwrap()).unwrap();
    let before = std::fs::read(&path).unwrap();

    let store = DesktopSettingsStore::load(path.clone());
    assert_eq!(
        store.snapshot().await.settings.general.tray_icon,
        TrayIcon::Glyph
    );
    assert_eq!(std::fs::read(&path).unwrap(), before, "load rewrote");

    store.apply(tray(TrayIcon::Cat)).await.unwrap();

    let mut expected = original.clone();
    expected["general"]["trayIcon"] = json!("cat");
    expected["import"]["userWritten"] = json!(["general.trayIcon"]);
    assert_eq!(read_json(&path), expected);
}

// F5: a wrong type is invalid like an unknown enum value.
#[tokio::test]
async fn wrong_value_type_yields_the_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = settings_path(&dir);
    std::fs::write(&path, br#"{"version":1,"general":{"trayIcon":7}}"#).unwrap();

    let store = DesktopSettingsStore::load(path.clone());

    assert_eq!(
        store.snapshot().await.settings.general.tray_icon,
        TrayIcon::Glyph
    );
}

// F6
#[tokio::test]
async fn non_object_containers_are_replaced_only_by_an_explicit_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = settings_path(&dir);
    std::fs::write(
        &path,
        br#"{"version":1,"general":5,"import":"x","keep":true}"#,
    )
    .unwrap();

    let store = DesktopSettingsStore::load(path.clone());
    assert_eq!(
        store.snapshot().await.settings.general.tray_icon,
        TrayIcon::Glyph
    );

    store.apply(tray(TrayIcon::Cat)).await.unwrap();

    assert_eq!(
        read_json(&path),
        json!({
            "version": 1,
            "general": { "trayIcon": "cat" },
            "import": { "userWritten": ["general.trayIcon"] },
            "keep": true
        })
    );
}

// F11, F13
#[tokio::test]
async fn explicit_write_persists_across_restart_and_same_value_is_a_no_op() {
    let dir = tempfile::tempdir().unwrap();
    let path = settings_path(&dir);

    let store = DesktopSettingsStore::load(path.clone());
    let mut changes = store.subscribe();

    let first = store.apply(tray(TrayIcon::Cat)).await.unwrap();
    assert!(first.changed);
    assert_eq!(first.snapshot.revision, 1);
    assert_eq!(first.snapshot.settings.general.tray_icon, TrayIcon::Cat);
    let change = changes.try_recv().unwrap();
    assert_eq!(change.revision, 1);
    assert_eq!(change.sections, vec![SettingsSection::General]);

    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let same = store.apply(tray(TrayIcon::Cat)).await.unwrap();
    assert!(!same.changed);
    assert_eq!(same.snapshot.revision, 1);
    assert!(changes.try_recv().is_err(), "same-value write notified");
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        modified
    );

    drop(store);
    let restarted = DesktopSettingsStore::load(path.clone());
    let snapshot = restarted.snapshot().await;
    assert_eq!(snapshot.settings.general.tray_icon, TrayIcon::Cat);
    assert_eq!(snapshot.revision, 0);
}

// F11: the default value written explicitly is still recorded as a user write.
#[tokio::test]
async fn first_explicit_write_of_the_default_records_the_user_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = settings_path(&dir);

    let store = DesktopSettingsStore::load(path.clone());
    let result = store.apply(tray(TrayIcon::Glyph)).await.unwrap();

    assert!(result.changed);
    assert_eq!(result.snapshot.revision, 1);
    assert_eq!(
        read_json(&path),
        json!({
            "version": 1,
            "general": { "trayIcon": "glyph" },
            "import": { "userWritten": ["general.trayIcon"] }
        })
    );
}

// F11: an empty patch changes nothing.
#[tokio::test]
async fn empty_patch_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = settings_path(&dir);

    let store = DesktopSettingsStore::load(path.clone());
    let result = store.apply(DesktopSettingsPatch::default()).await.unwrap();

    assert!(!result.changed);
    assert_eq!(result.snapshot.revision, 0);
    assert!(!path.exists());
}

// F9
#[cfg(unix)]
#[tokio::test]
async fn failed_write_keeps_memory_revision_and_file() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let path = settings_path(&dir);
    std::fs::write(
        &path,
        br#"{"version":1,"general":{"trayIcon":"glyph"},"keep":1}"#,
    )
    .unwrap();
    let before = std::fs::read(&path).unwrap();

    let store = DesktopSettingsStore::load(path.clone());
    let mut changes = store.subscribe();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();

    let result = store.apply(tray(TrayIcon::Cat)).await;

    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(result.is_err());
    let snapshot = store.snapshot().await;
    assert_eq!(snapshot.settings.general.tray_icon, TrayIcon::Glyph);
    assert_eq!(snapshot.revision, 0);
    assert!(changes.try_recv().is_err(), "failed write notified");
    assert_eq!(std::fs::read(&path).unwrap(), before);

    // The store keeps working once the directory is writable again.
    let retry = store.apply(tray(TrayIcon::Cat)).await.unwrap();
    assert!(retry.changed);
    assert_eq!(retry.snapshot.revision, 1);
    assert_eq!(changes.try_recv().unwrap().revision, 1);
}

// F12
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_writes_are_serialized_and_counted_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let path = settings_path(&dir);
    std::fs::write(
        &path,
        br#"{"version":1,"general":{"futureGeneral":"keep"},"futureSection":{"x":1}}"#,
    )
    .unwrap();

    let store = std::sync::Arc::new(DesktopSettingsStore::load(path.clone()));
    let mut changes = store.subscribe();

    let mut tasks = Vec::new();
    for index in 0..32 {
        let store = std::sync::Arc::clone(&store);
        let icon = if index % 3 == 0 {
            TrayIcon::Glyph
        } else {
            TrayIcon::Cat
        };
        tasks.push(tokio::spawn(async move {
            store.apply(tray(icon)).await.unwrap()
        }));
    }
    let mut changed = 0;
    for task in tasks {
        if task.await.unwrap().changed {
            changed += 1;
        }
    }

    let snapshot = store.snapshot().await;
    assert_eq!(snapshot.revision, changed);
    let mut seen = Vec::new();
    while let Ok(change) = changes.try_recv() {
        seen.push(change.revision);
    }
    assert_eq!(seen, (1..=changed).collect::<Vec<_>>());

    let written = read_json(&path);
    let on_disk = match written["general"]["trayIcon"].as_str() {
        Some("cat") => TrayIcon::Cat,
        Some("glyph") => TrayIcon::Glyph,
        other => panic!("unexpected trayIcon {other:?}"),
    };
    assert_eq!(on_disk, snapshot.settings.general.tray_icon);
    assert_eq!(written["general"]["futureGeneral"], "keep");
    assert_eq!(written["futureSection"], json!({ "x": 1 }));
    assert_eq!(
        written["import"]["userWritten"],
        json!(["general.trayIcon"])
    );

    let reloaded = DesktopSettingsStore::load(path).snapshot().await;
    assert_eq!(reloaded.settings, snapshot.settings);
}

// F18
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cancelled_caller_cannot_split_the_file_from_memory() {
    let dir = tempfile::tempdir().unwrap();
    let path = settings_path(&dir);
    let store = DesktopSettingsStore::load(path.clone());
    let mut changes = store.subscribe();

    // The caller is dropped after the first poll, while the write is in flight.
    let _ = tokio::time::timeout(std::time::Duration::ZERO, store.apply(tray(TrayIcon::Cat))).await;

    let mut snapshot = store.snapshot().await;
    for _ in 0..200 {
        if snapshot.revision > 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        snapshot = store.snapshot().await;
    }
    let on_disk = std::fs::read(&path)
        .ok()
        .map(|bytes| serde_json::from_slice::<Value>(&bytes).unwrap());
    match on_disk {
        Some(document) => {
            assert_eq!(document["general"]["trayIcon"], "cat");
            assert_eq!(snapshot.settings.general.tray_icon, TrayIcon::Cat);
            assert_eq!(snapshot.revision, 1);
            assert_eq!(changes.try_recv().unwrap().revision, 1);
        }
        None => {
            assert_eq!(snapshot.revision, 0);
            assert!(changes.try_recv().is_err());
        }
    }
}

#[tokio::test]
async fn legacy_startup_modes_preserve_background_preference_independently() {
    use uc_desktop_settings::StartupMode;
    for (legacy, expected_mode, expected_background) in [
        ("normal", StartupMode::Normal, false),
        ("silent", StartupMode::Normal, true),
        ("lightweight", StartupMode::Lightweight, true),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = settings_path(&dir);
        let original = json!({"general": {"startupMode": legacy, "futurePreference": 42}});
        std::fs::write(&path, serde_json::to_vec(&original).unwrap()).unwrap();
        let store = DesktopSettingsStore::load(path.clone());
        let general = store.snapshot().await.settings.general;
        assert_eq!(general.startup_mode, expected_mode);
        assert_eq!(general.background_startup, expected_background);
        assert_eq!(
            read_json(&path),
            original,
            "reads must not rewrite legacy files"
        );
        store
            .apply(DesktopSettingsPatch {
                general: GeneralSettingsPatch {
                    startup_mode: Some(StartupMode::Lightweight),
                    ..Default::default()
                },
                ..Default::default()
            })
            .await
            .unwrap();
        let reloaded = DesktopSettingsStore::load(path.clone());
        assert_eq!(
            reloaded
                .snapshot()
                .await
                .settings
                .general
                .background_startup,
            expected_background
        );
        store
            .apply(DesktopSettingsPatch {
                general: GeneralSettingsPatch {
                    background_startup: Some(false),
                    ..Default::default()
                },
                ..Default::default()
            })
            .await
            .unwrap();
        let general = DesktopSettingsStore::load(path.clone())
            .snapshot()
            .await
            .settings
            .general;
        assert_eq!(general.startup_mode, StartupMode::Lightweight);
        assert!(!general.background_startup);
        assert_eq!(read_json(&path)["general"]["futurePreference"], 42);
    }
}

#[tokio::test]
async fn legacy_silent_patch_maps_to_normal_with_independent_background_preference() {
    use uc_desktop_settings::StartupMode;
    let dir = tempfile::tempdir().unwrap();
    let store = DesktopSettingsStore::load(settings_path(&dir));
    for explicit in [None, Some(false), Some(true)] {
        store
            .apply(DesktopSettingsPatch {
                general: GeneralSettingsPatch {
                    startup_mode: Some(StartupMode::Silent),
                    background_startup: explicit,
                    ..Default::default()
                },
                ..Default::default()
            })
            .await
            .unwrap();
        let general = store.snapshot().await.settings.general;
        assert_eq!(general.startup_mode, StartupMode::Normal);
        assert_eq!(general.background_startup, explicit.unwrap_or(true));
    }
}

#[tokio::test]
async fn explicit_and_invalid_background_values_do_not_infer_legacy_silent() {
    for value in [json!(false), json!("invalid"), serde_json::Value::Null] {
        let dir = tempfile::tempdir().unwrap();
        let path = settings_path(&dir);
        let original = json!({"general": {"startupMode": "silent", "backgroundStartup": value}});
        std::fs::write(&path, serde_json::to_vec(&original).unwrap()).unwrap();
        let store = DesktopSettingsStore::load(path.clone());
        assert!(!store.snapshot().await.settings.general.background_startup);
        assert_eq!(read_json(&path), original);
    }
}
