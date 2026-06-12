//! Daemon HTTP auth helpers for tests that hit the API directly.
//!
//! The daemon writes a file token under the profile data dir; tests exchange
//! it for a JWT session token via `POST /auth/connect` and send that as
//! `Authorization: Session <token>` on subsequent API calls.

use std::time::Duration;

use serde_json::Value;

use crate::TestDaemon;

/// Read the daemon's file token from the profile data dir, polling briefly
/// until the daemon has written it.
pub fn read_daemon_file_token(daemon: &TestDaemon) -> String {
    let token_path = daemon.profile.data_dir().join(".daemon-token");
    for _ in 0..30 {
        if let Ok(token) = std::fs::read_to_string(&token_path) {
            let trimmed = token.trim().to_string();
            if !trimmed.is_empty() {
                return trimmed;
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!("daemon token not found at {:?}", token_path);
}

/// Exchange the daemon file token for a JWT session token via
/// `POST /auth/connect`.
pub async fn get_session_token(daemon: &TestDaemon, client: &reqwest::Client) -> String {
    let file_token = read_daemon_file_token(daemon);

    let resp = client
        .post(format!("{}/auth/connect", daemon.base_url()))
        .header("Authorization", format!("Bearer {file_token}"))
        .json(&serde_json::json!({
            "pid": std::process::id(),
            "clientType": "cli"
        }))
        .send()
        .await
        .expect("auth/connect request");

    assert!(
        resp.status().is_success(),
        "auth/connect failed with {}",
        resp.status()
    );

    let body: Value = resp.json().await.expect("auth/connect json");
    let data = body.get("data").unwrap_or(&body);
    data.get("sessionToken")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("auth/connect response missing sessionToken: {body}"))
        .to_string()
}
