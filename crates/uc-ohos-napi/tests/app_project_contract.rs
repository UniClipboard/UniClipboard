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
        "apps/ohos-probe/engine/build-profile.json5",
        "apps/ohos-probe/engine/Index.ets",
        "apps/ohos-probe/engine/oh-package.json5",
        "apps/ohos-probe/engine/hvigorfile.ts",
        "apps/ohos-probe/engine/src/main/module.json5",
        "crates/uc-ohos-napi/ohos/index.d.ts",
    ] {
        assert!(
            workspace_root().join(relative).is_file(),
            "missing HarmonyOS project file: {relative}"
        );
    }
}

#[test]
fn ohos_probe_declares_network_access_for_engine_startup() {
    let module = read("apps/ohos-probe/entry/src/main/module.json5");

    assert!(module.contains("ohos.permission.INTERNET"));
}

#[test]
fn ohos_probe_declares_the_engine_napi_module() {
    let package = read("apps/ohos-probe/entry/oh-package.json5");
    assert!(package.contains("libuc_ohos_napi.so"));
    assert!(package.contains("src/main/cpp/types/libuc_ohos_napi"));

    let declarations = read("crates/uc-ohos-napi/ohos/index.d.ts");
    assert!(declarations.contains("coreVersion(): string"));
}

#[test]
fn ohos_probe_builds_the_arm64_binding_before_assembling_the_hap() {
    let script = read("apps/ohos-probe/build-emulator.sh");
    assert!(script.contains("aarch64-unknown-linux-ohos"));
    assert!(script.contains("libuc_ohos_napi.so"));
    assert!(script.contains("assembleHap"));
    assert!(script.contains("assembleHar"));
    assert!(script.contains("UniClipboardEngine.har"));
    assert!(!script.contains("/Users/"));
}

#[test]
fn ohos_binding_owns_a_distributable_har_module() {
    let entry = read("apps/ohos-probe/engine/Index.ets");
    let module = read("apps/ohos-probe/engine/src/main/module.json5");
    let package = read("apps/ohos-probe/engine/oh-package.json5");
    let script = read("apps/ohos-probe/build-emulator.sh");

    assert!(entry.contains("import engine from 'libuc_ohos_napi.so'"));
    assert!(entry.contains("export default engine"));
    assert!(module.contains("\"type\": \"har\""));
    assert!(package.contains("@uniclipboard/engine"));
    assert!(package.contains("libuc_ohos_napi.so"));
    assert!(script.contains("crates/uc-ohos-napi/ohos/index.d.ts"));
    assert!(script.contains("UniClipboardEngine.har.checksum.txt"));
}

#[test]
fn ohos_probe_page_loads_the_engine_version_from_napi() {
    let runtime = read("apps/ohos-probe/entry/src/main/ets/host/EngineRuntime.ets");
    assert!(runtime.contains("import engine from 'libuc_ohos_napi.so'"));
    assert!(runtime.contains("engine.coreVersion()"));
    assert!(runtime.contains("JSON.stringify(error)"));
    assert!(runtime.contains("this.details = String(error)"));
}

#[test]
fn ohos_probe_uses_asset_store_without_a_plaintext_fallback() {
    let storage = read("apps/ohos-probe/entry/src/main/ets/host/SecureAssetStorage.ets");

    assert!(storage.contains("@kit.AssetStoreKit"));
    assert!(storage.contains("asset.addSync"));
    assert!(storage.contains("asset.querySync"));
    assert!(storage.contains("asset.updateSync"));
    assert!(storage.contains("asset.removeSync"));
    assert!(!storage.contains("preferences"));
    assert!(!storage.contains("fs."));
}

#[test]
fn ohos_probe_normalizes_napi_secret_bytes_for_asset_store() {
    let storage = read("apps/ohos-probe/entry/src/main/ets/host/SecureAssetStorage.ets");

    assert!(storage.contains("const secret = new Uint8Array(value)"));
    assert!(storage.contains("[asset.Tag.SECRET, secret]"));
    assert!(!storage.contains("[asset.Tag.SECRET, value]"));
}

#[test]
fn ohos_probe_keeps_authorized_file_uris_behind_memory_handles() {
    let registry = read("apps/ohos-probe/entry/src/main/ets/host/FileHandleRegistry.ets");
    let runtime = read("apps/ohos-probe/entry/src/main/ets/host/EngineRuntime.ets");

    assert!(registry.contains("Map<string, FileEntry>"));
    assert!(registry.contains("registerOutput(uri: string)"));
    assert!(registry.contains("fileReadChunk"));
    assert!(registry.contains("fileWriteChunk"));
    assert!(registry.contains("fileFinishWrite"));
    assert!(!registry.contains("preferences"));
    assert!(runtime.contains("DocumentViewPicker"));
    assert!(runtime.contains(".save({"));
    assert!(runtime.contains("registerOutput"));
    assert!(runtime.contains(".exportEntry("));
}

