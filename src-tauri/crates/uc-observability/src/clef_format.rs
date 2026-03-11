//! CLEF (Compact Log Event Format) formatter for Seq ingestion.
//!
//! Produces newline-delimited JSON with CLEF fields (`@t`, `@l`, `@m`)
//! and flattened span fields at the top level.

use serde::ser::{SerializeMap, Serializer as _};
use std::collections::BTreeMap;
use std::fmt;
use tracing::field::{Field, Visit};
use tracing::Level;
use tracing::Subscriber;
use tracing_subscriber::fmt::format::{FormatFields, Writer};
use tracing_subscriber::fmt::{FmtContext, FormatEvent};
use tracing_subscriber::registry::LookupSpan;

use crate::span_fields::collect_span_fields;

/// A CLEF event formatter for Seq ingestion.
///
/// # CLEF JSON Structure
///
/// Each log line is a JSON object with:
/// - `@t` - ISO 8601 UTC timestamp (e.g., `2024-01-15T10:30:00.123Z`)
/// - `@l` - Seq level name (Verbose, Debug, Information, Warning, Error)
/// - `@m` - Rendered message string
/// - `target` - Rust module path of the log callsite
/// - `span` - Name of the current (leaf) span
/// - Span fields flattened to top level
/// - Event fields at top level
pub struct CLEFFormat;

impl CLEFFormat {
    /// Create a new `CLEFFormat` instance.
    pub fn new() -> Self {
        Self
    }

    fn format_timestamp() -> String {
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    }
}

impl Default for CLEFFormat {
    fn default() -> Self {
        Self::new()
    }
}

/// Map tracing Level to Seq/CLEF level name.
fn tracing_level_to_clef(level: &Level) -> &'static str {
    match *level {
        Level::TRACE => "Verbose",
        Level::DEBUG => "Debug",
        Level::INFO => "Information",
        Level::WARN => "Warning",
        Level::ERROR => "Error",
    }
}

impl<S, N> FormatEvent<S, N> for CLEFFormat
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

        // 1. CLEF base fields
        map.serialize_entry("@t", &Self::format_timestamp())
            .map_err(|_| fmt::Error)?;
        map.serialize_entry("@l", tracing_level_to_clef(event.metadata().level()))
            .map_err(|_| fmt::Error)?;

        // 2. Collect event fields (including message)
        let mut event_fields = BTreeMap::new();
        let mut visitor = ClefVisitor::new(&mut event_fields);
        event.record(&mut visitor);

        // Extract message
        if let Some(message) = event_fields.remove("message") {
            map.serialize_entry("@m", &message)
                .map_err(|_| fmt::Error)?;
        } else {
            map.serialize_entry("@m", "").map_err(|_| fmt::Error)?;
        }

        // 3. Target and span name
        map.serialize_entry("target", event.metadata().target())
            .map_err(|_| fmt::Error)?;

        // 4. Collect span fields using shared helper
        let (leaf_span_name, span_fields) = collect_span_fields(ctx);

        if let Some(span_name) = &leaf_span_name {
            map.serialize_entry("span", span_name)
                .map_err(|_| fmt::Error)?;
        }

        // 5. Flatten span fields at top level (no conflict resolution for CLEF)
        for (key, value) in &span_fields {
            map.serialize_entry(key, value).map_err(|_| fmt::Error)?;
        }

        // 6. Event fields at top level
        for (key, value) in &event_fields {
            map.serialize_entry(key, value).map_err(|_| fmt::Error)?;
        }

        map.end().map_err(|_| fmt::Error)?;

        writeln!(writer, "{}", String::from_utf8_lossy(&buf))
    }
}

/// Visitor that collects tracing fields as `serde_json::Value` entries for CLEF output.
struct ClefVisitor<'a> {
    fields: &'a mut BTreeMap<String, serde_json::Value>,
}

