use std::time::Duration;

use uc_daemon_client::DaemonClientContext;
use uc_daemon_contract::api::auth::DaemonConnectionInfo;
use uc_daemon_contract::api::dto::search::SearchQueryResultDto;

#[derive(Debug)]
pub enum SearchFailure {
    Locked,
    Failed,
    /// The daemon cannot be reached at all: no connection info, a refused connection or a failed
    /// authorization. Unlike `Failed` it is worth retrying by itself.
    Disconnected,
    Unavailable(String),
    Timeout,
}

impl std::fmt::Display for SearchFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Locked => "剪贴板已锁定，请解锁后继续。",
            Self::Failed => "搜索失败，请重试。",
            Self::Disconnected => "同步服务未响应。",
            Self::Unavailable(message) => message,
            Self::Timeout => "搜索超时，请重试。",
        })
    }
}

/// Process-wide daemon context. Every operation shares one connection state so
/// the session token obtained from `/auth/connect` is cached across requests
/// instead of being exchanged again for each search, preview or action.
static SHARED_CONTEXT: std::sync::Mutex<Option<(DaemonConnectionInfo, DaemonClientContext)>> =
    std::sync::Mutex::new(None);

async fn context() -> Result<DaemonClientContext, String> {
    let connection = uc_daemon_client::resolve_connection_info_from_env()
        .map_err(|_| "无法连接 UniClipboard，请先启动并解锁桌面应用。".to_string())?;
    shared_context(connection)
}

/// Returns the shared context, rebuilding it only when the resolved connection
/// (address or bearer token) differs from the one it was built for, for example
/// after the daemon restarts. A rebuild starts a fresh session-token cache.
fn shared_context(connection: DaemonConnectionInfo) -> Result<DaemonClientContext, String> {
    let mut shared = SHARED_CONTEXT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((known, context)) = shared.as_ref() {
        if *known == connection {
            return Ok(context.clone());
        }
    }
    let context = DaemonClientContext::new(connection.clone())
        .map_err(|_| "无法创建后台连接。".to_string())?;
    *shared = Some((connection, context.clone()));
    Ok(context)
}

#[cfg(test)]
pub async fn search(query: String) -> Result<SearchQueryResultDto, SearchFailure> {
    search_filtered(crate::filters::Filters {
        query,
        ..Default::default()
    })
    .await
}

pub async fn search_filtered(
    filters: crate::filters::Filters,
) -> Result<SearchQueryResultDto, SearchFailure> {
    let operation = async {
        let client = context()
            .await
            .map_err(|_| SearchFailure::Disconnected)?
            .search_client();
        client
            .query(filters.request())
            .await
            .map_err(search_failure)
    };
    tokio::time::timeout(Duration::from_secs(8), operation)
        .await
        .map_err(|_| SearchFailure::Timeout)?
}

/// How many entries match, without fetching them; used to size the suggestions to relax a search.
pub async fn count(filters: crate::filters::Filters) -> Option<u32> {
    let mut request = filters.request();
    request.limit = 1;
    let operation = async {
        let client = context().await.ok()?.search_client();
        client.query(request).await.ok().map(|result| result.total)
    };
    tokio::time::timeout(Duration::from_secs(8), operation)
        .await
        .ok()
        .flatten()
}

fn search_failure(error: anyhow::Error) -> SearchFailure {
    use uc_daemon_client::DaemonRequestError as Error;
    if matches!(
        error.downcast_ref::<Error>(),
        Some(Error::NotConnected | Error::Transport { .. } | Error::Auth { .. })
    ) {
        return SearchFailure::Disconnected;
    }
    if error
        .downcast_ref::<uc_daemon_client::DaemonRequestError>()
        .and_then(|e| e.status())
        .is_some_and(|s| s.as_u16() == 423)
    {
        SearchFailure::Locked
    } else {
        SearchFailure::Failed
    }
}

#[cfg(test)]
pub async fn restore(id: String) -> Result<(), String> {
    restore_with_options(id, false).await
}

pub async fn restore_with_options(id: String, plain: bool) -> Result<(), String> {
    let operation = async {
        context()
            .await?
            .clipboard_client()
            .restore_clipboard_entry_with_options(&id, plain)
            .await
            .map_err(|_| "无法复制这条记录，请刷新后重试。".to_string())
    };
    tokio::time::timeout(Duration::from_secs(8), operation)
        .await
        .map_err(|_| "复制超时，请重试。".to_string())?
}

