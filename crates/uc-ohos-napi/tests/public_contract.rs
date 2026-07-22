use uc_ohos_napi::core_version;

#[test]
fn core_version_uses_the_binding_package_version() {
    assert_eq!(
        core_version(),
        format!("core-v{}", env!("CARGO_PKG_VERSION"))
    );
}
