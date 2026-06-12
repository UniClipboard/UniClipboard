//! Centralized field-name blocklist for telemetry redaction.
//!
//! Pure-Rust helpers (no Sentry / OTLP type dependencies) so the same field
//! list is reused across whatever telemetry sinks the bootstrap layer wires
//! up. Keys are matched case-insensitively and after stripping non-
//! alphanumeric characters, so `clipboard.text`, `clipboardText`, and
//! `clipboard_text` all collapse to the same canonical form.

/// Replacement value inserted for redacted fields.
pub const REDACTED_PLACEHOLDER: &str = "[REDACTED]";

/// Field names whose values must be scrubbed before leaving the process.
///
/// Organised by category. Add new entries here when new sensitive fields are
/// introduced. Prefer explicit names over substring matching to avoid false
/// positives.
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

fn canonicalize(input: &str) -> String {
    input
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Returns `true` if `key` matches any entry in the blocklist after canonical
/// folding (lowercase + alphanumerics only).
pub fn is_sensitive_key(key: &str) -> bool {
    let k = canonicalize(key);
    REDACTED_FIELDS
        .iter()
        .any(|blocked| k == canonicalize(blocked))
}