pub struct Options {
    pub tags: Vec<String>,
    pub members: Vec<uc_daemon_contract::api::types::SpaceMemberDto>,
    pub settings: Option<uc_daemon_contract::api::dto::settings::SettingsDto>,
}

pub async fn options() -> Result<Options, String> {
    let context = context().await?;
    let mut tags: Vec<String> = crate::filters::BUILTIN_TAGS
        .iter()
        .map(|s| (*s).into())
        .collect();
    let fetched = context
        .search_client()
        .tags()
        .await
        .map_err(|_| "无法读取标签。")?;
    for tag in fetched {
        if !tags.contains(&tag.tag_id) {
            tags.push(tag.tag_id);
        }
    }
    let members = context
        .query_client()
        .get_paired_devices()
        .await
        .map_err(|_| "无法读取设备。")?;
    let settings = context.settings_client().get_settings().await.ok();
    Ok(Options {
        tags,
        members,
        settings,
    })
}

#[derive(Clone)]
pub enum EntryAction {
    Favorite(bool),
    Delete,
    Send(Option<String>),
}

pub async fn action(id: String, action: EntryAction) -> Result<(), String> {
    let context = context().await?;
    let result = match action {
        EntryAction::Favorite(value) => context.clipboard_client().set_favorite(&id, value).await,
        EntryAction::Delete => context.clipboard_client().delete_entry(&id).await,
        EntryAction::Send(peer) => context
            .clipboard_client()
            .resend_entry(&id, peer.map(|p| vec![p]))
            .await
            .map(|_| ()),
    };
    result.map_err(|_| "操作失败，请重试。".into())
}

pub struct Preview {
    pub text: Option<String>,
    pub image: Option<(Vec<u8>, String, u32, u32)>,
    pub size: i64,
}

pub async fn preview(id: String, kind: String) -> Result<Preview, String> {
    tokio::time::timeout(Duration::from_secs(10), preview_inner(id, kind))
        .await
        .map_err(|_| "预览加载超时。".to_string())?
}

async fn preview_inner(id: String, kind: String) -> Result<Preview, String> {
    use base64::Engine;
    let client = context().await?.clipboard_client();
    if kind == "text" || kind == "richtext" {
        let detail = client
            .entry_detail(&id)
            .await
            .map_err(|_| "无法读取预览。")?
            .ok_or("内容已不可用。")?;
        return Ok(Preview {
            text: Some(detail.content),
            image: None,
            size: detail.size_bytes,
        });
    }
    if kind == "image" {
        let resource = client
            .entry_resource(&id)
            .await
            .map_err(|_| "无法读取图片。")?
            .ok_or("图片已不可用。")?;
        let bytes = if let Some(inline) = resource.inline_data {
            base64::engine::general_purpose::STANDARD
                .decode(inline)
                .map_err(|_| "图片格式错误。")?
        } else if let Some(blob) = resource.blob_id {
            client
                .fetch_blob(&blob)
                .await
                .map_err(|_| "无法读取图片。")?
                .ok_or("图片已不可用。")?
        } else {
            return Err("图片已不可用。".into());
        };
        let format = image::guess_format(&bytes).map_err(|_| "图片格式不支持。")?;
        let reader = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format);
        let (width, height) = reader.into_dimensions().map_err(|_| "图片无法解码。")?;
        return Ok(Preview {
            text: None,
            image: Some((bytes, format.to_mime_type().into(), width, height)),
            size: resource.size_bytes,
        });
    }
    Ok(Preview {
        text: None,
        image: None,
        size: 0,
    })
}

