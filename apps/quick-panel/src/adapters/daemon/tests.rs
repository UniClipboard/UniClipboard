use quick_panel_core::ports::{HistoryService, SearchFailure, ServiceError};
use quick_panel_core::query::filters::Filters;
use serde_json::json;
use uc_daemon_contract::api::auth::DaemonConnectionInfo;
use wiremock::{
    matchers::{header, method, path, query_param},
    Mock, MockServer, ResponseTemplate,
};

use super::search::is_unsearchable;
use super::DaemonHistory;

#[test]
fn an_unsearchable_query_is_no_result_not_a_failure() {
    assert!(is_unsearchable(400, Some("invalid_query")));
    assert!(!is_unsearchable(500, Some("search_failed")));
    assert!(!is_unsearchable(400, Some("bad_request")));
    assert!(!is_unsearchable(400, None));
}

#[test]
fn visual_fixture_settings_match_the_daemon_contract() {
    let settings: uc_daemon_contract::api::dto::settings::SettingsDto =
        serde_json::from_str(include_str!("../../../tests/settings.json")).unwrap();
    assert!(settings.quick_panel.enabled);
    assert!(!settings.general.telemetry_enabled);
}

fn connection(port: u16, token: &str) -> DaemonConnectionInfo {
    DaemonConnectionInfo {
        base_url: format!("http://127.0.0.1:{port}"),
        ws_url: format!("ws://127.0.0.1:{port}/ws"),
        token: token.into(),
        pid: 1,
    }
}