#[test]
fn ohos_probe_normalizes_napi_file_bytes_for_system_writes() {
    let registry = read("apps/ohos-probe/entry/src/main/ets/host/FileHandleRegistry.ets");

    assert!(registry.contains("const copy = new Uint8Array(bytes)"));
    assert!(!registry.contains("const copy = bytes.slice()"));
}

#[test]
fn ohos_probe_reads_the_exported_file_back_before_reporting_success() {
    let runtime = read("apps/ohos-probe/entry/src/main/ets/host/EngineRuntime.ets");

    assert!(runtime.contains("const exportId = Date.now()"));
    assert!(
        runtime.contains("const exportText = `UniClipboard HarmonyOS export check ${exportId}`")
    );
    assert!(runtime.contains("uniclipboard-export-check-${exportId}.txt"));
    assert!(runtime.contains("this.files.fileReadChunk(handle, '0'"));
    assert!(runtime.contains("OHOS_EXPORT_VERIFY_FAILED"));
    assert!(runtime.contains("this.details = `Export failed: ${String(error)}`"));
}

#[test]
fn ohos_probe_scans_private_directories_for_export_plaintext() {
    let runtime = read("apps/ohos-probe/entry/src/main/ets/host/EngineRuntime.ets");
    let probe = read("apps/ohos-probe/entry/src/main/ets/host/PrivateStorageProbe.ets");

    assert!(runtime.contains("PrivateStorageProbe"));
    assert!(runtime.contains("context.filesDir"));
    assert!(runtime.contains("context.cacheDir"));
    assert!(runtime.contains("context.tempDir"));
    assert!(probe.contains("listFileSync"));
    assert!(probe.contains("OHOS_PRIVATE_STORAGE_PLAINTEXT"));
}

#[test]
fn ohos_probe_starts_the_engine_with_real_host_capabilities() {
    let declarations = read("crates/uc-ohos-napi/ohos/index.d.ts");
    let runtime = read("apps/ohos-probe/entry/src/main/ets/host/EngineRuntime.ets");

    assert!(declarations.contains("prepareHost(host: OhHost): PreparedHost"));
    assert!(declarations.contains("export type PreparedHost = object"));
    assert!(declarations.contains("Promise<OhSpaceCreated>"));
    assert!(declarations.contains("startEngine("));
    assert!(declarations.contains("recoverSession(allowSecureStorageUnlock: boolean)"));
    assert!(declarations.contains("exportEntry(entryId: string, destinationHandle: string)"));
    assert!(runtime.contains("createEngineHost"));
    assert!(runtime.contains("engine.prepareHost"));
    assert!(runtime.contains("engine.startEngine"));
    assert!(runtime.contains("active.recoverSession(true)"));
}

#[test]
fn ohos_probe_recovers_a_persisted_space_before_creating_one() {
    let runtime = read("apps/ohos-probe/entry/src/main/ets/host/EngineRuntime.ets");

    assert!(runtime.contains("const recovery = await active.recoverSession(true)"));
    assert!(runtime.contains("if (recovery.unlocked && !recovery.resumed)"));
    assert!(runtime.contains("if (recovery.unlocked)"));
    assert!(runtime.contains("await active.createSpace"));
    assert!(!runtime.contains("EXISTING_SPACE_CONFLICT"));
    assert!(!runtime.contains("isExistingSpaceConflict"));
}

#[test]
fn ohos_system_lifecycle_uses_the_single_engine_runtime() {
    let runtime = read("apps/ohos-probe/entry/src/main/ets/host/EngineRuntime.ets");
    let ability = read("apps/ohos-probe/entry/src/main/ets/entryability/EntryAbility.ets");
    let page = read("apps/ohos-probe/entry/src/main/ets/pages/Index.ets");

    assert!(runtime.contains("export const engineRuntime"));
    assert!(runtime.contains("recoverSession(true)"));
    assert!(runtime.contains("lifecycleState()"));
    assert!(runtime.contains("await active.suspend()"));
    assert!(runtime.contains("await active.resume()"));
    assert!(runtime.contains("Lifecycle transition failed"));
    assert!(ability.contains("onBackground(): void"));
    assert!(ability.contains("engineRuntime.onBackground()"));
    assert!(ability.contains("onForeground(): void"));
    assert!(ability.contains("engineRuntime.onForeground()"));
    assert!(page.contains("engineRuntime.start(context)"));
    assert!(!page.contains("private activeEngine?: OhEngine"));
}

#[test]
fn ohos_probe_signs_with_sdk_test_material_and_verifies_the_hap() {
    let script = read("apps/ohos-probe/sign-emulator.sh");
    assert!(script.contains("hap-sign-tool.jar"));
    assert!(script.contains("UnsgnedReleasedProfileTemplate.json"));
    assert!(script.contains("verify-app"));
    assert!(!script.contains("/Users/"));
}
