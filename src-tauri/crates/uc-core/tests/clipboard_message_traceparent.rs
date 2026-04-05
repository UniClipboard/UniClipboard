//! Wave 0 scaffold for Phase 87. Verifies serde backward-compatibility of the new
//! `ClipboardMessage.traceparent` field added in Plan 03.
//!
//! The test pattern mirrors `origin_flow_id_defaults_to_none_when_missing_from_json`
//! (clipboard.rs line ~180) which established the serde(default) + skip_serializing_if
//! backward-compat convention for Phase 21.

#![allow(deprecated)]

use chrono::Utc;
use uc_core::network::protocol::{ClipboardMessage, ClipboardPayloadVersion};

/// REQ-87-06 — Backward compat: older peers that omit `traceparent` must still deserialize.
///
/// JSON from an old peer that predates the traceparent field should deserialize
/// with `traceparent: None` (serde(default) behavior).
#[test]
fn traceparent_defaults_to_none_when_missing_from_json() {
    let json = r#"{
        "id": "msg-traceparent-compat",
        "content_hash": "hash",
        "encrypted_content": "aGVsbG8=",
        "timestamp": "2024-01-01T00:00:00Z",
        "origin_device_id": "dev-1",
        "origin_device_name": "Device",
        "payload_version": 3
    }"#;

    let msg: ClipboardMessage = serde_json::from_str(json).expect("backward compat deserialize");
    assert!(
        msg.traceparent.is_none(),
        "traceparent should be None when field is absent from JSON (backward compat)"
    );
}

/// REQ-87-06 — Roundtrip: traceparent value is preserved through serialize → deserialize.
#[test]
fn traceparent_roundtrips_when_present() {
    let traceparent_value =
        "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01".to_string();

    let msg = ClipboardMessage {
        id: "test-traceparent".to_string(),
        content_hash: "h".to_string(),
        encrypted_content: vec![],
        timestamp: Utc::now(),
        origin_device_id: "d".to_string(),
        origin_device_name: "D".to_string(),
        payload_version: ClipboardPayloadVersion::V3,
        origin_flow_id: None,
        file_transfers: vec![],
        traceparent: Some(traceparent_value.clone()),
    };

    let json = serde_json::to_string(&msg).expect("serialize with traceparent");
    let decoded: ClipboardMessage =
        serde_json::from_str(&json).expect("deserialize with traceparent");

    assert_eq!(
        decoded.traceparent.as_deref(),
        Some(traceparent_value.as_str()),
        "traceparent value must survive a serialize/deserialize roundtrip"
    );
}

/// REQ-87-06 — skip_serializing_if: when traceparent is None it must not appear in JSON output.
#[test]
fn traceparent_field_skipped_when_none_in_output() {
    let msg = ClipboardMessage {
        id: "test-skip-field".to_string(), // NOTE: must NOT contain "traceparent" substring
        content_hash: "h".to_string(),
        encrypted_content: vec![],
        timestamp: Utc::now(),
        origin_device_id: "d".to_string(),
        origin_device_name: "D".to_string(),
        payload_version: ClipboardPayloadVersion::V3,
        origin_flow_id: None,
        file_transfers: vec![],
        traceparent: None,
    };

    let json = serde_json::to_string(&msg).expect("serialize with traceparent=None");
    assert!(
        !json.contains("\"traceparent\""),
        "traceparent key must NOT appear in serialized JSON when field is None (skip_serializing_if): {json}"
    );
}
