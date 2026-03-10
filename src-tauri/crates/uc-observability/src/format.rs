//! Custom flat JSON formatter for tracing events.
//!
//! Produces newline-delimited JSON with span fields flattened to the top level,
//! using `parent_` prefix for conflicting keys.

use serde::ser::{SerializeMap, Serializer as _};
use std::collections::BTreeMap;
use std::fmt;
use tracing::field::{Field, Visit};
use tracing::Subscriber;
use tracing_subscriber::fmt::format::{FormatFields, Writer};
use tracing_subscriber::fmt::{FmtContext, FormatEvent};
use tracing_subscriber::registry::LookupSpan;

/// A flat JSON event formatter that merges span fields into the top-level JSON object.
///
/// # JSON Structure
///
/// Each log line is a JSON object with:
/// - `timestamp` - ISO 8601 UTC timestamp
/// - `level` - Log level (TRACE, DEBUG, INFO, WARN, ERROR)
/// - `target` - Rust module path of the log callsite
/// - `message` - The log message string
/// - `span` - Name of the current (leaf) span
/// - Span fields flattened to top level
/// - Event fields at top level
///
/// # Conflict Resolution
///
/// If a span field has the same key as an event field, the span field is
/// prefixed with `parent_`. Event fields always keep their original key.
pub struct FlatJsonFormat;

impl FlatJsonFormat {
    /// Create a new `FlatJsonFormat` instance.
    pub fn new() -> Self {
        Self
    }

    fn format_timestamp() -> String {
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    }
}

impl Default for FlatJsonFormat {
    fn default() -> Self {
        Self::new()
    }
}

impl<S, N> FormatEvent<S, N> for FlatJsonFormat
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    N: for<'writer> FormatFields<'writer> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> fmt::Result {
        let mut buf = Vec::new();
        let mut ser = serde_json::Serializer::new(&mut buf);
        let mut map = ser.serialize_map(None).map_err(|_| fmt::Error)?;

        // 1. Base fields
        map.serialize_entry("timestamp", &Self::format_timestamp())
            .map_err(|_| fmt::Error)?;
        map.serialize_entry("level", &event.metadata().level().as_str())
            .map_err(|_| fmt::Error)?;
        map.serialize_entry("target", event.metadata().target())
            .map_err(|_| fmt::Error)?;

        // 2. Collect event fields (including message)
        let mut event_fields = BTreeMap::new();
        let mut visitor = JsonVisitor::new(&mut event_fields);
        event.record(&mut visitor);

        // Extract message from event fields
        if let Some(message) = event_fields.remove("message") {
            map.serialize_entry("message", &message)
                .map_err(|_| fmt::Error)?;
        } else {
            map.serialize_entry("message", "").map_err(|_| fmt::Error)?;
        }

        // 3. Collect span fields (root to leaf) and span name
        let mut span_fields = BTreeMap::new();
        if let Some(scope) = ctx.event_scope() {
            let spans: Vec<_> = scope.collect();

            // Leaf span name (last in the scope = first collected since scope iterates leaf-to-root)
            if let Some(leaf) = spans.first() {
                map.serialize_entry("span", leaf.name())
                    .map_err(|_| fmt::Error)?;
            }

            // Walk root-to-leaf to collect span fields
            for span in spans.iter().rev() {
                let extensions = span.extensions();
                if let Some(fields) =
                    extensions.get::<tracing_subscriber::fmt::FormattedFields<N>>()
                {
                    // Try to parse as JSON (when using JsonFields)
                    if let Ok(parsed) =
                        serde_json::from_str::<BTreeMap<String, serde_json::Value>>(fields.as_ref())
                    {
                        for (key, value) in parsed {
                            span_fields.insert(key, value);
                        }
                    }
                }
            }
        }

        // 4. Merge: span fields with conflict resolution, then event fields
        for (key, value) in &span_fields {
            if event_fields.contains_key(key) {
                map.serialize_entry(&format!("parent_{}", key), value)
                    .map_err(|_| fmt::Error)?;
            } else {
                map.serialize_entry(key, value).map_err(|_| fmt::Error)?;
            }
        }

        for (key, value) in &event_fields {
            map.serialize_entry(key, value).map_err(|_| fmt::Error)?;
        }

        map.end().map_err(|_| fmt::Error)?;

        // Write the JSON line
        writeln!(writer, "{}", String::from_utf8_lossy(&buf))
    }
}

/// Visitor that collects tracing fields as `serde_json::Value` entries.
struct JsonVisitor<'a> {
    fields: &'a mut BTreeMap<String, serde_json::Value>,
}

impl<'a> JsonVisitor<'a> {
    fn new(fields: &'a mut BTreeMap<String, serde_json::Value>) -> Self {
        Self { fields }
    }
}

