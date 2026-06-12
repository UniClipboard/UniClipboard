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

use crate::context::global_device_id;
use crate::span_fields::collect_span_fields;

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

        // 3. Collect span fields (root to leaf) and span name using shared helper
        let (leaf_span_name, span_fields) = collect_span_fields(ctx);

        if let Some(span_name) = &leaf_span_name {
            map.serialize_entry("span", span_name)
                .map_err(|_| fmt::Error)?;
        }

        let has_device_id =
            event_fields.contains_key("device_id") || span_fields.contains_key("device_id");

        if !has_device_id {
            if let Some(device_id) = global_device_id() {
                map.serialize_entry("device_id", device_id)
                    .map_err(|_| fmt::Error)?;
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
