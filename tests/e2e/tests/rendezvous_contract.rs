use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use uc_e2e_tests::LocalRendezvous;

async fn issue(server: &LocalRendezvous, ticket: &str, length: Option<Value>) -> Value {
    let mut body = json!({
        "sponsorDeviceId": "test-sponsor",
        "sponsorDeviceName": "Test Sponsor",
        "sponsorEndpointId": "test-endpoint",
        "sponsorTicket": ticket,
        "ttlSecs": 300,
    });
    if let Some(length) = length {
        body["codeLength"] = length;
    }
    Client::new()
        .post(format!("{}/v1/pairings", server.uri()))
        .json(&body)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap()
}

async fn lookup(server: &LocalRendezvous, code: &str) -> reqwest::Response {
    Client::new()
        .post(format!("{}/v1/pairings/resolve", server.uri()))
        .json(&json!({ "code": code }))
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn six_digit_invitation_preserves_leading_zeroes_and_resolves_exactly() {
    let server = LocalRendezvous::start().await;
    let issued = issue(&server, "opaque-six-digit-ticket", Some(json!(6))).await;
    let code = issued["code"].as_str().unwrap();
    assert_eq!(code, "000-001", "honor the client's six-digit code request");
    let resolved: Value = lookup(&server, code)
        .await
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(resolved["sponsorTicket"], "opaque-six-digit-ticket");
}

#[tokio::test]
async fn legacy_requested_and_default_lengths_remain_supported() {
    let server = LocalRendezvous::start().await;
    for length in [None, Some(json!(8))] {
        let issued = issue(&server, "opaque-legacy-ticket", length).await;
        let code = issued["code"].as_str().unwrap();
        let parts: Vec<_> = code.split('-').collect();
        assert_eq!(parts.len(), 2);
        assert!(parts
            .iter()
            .all(|part| part.len() == 4 && part.bytes().all(|b| b.is_ascii_digit())));
        let resolved: Value = lookup(&server, code)
            .await
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(resolved["sponsorTicket"], "opaque-legacy-ticket");
    }
}

#[tokio::test]
async fn unsupported_code_lengths_are_rejected() {
    let server = LocalRendezvous::start().await;
    for length in [
        json!(0),
        json!(5),
        json!(7),
        json!(9),
        json!(100),
        json!("6"),
        Value::Null,
    ] {
        let response = Client::new()
            .post(format!("{}/v1/pairings", server.uri()))
            .json(&json!({ "sponsorTicket": "opaque-ticket", "codeLength": length }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "invalid code length: {length}"
        );
    }
}

#[tokio::test]
async fn invitations_remain_distinct_and_consumption_only_removes_the_selected_code() {
    let server = LocalRendezvous::start().await;
    let first = issue(&server, "first-ticket", Some(json!(6))).await;
    let second = issue(&server, "second-ticket", Some(json!(6))).await;
    let first_code = first["code"].as_str().unwrap();
    let second_code = second["code"].as_str().unwrap();
    assert_ne!(first_code, second_code);
    assert_eq!(
        lookup(&server, "not-issued").await.status(),
        StatusCode::NOT_FOUND
    );
    let consumed = Client::new()
        .post(format!("{}/v1/pairings/consume", server.uri()))
        .json(&json!({ "code": first_code }))
        .send()
        .await
        .unwrap();
    assert_eq!(consumed.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        lookup(&server, first_code).await.status(),
        StatusCode::NOT_FOUND
    );
    let remaining: Value = lookup(&server, second_code)
        .await
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(remaining["sponsorTicket"], "second-ticket");
}