impl<'a> Visit for JsonVisitor<'a> {
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.fields
            .insert(field.name().to_string(), serde_json::Value::from(value));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields
            .insert(field.name().to_string(), serde_json::Value::from(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields
            .insert(field.name().to_string(), serde_json::Value::from(value));
    }

    fn record_i128(&mut self, field: &Field, value: i128) {
        self.fields.insert(
            field.name().to_string(),
            serde_json::Value::from(value.to_string()),
        );
    }

    fn record_u128(&mut self, field: &Field, value: u128) {
        self.fields.insert(
            field.name().to_string(),
            serde_json::Value::from(value.to_string()),
        );
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields
            .insert(field.name().to_string(), serde_json::Value::from(value));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.fields
            .insert(field.name().to_string(), serde_json::Value::from(value));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.fields.insert(
            field.name().to_string(),
            serde_json::Value::from(format!("{:?}", value)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::fmt::format::JsonFields;
    use tracing_subscriber::prelude::*;

    /// A writer that captures output into a shared buffer.
    #[derive(Clone)]
    struct BufWriter(Arc<Mutex<Vec<u8>>>);

    impl BufWriter {
        fn new() -> Self {
            Self(Arc::new(Mutex::new(Vec::new())))
        }

        fn contents(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    impl std::io::Write for BufWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for BufWriter {
        type Writer = BufWriter;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    #[test]
    fn test_flat_json_produces_valid_json() {
        let buf = BufWriter::new();
        let buf_clone = buf.clone();

        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .event_format(FlatJsonFormat::new())
                .fmt_fields(JsonFields::new())
                .with_writer(buf_clone)
                .with_ansi(false),
        );

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("test message");
        });

        let output = buf.contents();
        let line = output.trim();
        assert!(!line.is_empty(), "Expected JSON output");
        let parsed: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("Invalid JSON: {e}\nOutput: {line}"));
        assert!(parsed.is_object());
    }

    #[test]
    fn test_flat_json_has_required_base_fields() {
        let buf = BufWriter::new();
        let buf_clone = buf.clone();

        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .event_format(FlatJsonFormat::new())
                .fmt_fields(JsonFields::new())
                .with_writer(buf_clone)
                .with_ansi(false),
        );

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(target: "my_target", "hello world");
        });

        let output = buf.contents();
        let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
        let obj = parsed.as_object().unwrap();

        assert!(obj.contains_key("timestamp"), "Missing 'timestamp'");
        assert!(obj.contains_key("level"), "Missing 'level'");
        assert!(obj.contains_key("target"), "Missing 'target'");
        assert!(obj.contains_key("message"), "Missing 'message'");

        assert_eq!(obj["level"], "INFO");
        assert_eq!(obj["target"], "my_target");
        assert_eq!(obj["message"], "hello world");
    }

    #[test]
    fn test_flat_json_includes_span_name_and_fields() {
        let buf = BufWriter::new();
        let buf_clone = buf.clone();

        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .event_format(FlatJsonFormat::new())
                .fmt_fields(JsonFields::new())
                .with_writer(buf_clone)
                .with_ansi(false),
        );

        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("my_operation", user_id = 42);
            let _enter = span.enter();
            tracing::info!("inside span");
        });

        let output = buf.contents();
        let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
        let obj = parsed.as_object().unwrap();

        assert_eq!(
            obj.get("span").and_then(|v| v.as_str()),
            Some("my_operation")
        );
        assert_eq!(obj.get("user_id").and_then(|v| v.as_u64()), Some(42));
    }

    #[test]
    fn test_flat_json_flattens_parent_span_fields() {
        let buf = BufWriter::new();
        let buf_clone = buf.clone();

        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .event_format(FlatJsonFormat::new())
                .fmt_fields(JsonFields::new())
                .with_writer(buf_clone)
                .with_ansi(false),
        );

        tracing::subscriber::with_default(subscriber, || {
            let parent = tracing::info_span!("parent_op", request_id = "abc-123");
            let _parent_enter = parent.enter();
            let child = tracing::info_span!("child_op", step = 2);
            let _child_enter = child.enter();
            tracing::info!("nested event");
        });

        let output = buf.contents();
        let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
        let obj = parsed.as_object().unwrap();

        // Parent span fields should be flattened to top level
        assert_eq!(
            obj.get("request_id").and_then(|v| v.as_str()),
            Some("abc-123")
        );
        assert_eq!(obj.get("step").and_then(|v| v.as_u64()), Some(2));
        // Leaf span name
        assert_eq!(obj.get("span").and_then(|v| v.as_str()), Some("child_op"));
    }

    #[test]
    fn test_flat_json_prefixes_conflicting_span_keys() {
        let buf = BufWriter::new();
        let buf_clone = buf.clone();

        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .event_format(FlatJsonFormat::new())
                .fmt_fields(JsonFields::new())
                .with_writer(buf_clone)
                .with_ansi(false),
        );

        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("op", status = "pending");
            let _enter = span.enter();
            tracing::info!(status = "completed", "status changed");
        });

        let output = buf.contents();
        let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
        let obj = parsed.as_object().unwrap();

        // Event field keeps original key
        assert_eq!(
            obj.get("status").and_then(|v| v.as_str()),
            Some("completed")
        );
        // Span field gets parent_ prefix
        assert_eq!(
            obj.get("parent_status").and_then(|v| v.as_str()),
            Some("pending")
        );
    }

    #[test]
    fn test_flat_json_event_fields_at_top_level() {
        let buf = BufWriter::new();
        let buf_clone = buf.clone();

        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .event_format(FlatJsonFormat::new())
                .fmt_fields(JsonFields::new())
                .with_writer(buf_clone)
                .with_ansi(false),
        );

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(count = 5, name = "test", "event with fields");
        });

        let output = buf.contents();
        let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
        let obj = parsed.as_object().unwrap();

        assert_eq!(obj.get("count").and_then(|v| v.as_u64()), Some(5));
        assert_eq!(obj.get("name").and_then(|v| v.as_str()), Some("test"));
    }
}
