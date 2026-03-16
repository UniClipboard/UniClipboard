//! Serialization tests for command-layer DTO models.
//! 命令层 DTO 模型序列化测试。

use uc_app::usecases::LifecycleState;
use uc_core::settings::model::Settings;
use uc_tauri::bootstrap::file_transfer_wiring::FileTransferStatusPayload;
use uc_tauri::models::{ClipboardEntriesResponse, ClipboardEntryProjection, LifecycleStatusDto};

#[test]
fn clipboard_entries_response_ready_serializes_correctly() {
    let response = ClipboardEntriesResponse::Ready { entries: vec![] };
    let json = serde_json::to_string(&response).expect("serialize failed");
    assert_eq!(json, r#"{"status":"ready","entries":[]}"#);
}

#[test]
fn clipboard_entries_response_not_ready_serializes_correctly() {
    let response = ClipboardEntriesResponse::NotReady;
    let json = serde_json::to_string(&response).expect("serialize failed");
    assert_eq!(json, r#"{"status":"not_ready"}"#);
}

#[test]
fn lifecycle_status_dto_serializes_with_camel_case() {
    let dto = LifecycleStatusDto {
        state: LifecycleState::Ready,
    };
    let value = serde_json::to_value(&dto).expect("serialize failed");
    assert!(
        value.get("state").is_some(),
        "expected 'state' field in JSON"
    );
    assert_eq!(value["state"], serde_json::json!("Ready"));

    let idle = LifecycleStatusDto::from_state(LifecycleState::Idle);
    let idle_json = serde_json::to_value(&idle).expect("serialize failed");
    assert_eq!(idle_json["state"], serde_json::json!("Idle"));

    let watcher_failed = LifecycleStatusDto::from_state(LifecycleState::WatcherFailed);
    let wf_json = serde_json::to_value(&watcher_failed).expect("serialize failed");
    assert_eq!(wf_json["state"], serde_json::json!("WatcherFailed"));
}

#[test]
fn clipboard_entry_projection_preserves_snake_case() {
    let entry = ClipboardEntryProjection {
        id: "test-id".to_string(),
        preview: "hello".to_string(),
        has_detail: true,
        size_bytes: 100,
        captured_at: 1234567890,
        content_type: "text/plain".to_string(),
        thumbnail_url: None,
        is_encrypted: false,
        is_favorited: false,
        updated_at: 1234567890,
        active_time: 1234567890,
        file_transfer_status: None,
        file_transfer_reason: None,
        link_urls: None,
        link_domains: None,
        file_sizes: None,
    };
    let value = serde_json::to_value(&entry).expect("serialize failed");
    // Verify snake_case field names (not camelCase)
    assert!(
        value.get("has_detail").is_some(),
        "expected snake_case 'has_detail'"
    );
    assert!(
        value.get("size_bytes").is_some(),
        "expected snake_case 'size_bytes'"
    );
    assert!(
        value.get("captured_at").is_some(),
        "expected snake_case 'captured_at'"
    );
    assert!(
        value.get("is_encrypted").is_some(),
        "expected snake_case 'is_encrypted'"
    );
    // Ensure camelCase variants are NOT present
    assert!(
        value.get("hasDetail").is_none(),
        "unexpected camelCase 'hasDetail'"
    );
    assert!(
        value.get("sizeBytes").is_none(),
        "unexpected camelCase 'sizeBytes'"
    );
}

#[test]
fn clipboard_entry_projection_includes_file_transfer_status_when_present() {
    let entry = ClipboardEntryProjection {
        id: "file-entry-1".to_string(),
        preview: "document.pdf".to_string(),
        has_detail: false,
        size_bytes: 1024,
        captured_at: 1700000000,
        content_type: "text/uri-list".to_string(),
        thumbnail_url: None,
        is_encrypted: false,
        is_favorited: false,
        updated_at: 1700000000,
        active_time: 1700000000,
        file_transfer_status: Some("pending".to_string()),
        file_transfer_reason: None,
        link_urls: None,
        link_domains: None,
        file_sizes: None,
    };
    let value = serde_json::to_value(&entry).expect("serialize failed");
    assert_eq!(value["file_transfer_status"], "pending");
    // file_transfer_reason should be absent (skip_serializing_if = None)
    assert!(
        value.get("file_transfer_reason").is_none(),
        "expected file_transfer_reason to be omitted when None"
    );
}

#[test]
fn clipboard_entry_projection_includes_failure_reason_when_failed() {
    let entry = ClipboardEntryProjection {
        id: "file-entry-2".to_string(),
        preview: "photo.jpg".to_string(),
        has_detail: false,
        size_bytes: 2048,
        captured_at: 1700000000,
        content_type: "text/uri-list".to_string(),
        thumbnail_url: None,
        is_encrypted: false,
        is_favorited: false,
        updated_at: 1700000000,
        active_time: 1700000000,
        file_transfer_status: Some("failed".to_string()),
        file_transfer_reason: Some("hash mismatch".to_string()),
        link_urls: None,
        link_domains: None,
        file_sizes: None,
    };
    let value = serde_json::to_value(&entry).expect("serialize failed");
    assert_eq!(value["file_transfer_status"], "failed");
    assert_eq!(value["file_transfer_reason"], "hash mismatch");
}

#[test]
fn clipboard_entry_projection_omits_transfer_fields_for_non_file_entry() {
    let entry = ClipboardEntryProjection {
        id: "text-entry-1".to_string(),
        preview: "hello world".to_string(),
        has_detail: true,
        size_bytes: 11,
        captured_at: 1700000000,
        content_type: "text/plain".to_string(),
        thumbnail_url: None,
        is_encrypted: false,
        is_favorited: false,
        updated_at: 1700000000,
        active_time: 1700000000,
        file_transfer_status: None,
        file_transfer_reason: None,
        link_urls: None,
        link_domains: None,
        file_sizes: None,
    };
    let value = serde_json::to_value(&entry).expect("serialize failed");
    // Both transfer fields should be omitted for non-file entries
    assert!(
        value.get("file_transfer_status").is_none(),
        "expected file_transfer_status to be omitted for non-file entry"
    );
    assert!(
        value.get("file_transfer_reason").is_none(),
        "expected file_transfer_reason to be omitted for non-file entry"
    );
}

#[test]
fn file_transfer_status_payload_serializes_camel_case() {
    // Test without reason (should be omitted due to skip_serializing_if)
    let payload = FileTransferStatusPayload {
        transfer_id: "tf-1".to_string(),
        entry_id: "entry-1".to_string(),
        status: "pending".to_string(),
        reason: None,
    };
    let value = serde_json::to_value(&payload).expect("serialize failed");

    // Fields must be camelCase
    assert!(
        value.get("transferId").is_some(),
        "expected 'transferId' (camelCase) in JSON, got: {value}"
    );
    assert!(
        value.get("entryId").is_some(),
        "expected 'entryId' (camelCase) in JSON, got: {value}"
    );
    // Snake_case variants must NOT be present
    assert!(
        value.get("transfer_id").is_none(),
        "unexpected snake_case 'transfer_id' in JSON: {value}"
    );
    assert!(
        value.get("entry_id").is_none(),
        "unexpected snake_case 'entry_id' in JSON: {value}"
    );
    // reason must be omitted when None (skip_serializing_if)
    assert!(
        value.get("reason").is_none(),
        "expected 'reason' to be omitted when None, got: {value}"
    );
    assert_eq!(value["transferId"], "tf-1");
    assert_eq!(value["entryId"], "entry-1");
    assert_eq!(value["status"], "pending");

    // Test with reason (should be included)
    let payload_with_reason = FileTransferStatusPayload {
        transfer_id: "tf-2".to_string(),
        entry_id: "entry-2".to_string(),
        status: "failed".to_string(),
        reason: Some("timeout".to_string()),
    };
    let value2 = serde_json::to_value(&payload_with_reason).expect("serialize failed");
    assert_eq!(
        value2["reason"], "timeout",
        "expected reason 'timeout' when Some"
    );
    assert_eq!(value2["status"], "failed");
}

#[test]
fn get_settings_response_has_expected_fields() {
    let settings = Settings::default();
    let json = serde_json::to_value(&settings).expect("serialize Settings");

    // Verify top-level fields are present (contract stability check)
    assert!(
        json.get("general").is_some(),
        "expected 'general' field in Settings JSON"
    );
    assert!(
        json.get("sync").is_some(),
        "expected 'sync' field in Settings JSON"
    );
    assert!(
        json.get("security").is_some(),
        "expected 'security' field in Settings JSON"
    );
    assert!(
        json.get("retention_policy").is_some(),
        "expected 'retention_policy' field in Settings JSON"
    );
    assert!(
        json.get("schema_version").is_some(),
        "expected 'schema_version' field in Settings JSON"
    );
}
