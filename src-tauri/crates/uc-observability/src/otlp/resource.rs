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
        KeyValue::new(
            "deployment.environment.name",
            if cfg!(debug_assertions) { "development" } else { "production" },
        ),
    ];
    let resolved = device_id
        .map(|s| s.to_string())
        .or_else(|| crate::context::global_device_id().map(|s| s.to_string()));
    if let Some(did) = resolved {
        // TODO: use semconv::SERVICE_INSTANCE_ID when `semconv_experimental` feature is stable in 0.31.x
        kvs.push(KeyValue::new("service.instance.id", did));
    }
    Resource::builder().with_attributes(kvs).build()
}
