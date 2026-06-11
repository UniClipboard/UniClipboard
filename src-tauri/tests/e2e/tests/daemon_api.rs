//! E2E tests that exercise the daemon HTTP API directly (bypassing CLI).
//!
//! These tests verify the API envelope format, health endpoint details,
//! and endpoints that are hard to reach through CLI alone.
//!
//! Run with: cargo test -p uc-e2e-tests -- --ignored

use serde_json::Value;
use uc_e2e_tests::{get_session_token, TestCli, TestDaemon, TestProfile};

struct ApiTestContext {
    daemon: TestDaemon,
    #[allow(dead_code)]
    cli: TestCli,
    client: reqwest::Client,
    session_token: String,
}

impl ApiTestContext {
    fn auth_header(&self) -> String {
        format!("Session {}", self.session_token)
    }
}

async fn setup_with_client(name: &str) -> ApiTestContext {
    let profile = TestProfile::new(name);
    let daemon = TestDaemon::start(profile).await.expect("daemon start");
    let cli = TestCli::new(&daemon.profile);

    let out = cli.run_capture(&[
        "init",
        "--passphrase",
        "api-test-pass",
        "--device-name",
        "api-node",
    ]);
    assert!(out.success(), "init failed: {}", out.stderr);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();

    let session_token = get_session_token(&daemon, &client).await;

    ApiTestContext {
        daemon,
        cli,
        client,
        session_token,
    }
}

#[tokio::test]
#[ignore]
async fn test_health_returns_version_info() {
    let profile = TestProfile::new("api-health");
    let daemon = TestDaemon::start(profile).await.expect("daemon start");

    let resp = reqwest::get(format!("{}/health", daemon.base_url()))
        .await
        .expect("health request");
    assert_eq!(resp.status(), 200);

    let body: Value = resp.json().await.expect("health json");
    // ApiEnvelope: { data: { status, packageVersion, ... }, ts }
    let data = body.get("data").unwrap_or(&body);
    assert!(
        data.get("packageVersion").is_some() || data.get("status").is_some(),
        "health response missing packageVersion/status: {body}"
    );
}

#[tokio::test]
#[ignore]
async fn test_clipboard_entries_empty_after_init() {
    let ctx = setup_with_client("api-entries-empty").await;

    let resp = ctx
        .client
        .get(format!("{}/clipboard/entries", ctx.daemon.base_url()))
        .header("Authorization", ctx.auth_header())
        .send()
        .await
        .expect("entries request");

    assert!(
        resp.status().is_success(),
        "entries returned {}",
        resp.status()
    );

    let body: Value = resp.json().await.expect("entries json");
    let data = body.get("data").unwrap_or(&body);
    let empty_vec = vec![];
    let entries = data.as_array().unwrap_or(&empty_vec);
    assert!(
        entries.is_empty(),
        "fresh space should have no entries, got {}",
        entries.len()
    );
}

#[tokio::test]
#[ignore]
async fn test_clipboard_dispatch_via_api() {
    let ctx = setup_with_client("api-dispatch").await;

    let resp = ctx
        .client
        .post(format!("{}/clipboard/dispatch", ctx.daemon.base_url()))
        .header("Authorization", ctx.auth_header())
        .json(&serde_json::json!({
            "text": "hello from API test"
        }))
        .send()
        .await
        .expect("dispatch request");

    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    assert!(status.is_success(), "dispatch returned {status}: {body}");
}

#[tokio::test]
#[ignore]
async fn test_search_status_via_api() {
    let ctx = setup_with_client("api-search-status").await;

    let resp = ctx
        .client
        .get(format!("{}/search/status", ctx.daemon.base_url()))
        .header("Authorization", ctx.auth_header())
        .send()
        .await
        .expect("search status request");

    assert!(
        resp.status().is_success(),
        "search status returned {}",
        resp.status()
    );

    let body: Value = resp.json().await.expect("search status json");
    let data = body.get("data").unwrap_or(&body);
    assert!(
        data.get("state").is_some() || data.get("status").is_some(),
        "search status missing state field: {data}"
    );
}

#[tokio::test]
#[ignore]
async fn test_device_me_via_api() {
    let ctx = setup_with_client("api-device-me").await;

    let resp = ctx
        .client
        .get(format!("{}/device/me", ctx.daemon.base_url()))
        .header("Authorization", ctx.auth_header())
        .send()
        .await
        .expect("device/me request");

    assert!(
        resp.status().is_success(),
        "device/me returned {}",
        resp.status()
    );

    let body: Value = resp.json().await.expect("device/me json");
    let data = body.get("data").unwrap_or(&body);
    let name = data
        .get("deviceName")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        name.contains("api-node"),
        "device name should contain 'api-node', got '{name}'. full body: {body}"
    );
}

#[tokio::test]
#[ignore]
async fn test_settings_get_via_api() {
    let ctx = setup_with_client("api-settings").await;

    let resp = ctx
        .client
        .get(format!("{}/settings", ctx.daemon.base_url()))
        .header("Authorization", ctx.auth_header())
        .send()
        .await
        .expect("settings request");

    assert!(
        resp.status().is_success(),
        "settings returned {}",
        resp.status()
    );

    let body: Value = resp.json().await.expect("settings json");
    assert!(
        body.get("data").is_some() || body.get("ts").is_some(),
        "settings response not in ApiEnvelope format: {body}"
    );
}
