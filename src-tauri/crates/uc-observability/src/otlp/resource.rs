use opentelemetry::KeyValue;
use opentelemetry_sdk::Resource;
use opentelemetry_semantic_conventions::resource as semconv;

const PROCESS_EXECUTABLE_NAME_ATTR: &str = "process.executable.name";
const PROCESS_PID_ATTR: &str = "process.pid";

pub fn build_resource(device_id: Option<&str>) -> Resource {
    let mut kvs: Vec<KeyValue> = vec![
        KeyValue::new(semconv::SERVICE_NAME, "uniclipboard-desktop"),
        KeyValue::new(semconv::SERVICE_VERSION, env!("CARGO_PKG_VERSION")),
        KeyValue::new(PROCESS_PID_ATTR, std::process::id() as i64),
        // TODO: use semconv::OS_TYPE when `semconv_experimental` feature is stable in 0.31.x
        KeyValue::new("os.type", std::env::consts::OS),
        // TODO: semconv const when stabilized in opentelemetry-semantic-conventions 0.31
        KeyValue::new(
            "deployment.environment.name",
            if cfg!(debug_assertions) {
                "development"
            } else {
                "production"
            },
        ),
    ];
    if let Some(process_name) = current_process_name() {
        kvs.push(KeyValue::new(PROCESS_EXECUTABLE_NAME_ATTR, process_name));
    }
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

fn current_process_name() -> Option<String> {
    std::env::current_exe()
        .ok()?
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use opentelemetry::Value;

    use super::{build_resource, current_process_name};

    #[test]
    fn build_resource_includes_device_id_attribute_when_present() {
        let resource = build_resource(Some("device-xyz"));

        let device_id_value = resource
            .iter()
            .find(|(key, _)| key.as_str() == "device_id")
            .map(|(_, value)| value.as_str().to_string())
            .expect("device_id should be present when device id is supplied");

        assert_eq!(device_id_value, "device-xyz");
    }

    #[test]
    fn build_resource_includes_process_identity_attributes() {
        let resource = build_resource(None);

        let process_name = resource
            .iter()
            .find(|(key, _)| key.as_str() == "process.executable.name")
            .map(|(_, value)| value.as_str().to_string());
        let process_pid = resource
            .iter()
            .find(|(key, _)| key.as_str() == "process.pid")
            .map(|(_, value)| match value {
                Value::I64(pid) => Some(*pid),
                _ => None,
            })
            .flatten();

        assert_eq!(
            process_name.as_deref(),
            current_process_name().as_deref(),
            "process.executable.name should match the current executable"
        );
        assert_eq!(
            process_pid,
            Some(std::process::id() as i64),
            "process.pid should match the current process id"
        );
    }
}
