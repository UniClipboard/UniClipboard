use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use cargo_metadata::{CrateType, DependencyKind, MetadataCommand, TargetKind};

#[test]
fn uniffi_binding_is_a_workspace_member_with_a_public_engine_boundary() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let metadata = MetadataCommand::new()
        .manifest_path(workspace_root.join("Cargo.toml"))
        .no_deps()
        .exec()
        .expect("workspace metadata must be readable");
    let package = metadata
        .packages
        .iter()
        .find(|package| package.name == "uc-engine-uniffi")
        .expect("uc-engine-uniffi must be a workspace member");

    let library = package
        .targets
        .iter()
        .find(|target| target.name == "uc_engine_uniffi")
        .expect("uc-engine-uniffi must expose a library target");
    let crate_types = library.crate_types.iter().cloned().collect::<BTreeSet<_>>();
    assert_eq!(
        crate_types,
        BTreeSet::from([CrateType::CDyLib, CrateType::Lib, CrateType::StaticLib,])
    );

    let bindgen = package
        .targets
        .iter()
        .find(|target| target.name == "uc-engine-uniffi-bindgen")
        .expect("uc-engine-uniffi must expose its own bindgen target");
    assert!(bindgen.kind.contains(&TargetKind::Bin));
    assert_eq!(bindgen.required_features, ["bindgen-cli"]);

    let dependencies = package
        .dependencies
        .iter()
        .filter(|dependency| dependency.kind == DependencyKind::Normal)
        .map(|dependency| dependency.name.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        dependencies,
        BTreeSet::from([
            "jni",
            "ndk-context",
            "thiserror",
            "tokio",
            "uc-engine",
            "uniffi",
            "zeroize",
        ])
    );
    for forbidden in ["uc-core", "uc-application", "uc-infra", "uc-bootstrap"] {
        assert!(
            !dependencies.contains(forbidden),
            "binding must not depend directly on {forbidden}"
        );
    }
}

#[test]
fn uniffi_binding_owns_runnable_ios_and_android_packaging() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let binding_root = workspace_root.join("crates/uc-engine-uniffi");
    let ios = fs::read_to_string(binding_root.join("scripts/build-ios-xcframework.sh"))
        .expect("unified binding must own an iOS packaging script");
    let android = fs::read_to_string(binding_root.join("scripts/build-android-aar.sh"))
        .expect("unified binding must own an Android packaging script");
    let gradle = fs::read_to_string(binding_root.join("android/build.gradle"))
        .expect("unified binding must own an Android library project");
    let manifest = fs::read_to_string(binding_root.join("android/src/main/AndroidManifest.xml"))
        .expect("unified binding Android library must declare a manifest");

    for required in [
        "cargo build -p uc-engine-uniffi",
        "aarch64-apple-ios",
        "aarch64-apple-ios-sim",
        "x86_64-apple-ios",
        "--language swift",
        "CARGO_PROFILE_RELEASE_DEBUG",
        "UC_ENGINE_UNIFFI_IOS_DEPLOYMENT_TARGET:-16.4",
        "export IPHONEOS_DEPLOYMENT_TARGET",
        "selective_strip_archive",
        "xcodebuild -create-xcframework",
        "UniClipboardEngine.xcframework.zip",
        "UniClipboardEngine.checksum.txt",
    ] {
        assert!(ios.contains(required), "iOS packaging missing {required}");
    }
    assert!(!ios.contains("xcrun strip -S \"$DEVICE_DIR"));
    for required in [
        "cargo ndk",
        "arm64-v8a",
        "x86_64",
        "--language kotlin",
        "assembleRelease",
        "UniClipboardEngine.aar",
        "UniClipboardEngine.checksum.txt",
    ] {
        assert!(
            android.contains(required),
            "Android packaging missing {required}"
        );
    }
    assert!(gradle.contains("com.android.library"));
    assert!(gradle.contains("org.jetbrains.kotlin.android"));
    assert!(gradle.contains("net.java.dev.jna:jna:5.14.0@aar"));
    assert!(gradle.contains("namespace \"app.uniclipboard.engine\""));
    assert!(manifest.contains("android.permission.INTERNET"));
}
