//! Serialization tests for command-layer DTO models.
//! 命令层 DTO 模型序列化测试。

use uc_app::usecases::LifecycleState;
use uc_core::settings::model::Settings;
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
        link_urls: None,
        link_domains: None,
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
