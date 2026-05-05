use opentelemetry::KeyValue;
use opentelemetry_sdk::Resource;
use opentelemetry_semantic_conventions::resource as semconv;

pub fn build_resource(device_id: Option<&str>) -> Resource {
    let mut kvs: Vec<KeyValue> = vec![
        KeyValue::new(semconv::SERVICE_NAME, "uniclipboard-desktop"),
        KeyValue::new(semconv::SERVICE_VERSION, env!("CARGO_PKG_VERSION")),
        // TODO: use semconv::OS_TYPE when `semconv_experimental` feature is stable in 0.31.x
        KeyValue::new("os.type", std::env::consts::OS),
        // TODO: semconv const when stabilized in opentelemetry-semantic-conventions 0.31
        //
        // 优先读编译期 `APP_ENV`(CI 在 build.yml/alpha-build.yml 注入,与后端
        // Sentry tracing.rs 共用同一个变量来源),保证 channel 区分:
        //   - stable release → "production"
        //   - alpha/beta/rc release → 同名 channel
        // CI 没注入(本地 cargo run / cargo test)时退回旧行为:debug build 标
        // "development",release build 标 "production"。
        KeyValue::new(
            "deployment.environment.name",
            option_env!("APP_ENV")
                .filter(|s| !s.is_empty())
                .unwrap_or(if cfg!(debug_assertions) {
                    "development"
                } else {
                    "production"
                }),
        ),
    ];
    let resolved = device_id
        .map(|s| s.to_string())
        .or_else(|| crate::context::global_device_id().map(|s| s.to_string()));
    if let Some(did) = resolved {
        // TODO: use semconv::SERVICE_INSTANCE_ID when `semconv_experimental` feature is stable in 0.31.x
        kvs.push(KeyValue::new("service.instance.id", did.clone()));
        kvs.push(KeyValue::new("device_id", did));
    }
    Resource::builder().with_attributes(kvs).build()
}
