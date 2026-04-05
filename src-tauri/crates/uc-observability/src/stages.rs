//! Phase 87: stage constants are now dotted span names (OTel semconv-aligned).
//! Used directly as info_span! names.
//!
//! Each constant represents a discrete stage in the clipboard capture pipeline.
//! Used as tracing span names to provide consistent, queryable stage identifiers
//! across the `uc-app` and `uc-tauri` crates.

pub const DETECT: &str = "clipboard.detect";
pub const NORMALIZE: &str = "clipboard.normalize";
pub const PERSIST_EVENT: &str = "clipboard.persist_event";
pub const CACHE_REPRESENTATIONS: &str = "clipboard.cache_representations";
pub const SELECT_POLICY: &str = "clipboard.select_policy";
pub const PERSIST_ENTRY: &str = "clipboard.persist_entry";
pub const SPOOL_BLOBS: &str = "clipboard.spool_blobs";

pub const OUTBOUND_PREPARE: &str = "clipboard.outbound_prepare";
pub const OUTBOUND_SEND: &str = "clipboard.outbound_send";
pub const INBOUND_DECODE: &str = "clipboard.inbound_decode";
pub const INBOUND_APPLY: &str = "clipboard.inbound_apply";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_constants_are_dotted_otel_form() {
        let stages = [
            ("DETECT", DETECT),
            ("NORMALIZE", NORMALIZE),
            ("PERSIST_EVENT", PERSIST_EVENT),
            ("CACHE_REPRESENTATIONS", CACHE_REPRESENTATIONS),
            ("SELECT_POLICY", SELECT_POLICY),
            ("PERSIST_ENTRY", PERSIST_ENTRY),
            ("SPOOL_BLOBS", SPOOL_BLOBS),
            ("OUTBOUND_PREPARE", OUTBOUND_PREPARE),
            ("OUTBOUND_SEND", OUTBOUND_SEND),
            ("INBOUND_DECODE", INBOUND_DECODE),
            ("INBOUND_APPLY", INBOUND_APPLY),
        ];
        for (name, value) in stages {
            assert!(
                value.starts_with("clipboard."),
                "Stage {} (value: '{}') must start with 'clipboard.'",
                name,
                value
            );
            assert!(!value.is_empty(), "Stage {} must be non-empty", name);
        }
    }

    #[test]
    fn all_stages_are_non_empty() {
        assert!(!DETECT.is_empty());
        assert!(!NORMALIZE.is_empty());
        assert!(!PERSIST_EVENT.is_empty());
        assert!(!CACHE_REPRESENTATIONS.is_empty());
        assert!(!SELECT_POLICY.is_empty());
        assert!(!PERSIST_ENTRY.is_empty());
        assert!(!SPOOL_BLOBS.is_empty());
        assert!(!OUTBOUND_PREPARE.is_empty());
        assert!(!OUTBOUND_SEND.is_empty());
        assert!(!INBOUND_DECODE.is_empty());
        assert!(!INBOUND_APPLY.is_empty());
    }
}
