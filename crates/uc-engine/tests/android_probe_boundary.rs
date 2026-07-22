use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root should exist")
}

fn read(path: impl AsRef<Path>) -> String {
    let path = path.as_ref();
    fs::read_to_string(path).unwrap_or_else(|error| {
        panic!("failed to read {}: {error}", path.display());
    })
}

#[test]
fn ios_and_android_share_one_probe_core() {
    let root = workspace_root();
    let workspace = read(root.join("Cargo.toml"));
    let manifest = read(root.join("apps/mobile-probe-core/Cargo.toml"));

    assert!(workspace.contains("\"apps/mobile-probe-core\""));
    assert!(!workspace.contains("\"apps/ios-probe-core\""));
    assert!(manifest.contains("name = \"uc-mobile-probe-core\""));
    assert!(manifest.contains("crate-type = [\"lib\", \"staticlib\", \"cdylib\"]"));
}

#[test]
fn shared_probe_selects_each_platform_secure_storage() {
    let root = workspace_root();
    let source = read(root.join("apps/mobile-probe-core/src/lib.rs"));

    assert!(source.contains(
        "#[cfg(target_vendor = \"apple\")]\nfn host_secure_storage() -> Box<dyn HostSecureStorage> {\n    Box::new(KeychainStorage)\n}"
    ));
    assert!(source.contains(
        "#[cfg(not(any(target_vendor = \"apple\", target_os = \"android\")))]\nfn host_secure_storage() -> Box<dyn HostSecureStorage> {\n    Box::new(UnavailableSecureStorage)\n}"
    ));
}

#[test]
fn android_probe_uses_android_keystore_for_persisted_secrets() {
    let root = workspace_root();
    let bridge = read(root.join(
        "apps/android-probe/app/src/main/java/app/uniclipboard/engineprobe/ProbeBridge.java",
    ));

    assert!(bridge.contains("AndroidKeyStore"));
    assert!(bridge.contains("AES/GCM/NoPadding"));
    assert!(bridge.contains("KeyGenParameterSpec"));
    assert!(bridge.contains("nativeInstallHost(this, context.getApplicationContext())"));
    assert!(!bridge.contains("putString(key, new String(value"));
}

#[test]
fn android_shell_only_forwards_commands_to_the_shared_probe() {
    let root = workspace_root();
    let receiver = read(root.join(
        "apps/android-probe/app/src/main/java/app/uniclipboard/engineprobe/ProbeReceiver.java",
    ));
    let android_bridge = read(root.join("apps/mobile-probe-core/src/android.rs"));

    assert!(receiver.contains("bridge.command(command)"));
    assert!(android_bridge.contains("crate::probe_command"));
    assert!(android_bridge.contains("ndk_context::initialize_android_context"));
    assert!(android_bridge.contains("static ANDROID_CONTEXT: OnceLock<GlobalRef>"));
    assert!(!receiver.contains("createSpace"));
    assert!(!receiver.contains("sendText"));
}

#[test]
fn android_pairing_keeps_the_probe_alive_with_a_data_sync_service() {
    let root = workspace_root();
    let manifest = read(root.join("apps/android-probe/app/src/main/AndroidManifest.xml"));
    let activity = read(root.join(
        "apps/android-probe/app/src/main/java/app/uniclipboard/engineprobe/ProbeActivity.java",
    ));
    let receiver = read(root.join(
        "apps/android-probe/app/src/main/java/app/uniclipboard/engineprobe/ProbeReceiver.java",
    ));
    let service = read(root.join(
        "apps/android-probe/app/src/main/java/app/uniclipboard/engineprobe/ProbeService.java",
    ));

    assert!(manifest.contains("android:foregroundServiceType=\"dataSync\""));
    assert!(manifest.contains("android:name=\".ProbeActivity\""));
    assert!(activity.contains("startForegroundService"));
    assert!(!receiver.contains("startForegroundService"));
    assert!(service.contains("startForeground"));
    assert!(!service.contains("ProbeBridge"));
}
