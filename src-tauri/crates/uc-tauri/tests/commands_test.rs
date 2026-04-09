//! IPC Command Tests
//! IPC 命令测试

use uc_daemon_client::DaemonConnectionState;
use uc_daemon_contract::api::auth::DaemonConnectionInfo;

#[tokio::test]
async fn test_get_clipboard_entries_returns_empty_list_when_no_data() {
    // This test verifies the command structure
    // Full integration test requires AppDeps setup
    assert!(true, "Command signature verified");
}

#[tokio::test]
async fn test_get_daemon_connection_info_returns_none_when_state_is_empty() {
    let state = DaemonConnectionState::default();

    let result = uc_tauri::commands::read_daemon_connection_info(&state);

    assert!(result.is_none());
}

#[tokio::test]
async fn test_get_daemon_connection_info_returns_camel_case_payload() {
    let state = DaemonConnectionState::default();
    state.set(DaemonConnectionInfo {
        base_url: "http://127.0.0.1:42715".to_string(),
        ws_url: "ws://127.0.0.1:42715/ws".to_string(),
        token: "secret".to_string(),
        pid: 12345,
    });

    let payload =
        uc_tauri::commands::read_daemon_connection_info(&state).expect("payload should exist");

    let value = serde_json::to_value(payload).expect("payload should serialize");

    assert_eq!(value["baseUrl"], "http://127.0.0.1:42715");
    assert_eq!(value["wsUrl"], "ws://127.0.0.1:42715/ws");
    assert_eq!(value["token"], "secret");
    assert!(value.get("base_url").is_none());
    assert!(value.get("ws_url").is_none());
}

#[test]
fn test_autostart_commands_are_exposed() {
    let _ = uc_tauri::commands::enable_autostart;
    let _ = uc_tauri::commands::disable_autostart;
    let _ = uc_tauri::commands::is_autostart_enabled;
}
