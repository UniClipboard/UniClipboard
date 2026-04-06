//! Centralized field redaction for OTLP span export.
//!
//! Wraps any `SpanExporter` and scrubs sensitive field values before they
//! leave the process. This ensures privacy guarantees are enforced at the
//! pipeline layer rather than relying on individual call sites.
//!
//! ## Blocklist approach (v1)
//!
//! Uses an explicit field-name blocklist. Matching is case-insensitive to
//! catch both `snake_case` tracing fields and `camelCase` OTel conventions.

use std::fmt;

use opentelemetry::{Key, KeyValue, Value};
use opentelemetry_sdk::trace::SpanData;

/// Replacement value inserted for redacted fields.
const REDACTED: &str = "[REDACTED]";

/// Field names whose values must be redacted before OTLP export.
///
/// Organised by category. Matching is case-insensitive.
///
/// ## Extension
///
/// Add new entries here when new sensitive fields are introduced.
/// Prefer explicit names over substring matching to avoid false positives.
const REDACTED_FIELDS: &[&str] = &[
    // ── Clipboard content (never export user data) ──────────────
    "content",
    "text",
    "raw_text",
    "display_text",
    "clipboard_content",
    "clipboard_text",
    "clipboard.content",
    "clipboard.text",
    "payload",
    "body",
    // ── Binary / byte data ──────────────────────────────────────
    "raw_bytes",
    "blob_data",
    // ── Authentication & secrets ────────────────────────────────
    "token",
    "auth_token",
    "session_token",
    "api_key",
    "secret",
    "password",
    "passphrase",
    "credential",
    "private_key",
    "encryption_key",
    "master_key",
    // ── File system paths (may leak usernames / dir structure) ──
    "file_path",
    "file_content",
    "full_path",
    "home_dir",
    // ── Sensitive queries ───────────────────────────────────────
    "search_query",
];

fn canonicalize_key(input: &str) -> String {
    input
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Returns `true` if `key` matches any entry in the blocklist (case-insensitive).
fn is_sensitive(key: &Key) -> bool {
    let k = canonicalize_key(key.as_str());
    REDACTED_FIELDS
        .iter()
        .any(|blocked| k == canonicalize_key(blocked))
}

/// Redact sensitive values in a slice of [`KeyValue`] attributes in-place.
fn redact_attributes(attrs: &mut [KeyValue]) {
    for attr in attrs.iter_mut() {
        if is_sensitive(&attr.key) {
            attr.value = Value::String(REDACTED.into());
        }
    }
}

/// Redact all sensitive fields in a single [`SpanData`].
pub(crate) fn redact_span(span: &mut SpanData) {
    redact_attributes(&mut span.attributes);
    for event in &mut span.events.events {
        redact_attributes(&mut event.attributes);
    }
}

// ── RedactingExporter ───────────────────────────────────────────

/// A [`SpanExporter`] decorator that redacts sensitive field values before
/// forwarding the batch to the inner exporter.
pub struct RedactingExporter<E> {
    inner: E,
}

impl<E> RedactingExporter<E> {
    pub fn new(inner: E) -> Self {
        Self { inner }
    }
}

impl<E: fmt::Debug> fmt::Debug for RedactingExporter<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedactingExporter")
            .field("inner", &self.inner)
            .finish()
    }
}

impl<E: opentelemetry_sdk::trace::SpanExporter + 'static> opentelemetry_sdk::trace::SpanExporter
    for RedactingExporter<E>
{
    fn export(
        &self,
        batch: Vec<SpanData>,
    ) -> impl std::future::Future<Output = opentelemetry_sdk::error::OTelSdkResult> + Send {
        let mut redacted = batch;
        for span in &mut redacted {
            redact_span(span);
        }
        self.inner.export(redacted)
    }

    fn shutdown(&mut self) -> opentelemetry_sdk::error::OTelSdkResult {
        self.inner.shutdown()
    }

    fn force_flush(&mut self) -> opentelemetry_sdk::error::OTelSdkResult {
        self.inner.force_flush()
    }

    fn set_resource(&mut self, resource: &opentelemetry_sdk::Resource) {
        self.inner.set_resource(resource);
    }
}

// ── Tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::KeyValue;

    fn assert_redacted(value: &Value) {
        assert_eq!(*value, Value::String(REDACTED.into()));
    }

    fn assert_not_redacted(value: &Value, expected: &'static str) {
        assert_eq!(*value, Value::String(expected.into()));
    }

    #[test]
    fn blocklist_redacts_clipboard_content() {
        let mut attrs = vec![
            KeyValue::new("content", "super secret clipboard text"),
            KeyValue::new("entry_id", "abc-123"),
        ];
        redact_attributes(&mut attrs);
        assert_redacted(&attrs[0].value);
        assert_not_redacted(&attrs[1].value, "abc-123");
    }

    #[test]
    fn blocklist_redacts_auth_fields() {
        for field in ["token", "password", "passphrase", "api_key", "secret"] {
            let mut attrs = vec![KeyValue::new(field, "sensitive-value")];
            redact_attributes(&mut attrs);
            assert_redacted(&attrs[0].value);
        }
    }

    #[test]
    fn blocklist_is_case_insensitive() {
        let mut attrs = vec![KeyValue::new("Content", "secret")];
        redact_attributes(&mut attrs);
        assert_redacted(&attrs[0].value);
    }

    #[test]
    fn non_sensitive_fields_pass_through() {
        let mut attrs = vec![
            KeyValue::new("entry_id", "abc-123"),
            KeyValue::new("device_id", "dev-456"),
            KeyValue::new("content_type", "text/plain"),
            KeyValue::new("profile", "prod"),
        ];
        let original: Vec<_> = attrs.iter().map(|kv| kv.value.clone()).collect();
        redact_attributes(&mut attrs);
        for (i, attr) in attrs.iter().enumerate() {
            assert_eq!(
                attr.value, original[i],
                "field '{}' should NOT be redacted",
                attr.key
            );
        }
    }

    #[test]
    fn redact_span_covers_attributes_and_events() {
        use opentelemetry::trace::{SpanContext, SpanId, Status, TraceFlags, TraceId, TraceState};
        use std::borrow::Cow;
        use std::time::SystemTime;

        // Build a minimal SpanData with sensitive attributes and events.
        let mut span = SpanData {
            span_context: SpanContext::new(
                TraceId::from_bytes([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]),
                SpanId::from_bytes([0, 0, 0, 0, 0, 0, 0, 1]),
                TraceFlags::default(),
                false,
                TraceState::default(),
            ),
            parent_span_id: SpanId::INVALID,
            parent_span_is_remote: false,
            span_kind: opentelemetry::trace::SpanKind::Internal,
            name: Cow::Borrowed("test"),
            start_time: SystemTime::now(),
            end_time: SystemTime::now(),
            attributes: vec![KeyValue::new("password", "hunter2")],
            dropped_attributes_count: 0,
            events: Default::default(),
            links: Default::default(),
            status: Status::Unset,
            instrumentation_scope: Default::default(),
        };

        // Add an event with a sensitive attribute.
        span.events.events.push(opentelemetry::trace::Event::new(
            "log",
            SystemTime::now(),
            vec![KeyValue::new("content", "secret clipboard")],
            0,
        ));

        redact_span(&mut span);

        assert_redacted(&span.attributes[0].value);
        assert_redacted(&span.events.events[0].attributes[0].value);
    }
}