impl<'a> ClefVisitor<'a> {
    fn new(fields: &'a mut BTreeMap<String, serde_json::Value>) -> Self {
        Self { fields }
    }
}

impl<'a> Visit for ClefVisitor<'a> {
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
    fn test_clef_produces_valid_json() {
        let buf = BufWriter::new();
        let buf_clone = buf.clone();

        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .event_format(CLEFFormat::new())
                .fmt_fields(JsonFields::new())
                .with_writer(buf_clone)
                .with_ansi(false),
        );

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("test message");
        });

        let output = buf.contents();
        let line = output.trim();
        assert!(!line.is_empty(), "Expected CLEF JSON output");
        let parsed: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("Invalid JSON: {e}\nOutput: {line}"));
        assert!(parsed.is_object());

        let obj = parsed.as_object().unwrap();
        assert!(obj.contains_key("@t"), "Missing '@t' timestamp");
        assert!(obj.contains_key("@l"), "Missing '@l' level");
        assert!(obj.contains_key("@m"), "Missing '@m' message");
    }

    #[test]
    fn test_clef_level_mapping() {
        let test_cases = vec![
            (tracing::Level::TRACE, "Verbose"),
            (tracing::Level::DEBUG, "Debug"),
            (tracing::Level::INFO, "Information"),
            (tracing::Level::WARN, "Warning"),
            (tracing::Level::ERROR, "Error"),
        ];

        for (level, expected) in &test_cases {
            assert_eq!(
                tracing_level_to_clef(level),
                *expected,
                "Level {:?} should map to {}",
                level,
                expected
            );
        }

        // Also verify via actual event output
        let buf = BufWriter::new();
        let buf_clone = buf.clone();

        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .event_format(CLEFFormat::new())
                .fmt_fields(JsonFields::new())
                .with_writer(buf_clone)
                .with_ansi(false),
        );

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("info event");
        });

        let output = buf.contents();
        let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
        assert_eq!(parsed["@l"], "Information");
    }

    #[test]
    fn test_clef_includes_span_fields() {
        let buf = BufWriter::new();
        let buf_clone = buf.clone();

        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .event_format(CLEFFormat::new())
                .fmt_fields(JsonFields::new())
                .with_writer(buf_clone)
                .with_ansi(false),
        );

        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("my_operation", flow_id = "abc-123", stage = "capture");
            let _enter = span.enter();
            tracing::info!("inside span");
        });

        let output = buf.contents();
        let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
        let obj = parsed.as_object().unwrap();

        assert_eq!(
            obj.get("flow_id").and_then(|v| v.as_str()),
            Some("abc-123"),
            "flow_id should be flattened at top level"
        );
        assert_eq!(
            obj.get("stage").and_then(|v| v.as_str()),
            Some("capture"),
            "stage should be flattened at top level"
        );
        assert_eq!(
            obj.get("span").and_then(|v| v.as_str()),
            Some("my_operation"),
            "span name should be present"
        );
    }

    #[test]
    fn test_clef_includes_event_fields() {
        let buf = BufWriter::new();
        let buf_clone = buf.clone();

        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .event_format(CLEFFormat::new())
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

    #[test]
    fn test_clef_timestamp_is_iso8601() {
        let buf = BufWriter::new();
        let buf_clone = buf.clone();

        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .event_format(CLEFFormat::new())
                .fmt_fields(JsonFields::new())
                .with_writer(buf_clone)
                .with_ansi(false),
        );

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("timestamp test");
        });

        let output = buf.contents();
        let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
        let timestamp = parsed["@t"].as_str().unwrap();

        // Verify RFC 3339 format (subset of ISO 8601)
        assert!(
            chrono::DateTime::parse_from_rfc3339(timestamp).is_ok(),
            "Timestamp '{}' is not valid RFC 3339",
            timestamp
        );
        // Should end with Z (UTC)
        assert!(
            timestamp.ends_with('Z'),
            "Timestamp should be UTC (end with Z)"
        );
    }
}
