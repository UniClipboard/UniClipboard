fn main() {
    napi_build::setup();

    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("ohos") {
        println!("cargo:rustc-link-lib=dylib=ace_napi.z");
    }
}
