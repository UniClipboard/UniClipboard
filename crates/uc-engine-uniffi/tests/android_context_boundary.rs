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
fn android_binding_installs_the_jni_context_before_engine_start() {
    let root = workspace_root();
    let manifest = read(root.join("crates/uc-engine-uniffi/Cargo.toml"));
    let library = read(root.join("crates/uc-engine-uniffi/src/lib.rs"));
    let android = read(root.join("crates/uc-engine-uniffi/src/android.rs"));

    assert!(manifest.contains("ndk-context"));
    assert!(manifest.contains("jni"));
    assert!(library.contains("#[cfg(target_os = \"android\")]\nmod android;"));
    assert!(android.contains("ndk_context::initialize_android_context"));
    assert!(android.contains("static ANDROID_CONTEXT: OnceLock<GlobalRef>"));
    assert!(
        android.contains("Java_expo_modules_ucengine_UcEngineModule_nativeInstallAndroidContext")
    );
}
