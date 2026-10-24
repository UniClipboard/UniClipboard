//! E2E tests for the receiver-side per-item receive API against a real daemon.
//!
//! A single isolated node cannot produce a partially delivered receive; that
//! needs two paired devices and is recorded as not-run in the acceptance gaps.
//! These tests pin the parts of the contract a single real daemon can prove:
//! the selective retry route exists, validates its input before touching the
//! Engine, never claims `accepted` for a receive that does not exist, and the
//! progress snapshot keeps the aggregate fields next to the new `items` array.
//!
//! Run with: cargo test -p uc-e2e-tests --test entry_receive -- --ignored

use serde_json::{json, Value};
use uc_e2e_tests::{get_session_token, TestCli, TestDaemon, TestProfile};

struct Node {
    daemon: TestDaemon,
    client: reqwest::Client,
    auth: String,
}

async fn start_node(name: &str) -> Node {
    let profile = TestProfile::new(name);
    let daemon = TestDaemon::start(profile).await.expect("daemon start");
    let cli = TestCli::new(&daemon.profile);
    let init = cli.run_capture(&[
        "init",
        "--passphrase",
        "entry-receive-pass",
        "--device-name",
        "entry-receive-node",
    ]);
    assert!(init.success(), "init failed: {}", init.stderr);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("http client");
    let token = get_session_token(&daemon, &client).await;
    Node {
        daemon,
        client,
        auth: format!("Session {token}"),
    }
}

impl Node {
    async fn retry(&self, entry_id: &str, body: Value) -> (reqwest::StatusCode, Value) {
        let response = self
            .client
            .post(format!(
                "{}/clipboard/entries/{entry_id}/receive/retry",
                self.daemon.base_url()
            ))
            .header("Authorization", &self.auth)
            .json(&body)
            .send()
            .await
            .expect("retry request");
        let status = response.status();
        let body = response.json().await.unwrap_or(Value::Null);
        (status, body)
    }
}

/// An empty selection is rejected by the contract before any receive state is
/// consulted. A missing route (404/405) is the failing baseline.
#[tokio::test]
#[ignore]
async fn retry_rejects_empty_item_selection() {
    let node = start_node("entry-receive-empty").await;
    let (status, body) = node
        .retry(
            "00000000-0000-0000-0000-000000000001",
            json!({ "expectedAttemptId": "attempt-1", "itemIds": [] }),
        )
        .await;
    assert_eq!(status.as_u16(), 400, "empty selection must be 400: {body}");
}

/// A body without the expected attempt is a malformed request, not a retry.
#[tokio::test]
#[ignore]
async fn retry_rejects_missing_expected_attempt() {
    let node = start_node("entry-receive-no-attempt").await;
    let (status, body) = node
        .retry(
            "00000000-0000-0000-0000-000000000001",
            json!({ "itemIds": ["item-1"] }),
        )
        .await;
    assert_eq!(
        status.as_u16(),
        400,
        "missing expectedAttemptId must be 400: {body}"
    );
}

/// With no receive for the entry, selective retry must not report `accepted`:
/// `accepted` means a new attempt was admitted, which cannot be true here. The
/// Engine answers `unavailable` / `source_unavailable`, the same answer a restart
/// gives when the original retry material is gone.
#[tokio::test]
#[ignore]
async fn retry_without_a_receive_is_never_accepted() {
    let node = start_node("entry-receive-none").await;
    let (status, body) = node
        .retry(
            "00000000-0000-0000-0000-000000000001",
            json!({ "expectedAttemptId": "attempt-1", "itemIds": ["item-1"] }),
        )
        .await;
    assert_eq!(status.as_u16(), 200, "retry route must answer: {body}");
    // Observed on the real daemon: no retry material means `source_unavailable`.
    assert_eq!(body["data"]["outcome"], "unavailable", "{body}");
    assert_eq!(body["data"]["reason"], "source_unavailable", "{body}");
    assert!(
        body["data"].get("attemptId").map_or(true, Value::is_null),
        "only an accepted retry carries a new attempt id: {body}"
    );
}

/// The receive list stays an array of aggregate rows; an idle daemon has none.
/// Cancelling a receive that does not exist is `not_receiving`, never
/// `cancelled` or `cancellation_incomplete`.
#[tokio::test]
#[ignore]
async fn idle_node_reports_no_receives_and_cancel_is_not_receiving() {
    let node = start_node("entry-receive-idle").await;
    let list = node
        .client
        .get(format!("{}/clipboard/receives", node.daemon.base_url()))
        .header("Authorization", &node.auth)
        .send()
        .await
        .expect("list receives");
    assert!(list.status().is_success());
    let body: Value = list.json().await.expect("list json");
    assert_eq!(body["data"], json!([]), "idle node has no receives: {body}");

    let cancel = node
        .client
        .post(format!(
            "{}/clipboard/entries/00000000-0000-0000-0000-000000000001/receive/cancel",
            node.daemon.base_url()
        ))
        .header("Authorization", &node.auth)
        .json(&json!({ "attemptId": "attempt-1" }))
        .send()
        .await
        .expect("cancel request");
    assert!(cancel.status().is_success(), "cancel: {}", cancel.status());
    let body: Value = cancel.json().await.expect("cancel json");
    assert_eq!(body["data"]["outcome"], "not_receiving", "{body}");
}

/// The published OpenAPI document carries the retry operation and the item
/// schema, so generated clients cannot drift from the daemon.
#[tokio::test]
#[ignore]
async fn openapi_publishes_retry_and_item_schema() {
    let node = start_node("entry-receive-openapi").await;
    let response = node
        .client
        .get(format!("{}/api-docs/openapi.json", node.daemon.base_url()))
        .header("Authorization", &node.auth)
        .send()
        .await
        .expect("openapi request");
    assert!(response.status().is_success(), "openapi: {}", response.status());
    let doc: Value = response.json().await.expect("openapi json");
    let retry = &doc["paths"]["/clipboard/entries/{id}/receive/retry"]["post"];
    assert_eq!(retry["operationId"], "retryEntryReceive", "{retry}");
    let progress = &doc["components"]["schemas"]["EntryReceiveProgressResponse"]["properties"];
    for aggregate in [
        "entryId",
        "attemptId",
        "state",
        "totalBytes",
        "completedBytes",
        "itemsTotal",
        "itemsCompleted",
    ] {
        assert!(progress.get(aggregate).is_some(), "aggregate {aggregate} kept");
    }
    assert!(progress.get("items").is_some(), "items array published");
    let item = &doc["components"]["schemas"]["ReceiveItemResponse"]["properties"];
    for field in ["itemId", "kind", "state", "failureReason"] {
        assert!(item.get(field).is_some(), "item field {field} published");
    }
}
