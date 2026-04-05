//! Wave 0 scaffold for Phase 87. Targets uc_observability::otlp::{build_resource, propagator} in Plan 02.
//!
//! All tests here are gated behind `__wave0_scaffold_87` feature so the default
//! workspace build remains green. After Plan 02 lands, run:
//!   cd src-tauri && cargo test -p uc-observability --features __wave0_scaffold_87
//! to flip them green.
#![cfg(feature = "__wave0_scaffold_87")]

use uc_observability::otlp::{build_resource, propagator};

/// REQ-87-03 — build_resource populates standard OTel semantic convention attributes.
///
/// Verifies that the resource contains:
/// - `service.name` = "uniclipboard-desktop"
/// - `service.version` (non-empty)
/// - `service.instance.id` = "device-xyz"
/// - `deployment.environment.name` (present)
/// - `os.type` (present)
#[test]
fn build_resource_contains_semconv_keys() {
    let resource = build_resource(Some("device-xyz"));

    // Collect all attribute keys from the resource.
    // resource.iter() yields (&Key, &Value) pairs.
    let attrs: Vec<(opentelemetry::Key, opentelemetry::Value)> = resource
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let keys: Vec<&str> = attrs.iter().map(|(k, _)| k.as_str()).collect();

    // service.name must be "uniclipboard-desktop"
    let svc_name_value = attrs
        .iter()
        .find(|(k, _)| k.as_str() == "service.name")
        .map(|(_, v)| v.as_str().to_string())
        .expect("service.name must be present in OTLP resource");
    assert_eq!(
        svc_name_value.as_str(),
        "uniclipboard-desktop",
        "service.name should be 'uniclipboard-desktop'"
    );

    // service.version must be present and non-empty
    let svc_version_value = attrs
        .iter()
        .find(|(k, _)| k.as_str() == "service.version")
        .map(|(_, v)| v.as_str().to_string())
        .expect("service.version must be present in OTLP resource");
    assert!(
        !svc_version_value.is_empty(),
        "service.version should not be empty"
    );

    // service.instance.id = "device-xyz"
    let instance_id_value = attrs
        .iter()
        .find(|(k, _)| k.as_str() == "service.instance.id")
        .map(|(_, v)| v.as_str().to_string())
        .expect("service.instance.id must be present when device_id is supplied");
    assert_eq!(
        instance_id_value.as_str(),
        "device-xyz",
        "service.instance.id should equal the supplied device_id"
    );

    // deployment.environment.name must be present
    assert!(
        keys.contains(&"deployment.environment.name"),
        "deployment.environment.name must be present; got keys: {keys:?}"
    );

    // os.type must be present
    assert!(
        keys.contains(&"os.type"),
        "os.type must be present; got keys: {keys:?}"
    );
}

/// REQ-87-03 — service.instance.id must be absent when device_id is None.
#[test]
fn build_resource_omits_instance_id_when_device_missing() {
    let resource = build_resource(None);
    let has_instance_id = resource
        .iter()
        .any(|(k, _)| k.as_str() == "service.instance.id");
    assert!(
        !has_instance_id,
        "service.instance.id should be absent when device_id is None"
    );
}

/// REQ-87-06 — W3C traceparent roundtrip: inject current context → extract → same trace_id.
///
/// NOTE: Full roundtrip assertion requires an active span with a valid TraceContext.
/// This test is `#[ignore]`d with a TODO until Plan 02 implements the real propagator.
/// The function MUST exist and compile now.
#[test]
#[ignore = "TODO Plan 02: wire TraceContextPropagator + real inject/extract helpers"]
fn traceparent_roundtrip() {
    use opentelemetry::trace::TraceContextExt;
    use tracing::info_span;

    // Build a minimal local subscriber for this test (not set globally).
    let subscriber = tracing_subscriber::registry();

    tracing::subscriber::with_default(subscriber, || {
        let root_span = info_span!("test.root");
        let _guard = root_span.enter();

        // Inject current context to produce a traceparent header.
        let header = propagator::inject_current_context();
        assert!(
            header.is_some(),
            "inject_current_context() should return Some(traceparent) when inside an active span"
        );

        // Extract back to an OTel context.
        let remote_ctx = propagator::extract_remote_context(header);

        // The extracted span context must have the same trace_id as the original.
        let remote_span_ctx = remote_ctx.span().span_context().clone();
        assert!(
            remote_span_ctx.is_valid(),
            "extracted SpanContext must be valid"
        );
        // TODO Plan 02: assert remote_span_ctx.trace_id() == original trace_id
    });
}
