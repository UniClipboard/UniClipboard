use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(relative: &str) -> String {
    let path = workspace_root().join(relative);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
}

#[test]
fn ohos_probe_has_a_stage_application_project() {
    for relative in [
        "apps/ohos-probe/build-profile.json5",
        "apps/ohos-probe/oh-package.json5",
        "apps/ohos-probe/hvigorfile.ts",
        "apps/ohos-probe/entry/build-profile.json5",
        "apps/ohos-probe/entry/oh-package.json5",
        "apps/ohos-probe/entry/hvigorfile.ts",
        "apps/ohos-probe/entry/src/main/module.json5",
    ] {
        assert!(
            workspace_root().join(relative).is_file(),
            "missing HarmonyOS project file: {relative}"
        );
    }
}

#[test]
fn ohos_probe_declares_the_engine_napi_module() {
    let package = read("apps/ohos-probe/entry/oh-package.json5");
    assert!(package.contains("libuc_ohos_napi.so"));
    assert!(package.contains("src/main/cpp/types/libuc_ohos_napi"));

    let declarations = read("apps/ohos-probe/entry/src/main/cpp/types/libuc_ohos_napi/index.d.ts");
    assert!(declarations.contains("coreVersion(): string"));
}

#[test]
fn ohos_probe_builds_the_arm64_binding_before_assembling_the_hap() {
    let script = read("apps/ohos-probe/build-emulator.sh");
    assert!(script.contains("aarch64-unknown-linux-ohos"));
    assert!(script.contains("libuc_ohos_napi.so"));
    assert!(script.contains("assembleHap"));
    assert!(!script.contains("/Users/"));
}

#[test]
fn ohos_probe_page_loads_the_engine_version_from_napi() {
    let page = read("apps/ohos-probe/entry/src/main/ets/pages/Index.ets");
    assert!(page.contains("import engine from 'libuc_ohos_napi.so'"));
    assert!(page.contains("engine.coreVersion()"));
    assert!(page.contains("JSON.stringify(error)"));
    assert!(page.contains("this.details = String(error)"));
}

#[test]
fn ohos_probe_signs_with_sdk_test_material_and_verifies_the_hap() {
    let script = read("apps/ohos-probe/sign-emulator.sh");
    assert!(script.contains("hap-sign-tool.jar"));
    assert!(script.contains("UnsgnedReleasedProfileTemplate.json"));
    assert!(script.contains("verify-app"));
    assert!(!script.contains("/Users/"));
}
