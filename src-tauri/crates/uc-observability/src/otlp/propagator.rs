use opentelemetry::global;
use opentelemetry::Context;
use std::collections::HashMap;
use tracing::Span;
use tracing_opentelemetry::OpenTelemetrySpanExt;

/// Export the current tracing span's OTel context as a W3C traceparent header.
/// Returns None if no valid context is active (e.g., outside any span).
pub fn inject_current_context() -> Option<String> {
    let ctx = Span::current().context();
    let mut carrier: HashMap<String, String> = HashMap::new();
    global::get_text_map_propagator(|propagator| {
        propagator.inject_context(&ctx, &mut carrier);
    });
    carrier.remove("traceparent")
}

/// Extract a remote OTel context from an optional traceparent header.
/// Returns an empty Context if the header is missing or malformed.
pub fn extract_remote_context(traceparent: Option<impl AsRef<str>>) -> Context {
    let mut carrier: HashMap<String, String> = HashMap::new();
    if let Some(tp) = traceparent {
        carrier.insert("traceparent".to_string(), tp.as_ref().to_string());
    }
    global::get_text_map_propagator(|propagator| propagator.extract(&carrier))
}
