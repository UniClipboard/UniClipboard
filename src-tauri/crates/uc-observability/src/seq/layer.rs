//! Seq tracing layer that formats events to CLEF and sends via mpsc channel.

use std::collections::BTreeMap;
use std::fmt;

use serde::ser::{SerializeMap, Serializer as _};
use tokio::sync::mpsc;
use tracing::field::{Field, Visit};
use tracing::Subscriber;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

/// Map tracing Level to Seq/CLEF level name.
fn tracing_level_to_clef(level: &tracing::Level) -> &'static str {
    match *level {
        tracing::Level::TRACE => "Verbose",
        tracing::Level::DEBUG => "Debug",
        tracing::Level::INFO => "Information",
        tracing::Level::WARN => "Warning",
        tracing::Level::ERROR => "Error",
    }
}

/// A tracing layer that formats events as CLEF JSON and sends them
/// to the background Seq sender via an mpsc channel.
pub(crate) struct SeqLayer {
    tx: mpsc::Sender<String>,
    device_id: Option<String>,
}

impl SeqLayer {
    pub(crate) fn new(tx: mpsc::Sender<String>, device_id: Option<String>) -> Self {
        Self { tx, device_id }
    }
}

impl<S> Layer<S> for SeqLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_event(&self, event: &tracing::Event<'_>, ctx: tracing_subscriber::layer::Context<'_, S>) {
        // Format event as CLEF JSON
        let clef_json = match format_clef_event(event, &ctx, self.device_id.as_deref()) {
            Some(json) => json,
            None => return,
        };

        // Try to send, silently drop if channel is full
        let _ = self.tx.try_send(clef_json);
    }
}

/// Format a tracing event as a CLEF JSON string.
fn format_clef_event<S>(
    event: &tracing::Event<'_>,
    ctx: &tracing_subscriber::layer::Context<'_, S>,
    device_id: Option<&str>,
) -> Option<String>
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    let mut buf = Vec::new();
    let mut ser = serde_json::Serializer::new(&mut buf);
    let mut map = ser.serialize_map(None).ok()?;

    // @t - timestamp
    let timestamp = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    map.serialize_entry("@t", &timestamp).ok()?;

    // @l - level
    map.serialize_entry("@l", tracing_level_to_clef(event.metadata().level()))
        .ok()?;

    // Collect event fields
    let mut event_fields = BTreeMap::new();
    let mut visitor = ClefLayerVisitor::new(&mut event_fields);
    event.record(&mut visitor);

    // @m - message
    if let Some(message) = event_fields.remove("message") {
        map.serialize_entry("@m", &message).ok()?;
    } else {
        map.serialize_entry("@m", "").ok()?;
    }

    // target
    map.serialize_entry("target", event.metadata().target())
        .ok()?;

    // device_id - static field for cross-device log correlation
    if let Some(did) = device_id {
        map.serialize_entry("device_id", did).ok()?;
    }

    // Span fields - we need to manually walk the span scope since we have a Layer context
    // not a FmtContext. We use the same logic as collect_span_fields but adapted for Layer context.
    let mut span_fields = BTreeMap::new();
    let mut leaf_span_name: Option<String> = None;

    if let Some(scope) = ctx.event_scope(event) {
        let spans: Vec<_> = scope.collect();

        if let Some(leaf) = spans.first() {
            leaf_span_name = Some(leaf.name().to_string());
        }

        for span in spans.iter().rev() {
            let extensions = span.extensions();
            if let Some(fields) = extensions.get::<tracing_subscriber::fmt::FormattedFields<
                tracing_subscriber::fmt::format::JsonFields,
            >>() {
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

    if let Some(span_name) = &leaf_span_name {
        map.serialize_entry("span", span_name).ok()?;
    }

    for (key, value) in &span_fields {
        map.serialize_entry(key, value).ok()?;
    }

    for (key, value) in &event_fields {
        map.serialize_entry(key, value).ok()?;
    }

    map.end().ok()?;

    String::from_utf8(buf).ok()
}

/// Visitor that collects tracing fields for CLEF layer output.
struct ClefLayerVisitor<'a> {
    fields: &'a mut BTreeMap<String, serde_json::Value>,
}

impl<'a> ClefLayerVisitor<'a> {
    fn new(fields: &'a mut BTreeMap<String, serde_json::Value>) -> Self {
        Self { fields }
    }
}

impl<'a> Visit for ClefLayerVisitor<'a> {
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