#[tokio::test]
async fn authenticated_search_restore_and_private_errors() {
    let server = MockServer::start().await;
    Mock::given(method("POST")).and(path("/auth/connect"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": { "sessionToken": "test-session", "expiresInSecs": 3600, "refreshAtSecs": 3000 }, "ts": 0
        }))).expect(1).mount(&server).await;
    Mock::given(method("GET"))
        .and(path("/search/query"))
        .and(header("authorization", "Session test-session"))
        .and(query_param("query", "中文 search"))
        .and(query_param("limit", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": { "items": [{
                "entryId": "test-entry", "contentType": "text", "activeTimeMs": 0,
                "tags": [], "textPreview": "Test", "charCount": 4, "mimeType": "text/plain",
                "fileExtensions": [], "fileNames": [], "filePaths": [], "linkUrls": [],
                "sourceDevice": null, "payloadState": null
            }], "total": 1, "hasMore": false, "state": "ready" }, "ts": 0
        })))
        .expect(1)
        .mount(&server)
        .await;
    let today = chrono::Local::now().date_naive();
    let (from_ms, to_ms) = quick_panel_core::query::date_range::parse("9.1-9.15", today)
        .unwrap()
        .bounds_ms(today);
    let items: Vec<_> = (0..2)
        .map(|index| {
            json!({
                "entryId": format!("multi-{index}"), "contentType": "image", "activeTimeMs": 0,
                "tags": if index == 0 { vec!["favorited", "工作"] } else { vec!["favorited"] },
                "textPreview": null, "charCount": null, "mimeType": "image/png",
                "fileExtensions": [], "fileNames": [], "filePaths": [], "linkUrls": [],
                "sourceDevice": null, "payloadState": null
            })
        })
        .collect();
    Mock::given(method("GET"))
        .and(path("/search/query"))
        .and(query_param("query", "multi"))
        .and(query_param("contentTypes", "image,html"))
        .and(query_param("tags", "favorited,工作"))
        .and(query_param("sourceDevices", "phone,laptop"))
        .and(query_param("fromMs", from_ms.to_string()))
        .and(query_param("toMs", to_ms.to_string()))
        .and(query_param("offset", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {"items":items, "total":2, "hasMore":false, "state":"ready"}, "ts":0
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/clipboard/restore/test-entry"))
        .and(header("authorization", "Session test-session"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    for (verb, route, code) in [
        ("GET", "/search/query", 503),
        ("POST", "/clipboard/restore/missing", 404),
    ] {
        let mut mock = Mock::given(method(verb)).and(path(route));
        if verb == "GET" {
            mock = mock.and(query_param("query", "error"));
        }
        mock.respond_with(ResponseTemplate::new(code).set_body_json(json!({
            "error": { "code": "unavailable", "message": "PRIVATE_REMOTE_DETAIL" }
        })))
        .expect(1)
        .mount(&server)
        .await;
    }
    let address = server.uri();
    let history = DaemonHistory::with_resolver(move || {
        Ok(DaemonConnectionInfo {
            base_url: address.clone(),
            ws_url: address.replacen("http://", "ws://", 1) + "/ws",
            token: "test-token".into(),
            pid: 1,
        })
    });
    let search = |query: &str| {
        history.search(Filters {
            query: query.into(),
            ..Default::default()
        })
    };
    let results = search("中文 search").await.unwrap();
    assert_eq!(results.items.len(), 1);
    assert_eq!(results.items[0].entry_id, "test-entry");
    assert_eq!(results.total, 1);
    // Several values of one dimension go to the daemon in a single request, which matches any of
    // them; the panel does not narrow the result any further.
    let results = history
        .search(Filters {
            query: "multi".into(),
            types: vec!["image".into(), "richtext".into()],
            tags: vec!["favorited".into(), "工作".into()],
            sources: vec!["phone".into(), "laptop".into()],
            time: quick_panel_core::query::date_range::parse(
                "9.1-9.15",
                chrono::Local::now().date_naive(),
            ),
        })
        .await
        .unwrap();
    assert_eq!(results.total, 2);
    assert_eq!(results.items.len(), 2);
    assert_eq!(results.items[0].entry_id, "multi-0");
    assert!(!results.has_more);
    history.restore("test-entry".into(), false).await.unwrap();
    let failure = search("error").await.unwrap_err();
    assert_eq!(failure, SearchFailure::Failed);
    assert!(failure.to_string().contains("搜索失败"));
    assert!(!failure.to_string().contains("PRIVATE_REMOTE_DETAIL"));
    let failure = history.restore("missing".into(), false).await.unwrap_err();
    assert_eq!(failure, ServiceError::RestoreFailed);
    assert!(failure.to_string().contains("无法复制"));
    assert!(!failure.to_string().contains("PRIVATE_REMOTE_DETAIL"));
}

#[test]
fn shared_context_is_reused_until_the_daemon_connection_changes() {
    let history = DaemonHistory::with_resolver(|| Err(anyhow::anyhow!("not used")));
    let first = connection(48001, "token-a");
    history.context_for(first.clone()).unwrap();
    assert!(history.cached_connection() == Some(first.clone()));
    // The same connection keeps the cached context (and its session token).
    history.context_for(first.clone()).unwrap();
    assert!(history.cached_connection() == Some(first));
    // A restarted daemon has a new address and token, so the cache must follow it.
    let restarted = connection(48002, "token-b");
    history.context_for(restarted.clone()).unwrap();
    assert!(history.cached_connection() == Some(restarted));
}

#[tokio::test]
async fn a_missing_daemon_is_a_disconnected_search_and_a_readable_error() {
    let history = DaemonHistory::with_resolver(|| Err(anyhow::anyhow!("no connection file")));
    assert_eq!(
        history.search(Filters::default()).await.unwrap_err(),
        SearchFailure::Disconnected
    );
    assert_eq!(
        history.restore("x".into(), false).await.unwrap_err(),
        ServiceError::NotRunning
    );
}

#[tokio::test]
#[ignore = "Requires an unlocked running daemon for the selected UC_PROFILE"]
async fn live_daemon_search() {
    let result = DaemonHistory::new()
        .search(Filters::default())
        .await
        .unwrap();
    assert!(result.items.len() <= 100);
    println!(
        "Live daemon search succeeded: {} results, total {}",
        result.items.len(),
        result.total
    );
}