pub async fn watch_changes(send: tokio::sync::mpsc::Sender<()>) {
    use std::sync::Arc;
    use uc_daemon_client::{realtime::RealtimeTopic, DaemonWsBridge, DaemonWsBridgeConfig};
    let Ok(context) = context().await else {
        return;
    };
    let state = context.connection_state();
    let bridge = Arc::new(DaemonWsBridge::new(
        state.clone(),
        DaemonWsBridgeConfig::default(),
    ));
    let mut events = match bridge
        .subscribe(
            "gpui-quick-panel",
            &[
                RealtimeTopic::Clipboard,
                RealtimeTopic::FileTransfer,
                RealtimeTopic::Peers,
                RealtimeTopic::Setup,
            ],
        )
        .await
    {
        Ok(events) => events,
        Err(_) => {
            tracing::warn!("Quick panel realtime subscription failed");
            return;
        }
    };
    let stop = tokio_util::sync::CancellationToken::new();
    let run = bridge.run(stop.clone());
    tokio::pin!(run);
    let mut discovery = tokio::time::interval(Duration::from_secs(2));
    loop {
        tokio::select! {
            _=send.closed()=>break,
            _=&mut run=>break,
            _=discovery.tick()=>{if let Ok(connection)=uc_daemon_client::resolve_connection_info_from_env(){state.set(connection);}},
            event=events.recv()=>{
                if event.is_none(){break;}
                if send.try_send(()).is_err()&&send.is_closed(){break;}
            },
        }
    }
    stop.cancel();
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::{
        matchers::{header, method, path, query_param},
        Mock, MockServer, ResponseTemplate,
    };

    #[test]
    fn visual_fixture_settings_match_the_daemon_contract() {
        let settings: uc_daemon_contract::api::dto::settings::SettingsDto =
            serde_json::from_str(include_str!("../tests/settings.json")).unwrap();
        assert!(settings.quick_panel.enabled);
        assert!(!settings.general.telemetry_enabled);
    }

    #[tokio::test]
    async fn authenticated_search_restore_and_private_errors() {
        const CHILD: &str = "UC_GPUI_PROTOCOL_TEST_CHILD";
        if std::env::var_os(CHILD).is_some() {
            let results = search("中文 search".into()).await.unwrap();
            assert_eq!(results.items.len(), 1);
            assert_eq!(results.items[0].entry_id, "test-entry");
            assert_eq!(results.total, 1);
            // Several values of one dimension go to the daemon in a single request, which
            // matches any of them; the panel does not narrow the result any further.
            let results = search_filtered(crate::filters::Filters {
                query: "multi".into(),
                types: vec!["image".into(), "richtext".into()],
                tags: vec!["favorited".into(), "工作".into()],
                sources: vec!["phone".into(), "laptop".into()],
                time: crate::date_range::parse("9.1-9.15", chrono::Local::now().date_naive()),
            })
            .await
            .unwrap();
            assert_eq!(results.total, 2);
            assert_eq!(results.items.len(), 2);
            assert_eq!(results.items[0].entry_id, "multi-0");
            assert!(!results.has_more);
            restore("test-entry".into()).await.unwrap();
            let failure = search("error".into()).await.unwrap_err().to_string();
            assert!(failure.contains("搜索失败"));
            assert!(!failure.contains("PRIVATE_REMOTE_DETAIL"));
            let failure = restore("missing".into()).await.unwrap_err();
            assert!(failure.contains("无法复制"));
            assert!(!failure.contains("PRIVATE_REMOTE_DETAIL"));
            return;
        }
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
        let (from_ms, to_ms) = crate::date_range::parse("9.1-9.15", today)
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
        let token = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(token.path(), "test-token").unwrap();
        let executable = std::env::current_exe().unwrap();
        let address = server.uri();
        let output = tokio::task::spawn_blocking(move || {
            std::process::Command::new(executable)
                .args([
                    "--exact",
                    "backend::tests::authenticated_search_restore_and_private_errors",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .env("UNICLIPBOARD_DAEMON_BASE_URL", address)
                .env("UNICLIPBOARD_DAEMON_TOKEN_PATH", token.path())
                .output()
                .unwrap()
        })
        .await
        .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn connection(port: u16, token: &str) -> DaemonConnectionInfo {
        DaemonConnectionInfo {
            base_url: format!("http://127.0.0.1:{port}"),
            ws_url: format!("ws://127.0.0.1:{port}/ws"),
            token: token.into(),
            pid: 1,
        }
    }

    fn cached_connection() -> Option<DaemonConnectionInfo> {
        SHARED_CONTEXT
            .lock()
            .unwrap()
            .as_ref()
            .map(|(known, _)| known.clone())
    }

    #[test]
    fn shared_context_is_reused_until_the_daemon_connection_changes() {
        let first = connection(48001, "token-a");
        shared_context(first.clone()).unwrap();
        assert!(cached_connection() == Some(first.clone()));
        // The same connection keeps the cached context (and its session token).
        shared_context(first.clone()).unwrap();
        assert!(cached_connection() == Some(first));
        // A restarted daemon has a new address and token, so the cache must follow it.
        let restarted = connection(48002, "token-b");
        shared_context(restarted.clone()).unwrap();
        assert!(cached_connection() == Some(restarted));
    }

    #[tokio::test]
    #[ignore = "Requires an unlocked running daemon for the selected UC_PROFILE"]
    async fn live_daemon_search() {
        let result = search(String::new()).await.unwrap();
        assert!(result.items.len() <= 100);
        println!(
            "Live daemon search succeeded: {} results, total {}",
            result.items.len(),
            result.total
        );
    }
}
