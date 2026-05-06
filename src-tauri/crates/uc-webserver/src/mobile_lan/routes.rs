//! SyncClipboard 协议根路径路由(Phase 3 子步骤 5e/5f)。
//!
//! 替代 Phase 3 子步骤 3 的 `GET /mobile/v1/handshake` stub —— v3 切到
//! SyncClipboard 兼容路径后, iOS shortcut 客户端在用户填的 base URL 后面
//! 拼 `/SyncClipboard.json` / `/file/{name}`, daemon 必须把这 4 条路由挂在
//! 根路径(否则路径前缀对不上)。
//!
//! ## 路由
//!
//! | Method | Path | 业务 |
//! |---|---|---|
//! | GET | `/SyncClipboard.json` | 取最近一次 PUT 的元数据 |
//! | PUT | `/SyncClipboard.json` | 上传新元数据(daemon 自动算 SHA-256 填响应) |
//! | GET | `/file/:dataName` | 取附件原始字节 |
//! | PUT | `/file/:dataName` | 上传附件 |
//!
//! 所有 4 条路由都通过 [`crate::mobile_lan::middleware::basic_auth`] 校验,
//! 不经 middleware 不会到达 handler;500 / 401 由 middleware 自己回, handler
//! 只处理 happy / 404。
//!
//! ## DTO 与应用模型映射
//!
//! `SyncClipboardDoc` 是**wire DTO**(SyncClipboard 协议的 JSON schema),
//! 字段大小写按协议固定:`type` 是 PascalCase value, key 是 camelCase。
//! 内部转换到 [`SyncClipboardMeta`] 应用模型, 再调 facade。
//!
//! `from_meta` / `into_meta` 在本文件内单独定义, 让 webserver 拥有完整的
//! "wire schema 控制权"(`uc-application/AGENTS.md` §6.3 拒绝把 wire DTO
//! 上浮到应用层)。

use std::sync::Arc;

use axum::{
    body::{to_bytes, Body},
    extract::{Path, Request, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use uc_application::facade::{
    MobileSyncFacade, SyncClipboardError, SyncClipboardItemType, SyncClipboardMeta,
};

use crate::mobile_lan::middleware::basic_auth;

/// `PUT /file/{dataName}` 的请求体上限 —— Phase 3 stub 用 16 MiB 兜底
/// (与 SyncClipboard 项目桌面端同档)。Phase 5 接入真实剪贴板存储 + blob
/// store 后这里换成 streaming, 不再一次性 buffer。
const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;

// ─── wire DTO ───────────────────────────────────────────────────────────

/// SyncClipboard 协议的 JSON schema(wire-format)。字段大小写**严格**按
/// SyncClipboard 项目 4 年实战定义来, 修改即兼容性破坏。
///
/// `type` 是 SyncClipboard 自定义关键字 —— Rust 用 `r#type` raw identifier
/// 写, serde rename 到 `type`。
#[derive(Debug, Clone, Deserialize, Serialize)]
struct SyncClipboardDoc {
    #[serde(rename = "type")]
    r#type: String, // PascalCase: "Text" / "Image" / "File" / "Group"
    #[serde(default)]
    text: String,
    #[serde(rename = "dataName", default, skip_serializing_if = "Option::is_none")]
    data_name: Option<String>,
    #[serde(rename = "hasData", default)]
    has_data: bool,
    #[serde(default)]
    size: u64,
    /// SHA-256 hex —— 接收侧可缺省(SyncClipboard shortcut 不上传), 响应侧
    /// daemon 一定填(给 SyncClipboard 桌面端兼容用)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hash: Option<String>,
}

impl SyncClipboardDoc {
    fn from_meta(meta: SyncClipboardMeta) -> Self {
        Self {
            r#type: match meta.item_type {
                SyncClipboardItemType::Text => "Text",
                SyncClipboardItemType::Image => "Image",
                SyncClipboardItemType::File => "File",
                SyncClipboardItemType::Group => "Group",
            }
            .to_string(),
            text: meta.text,
            data_name: meta.data_name,
            has_data: meta.has_data,
            size: meta.size,
            hash: meta.hash,
        }
    }

    fn into_meta(self) -> Result<SyncClipboardMeta, &'static str> {
        let item_type = match self.r#type.as_str() {
            "Text" => SyncClipboardItemType::Text,
            "Image" => SyncClipboardItemType::Image,
            "File" => SyncClipboardItemType::File,
            "Group" => SyncClipboardItemType::Group,
            _ => return Err("unknown SyncClipboard `type` value"),
        };
        Ok(SyncClipboardMeta {
            item_type,
            text: self.text,
            data_name: self.data_name,
            has_data: self.has_data,
            size: self.size,
            hash: self.hash,
        })
    }
}

// ─── handlers ───────────────────────────────────────────────────────────

async fn get_sync_clipboard_json(
    State(facade): State<Arc<MobileSyncFacade>>,
) -> Result<Json<SyncClipboardDoc>, Response> {
    match facade.get_latest_sync_doc().await {
        Ok(meta) => {
            tracing::info!(
                item_type = ?meta.item_type,
                has_data = meta.has_data,
                size = meta.size,
                "GET /SyncClipboard.json: 200"
            );
            Ok(Json(SyncClipboardDoc::from_meta(meta)))
        }
        Err(SyncClipboardError::NotFound) => {
            tracing::info!("GET /SyncClipboard.json: 404 (no doc yet)");
            Err(StatusCode::NOT_FOUND.into_response())
        }
        Err(SyncClipboardError::Internal(msg)) => {
            tracing::warn!(error = %msg, "get_sync_clipboard_json: internal failure");
            Err(StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
    }
}

async fn put_sync_clipboard_json(
    State(facade): State<Arc<MobileSyncFacade>>,
    Json(doc): Json<SyncClipboardDoc>,
) -> Result<Json<SyncClipboardDoc>, Response> {
    let meta = doc
        .into_meta()
        .map_err(|reason| (StatusCode::BAD_REQUEST, reason).into_response())?;

    let item_type = meta.item_type;
    let has_data = meta.has_data;
    let size = meta.size;
    let text_preview_len = meta.text.len();

    match facade.put_sync_doc(meta).await {
        Ok(stored) => {
            tracing::info!(
                item_type = ?item_type,
                has_data,
                size,
                text_len = text_preview_len,
                hash_prefix = stored.hash.as_deref().map(|h| &h[..h.len().min(12)]),
                "PUT /SyncClipboard.json: 200"
            );
            Ok(Json(SyncClipboardDoc::from_meta(stored)))
        }
        Err(SyncClipboardError::NotFound) => {
            // PUT 不应该返回 NotFound;视为 stub 实现错误, 兜底 500。
            Err(StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Err(SyncClipboardError::Internal(msg)) => {
            tracing::warn!(error = %msg, "put_sync_clipboard_json: internal failure");
            Err(StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
    }
}

async fn get_clipboard_file(
    State(facade): State<Arc<MobileSyncFacade>>,
    Path(data_name): Path<String>,
) -> Result<Response, Response> {
    match facade.get_clipboard_file(&data_name).await {
        Ok((mime, bytes)) => {
            tracing::info!(
                data_name = %data_name,
                mime = %mime,
                bytes = bytes.len(),
                "GET /file: 200"
            );
            let mut resp = Response::new(Body::from(bytes));
            *resp.status_mut() = StatusCode::OK;
            resp.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_str(&mime)
                    .unwrap_or(HeaderValue::from_static("application/octet-stream")),
            );
            Ok(resp)
        }
        Err(SyncClipboardError::NotFound) => {
            tracing::info!(data_name = %data_name, "GET /file: 404");
            Err(StatusCode::NOT_FOUND.into_response())
        }
        Err(SyncClipboardError::Internal(msg)) => {
            tracing::warn!(error = %msg, "get_clipboard_file: internal failure");
            Err(StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
    }
}

async fn put_clipboard_file(
    State(facade): State<Arc<MobileSyncFacade>>,
    Path(data_name): Path<String>,
    request: Request,
) -> Result<StatusCode, Response> {
    // mime 走 Content-Type 头;客户端不带就回退 application/octet-stream
    // (与 SyncClipboard shortcut 上传 PNG / RTF 等场景一致)。
    let mime = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    let body_bytes = to_bytes(request.into_body(), MAX_FILE_BYTES)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "put_clipboard_file: failed to buffer body");
            (StatusCode::PAYLOAD_TOO_LARGE, "body too large").into_response()
        })?
        .to_vec();

    let bytes_len = body_bytes.len();
    let log_data_name = data_name.clone();
    let log_mime = mime.clone();
    match facade.put_clipboard_file(data_name, mime, body_bytes).await {
        Ok(()) => {
            tracing::info!(
                data_name = %log_data_name,
                mime = %log_mime,
                bytes = bytes_len,
                "PUT /file: 200"
            );
            Ok(StatusCode::OK)
        }
        Err(SyncClipboardError::NotFound) => {
            // PUT 不应该返回 NotFound, 兜底 500。
            Err(StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Err(SyncClipboardError::Internal(msg)) => {
            tracing::warn!(error = %msg, "put_clipboard_file: internal failure");
            Err(StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
    }
}

// ─── router ─────────────────────────────────────────────────────────────

/// 构造根路径 SyncClipboard 协议路由。daemon listener 把它挂到 axum app 根。
///
/// 所有路由都接 Basic Auth middleware, 未登记 / 未带头 / 凭据错的请求拿
/// 401 + `WWW-Authenticate: Basic` 头。
pub(crate) fn build_router(facade: Arc<MobileSyncFacade>) -> Router {
    Router::new()
        .route(
            "/SyncClipboard.json",
            get(get_sync_clipboard_json).put(put_sync_clipboard_json),
        )
        .route(
            "/file/:data_name",
            get(get_clipboard_file).put(put_clipboard_file),
        )
        .layer(axum::middleware::from_fn_with_state(
            facade.clone(),
            basic_auth,
        ))
        .with_state(facade)
}

#[cfg(test)]
mod tests {
    //! 路由 + middleware 集成测试。覆盖 SPEC §14 的 happy path / 401 / 404
    //! 三类断言, 避免每次改动后要拉真 daemon 跑 curl。
    //!
    //! 测试不直接构造 `MobileSyncFacade`(需要 6 个 ports), 而是搭建一份
    //! 最小 in-memory 装配:
    //! - InMemoryDeviceRepo + InMemorySettings: 满足 facade::new 要求
    //! - 真 Argon2idPasswordHasher 与 OsRngCredentialsMinter: 鉴权链路要 round-trip
    //! - 不需要 endpoint_info / lan_interface_probe (没用到)

    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use base64::engine::general_purpose::STANDARD as BASE64_STD;
    use base64::Engine;
    use std::sync::Mutex;
    use tower::ServiceExt;
    use uc_core::mobile_sync::{LanEndpointInfo, LanInterface, MobileDevice, MobileDeviceError};
    use uc_core::settings::model::Settings;

    use uc_application::facade::{MobileSyncFacade, MobileSyncFacadeDeps};

    fn build_app(facade: Arc<MobileSyncFacade>) -> Router {
        build_router(facade)
    }

    /// 把 5 个最小 fake port 拼好的 facade 拿出来。手动塞一台已知凭据的
    /// 设备让后续测试做"happy path"。
    ///
    /// 这里**不**依赖 uc_infra(webserver crate 边界禁止依赖 uc_infra), 用
    /// 本地 FakeHasher: phc 形态固定为 `phc:<password>`, verify 比较。
    async fn build_facade_with_seeded_device(
        username: &str,
        password: &str,
    ) -> Arc<MobileSyncFacade> {
        use std::net::Ipv4Addr;

        use async_trait::async_trait;
        use uc_core::mobile_sync::{MintedCredentials, MobileDeviceId};
        use uc_core::ports::{
            ClockPort, EndpointInfoError, LanInterfaceProbeError, LanInterfaceProbePort,
            MobileCredentialsMinterPort, MobileDeviceRepositoryPort, MobileSyncEndpointInfoPort,
            PasswordHasherError, PasswordHasherPort, SettingsPort,
        };

        struct FixedClock;
        impl ClockPort for FixedClock {
            fn now_ms(&self) -> i64 {
                1_000
            }
        }

        struct StaticMinter;
        impl MobileCredentialsMinterPort for StaticMinter {
            fn mint_credentials(&self) -> MintedCredentials {
                MintedCredentials {
                    username: "mobile_unused".into(),
                    password: "unused".into(),
                    password_hash: "phc:unused".into(),
                    device_id: MobileDeviceId::new("did_unused"),
                }
            }
        }

        #[derive(Default)]
        struct InMemoryDeviceRepo {
            devices: Mutex<Vec<MobileDevice>>,
        }
        #[async_trait]
        impl MobileDeviceRepositoryPort for InMemoryDeviceRepo {
            async fn save(&self, device: &MobileDevice) -> Result<(), MobileDeviceError> {
                self.devices.lock().unwrap().push(device.clone());
                Ok(())
            }
            async fn find_by_username(
                &self,
                username: &str,
            ) -> Result<Option<MobileDevice>, MobileDeviceError> {
                Ok(self
                    .devices
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|d| d.username == username)
                    .cloned())
            }
            async fn find_by_device_id(
                &self,
                _: &MobileDeviceId,
            ) -> Result<Option<MobileDevice>, MobileDeviceError> {
                Ok(None)
            }
            async fn list_all(&self) -> Result<Vec<MobileDevice>, MobileDeviceError> {
                Ok(self.devices.lock().unwrap().clone())
            }
            async fn delete(&self, _: &MobileDeviceId) -> Result<bool, MobileDeviceError> {
                Ok(false)
            }
            async fn record_activity(
                &self,
                _: &MobileDeviceId,
                _: i64,
                _: Option<String>,
                _: Option<String>,
                _: Option<String>,
            ) -> Result<(), MobileDeviceError> {
                Ok(())
            }
        }

        struct FakeHasher;
        #[async_trait]
        impl PasswordHasherPort for FakeHasher {
            async fn hash(&self, password: &str) -> Result<String, PasswordHasherError> {
                Ok(format!("phc:{password}"))
            }
            async fn verify(&self, password: &str, phc: &str) -> Result<bool, PasswordHasherError> {
                Ok(phc == format!("phc:{password}"))
            }
        }

        struct FixedEndpoint;
        #[async_trait]
        impl MobileSyncEndpointInfoPort for FixedEndpoint {
            async fn current_lan_endpoint(
                &self,
            ) -> Result<Option<LanEndpointInfo>, EndpointInfoError> {
                Ok(None)
            }
        }

        struct StubLanProbe;
        #[async_trait]
        impl LanInterfaceProbePort for StubLanProbe {
            async fn list_interfaces(&self) -> Result<Vec<LanInterface>, LanInterfaceProbeError> {
                Ok(vec![LanInterface {
                    name: "en0".into(),
                    ipv4: Ipv4Addr::new(192, 168, 1, 5),
                    is_loopback: false,
                }])
            }
        }

        #[derive(Default)]
        struct InMemorySettings {
            current: Mutex<Option<Settings>>,
        }
        #[async_trait]
        impl SettingsPort for InMemorySettings {
            async fn load(&self) -> anyhow::Result<Settings> {
                Ok(self
                    .current
                    .lock()
                    .unwrap()
                    .clone()
                    .unwrap_or_else(Settings::default))
            }
            async fn save(&self, settings: &Settings) -> anyhow::Result<()> {
                *self.current.lock().unwrap() = Some(settings.clone());
                Ok(())
            }
        }

        let repo = Arc::new(InMemoryDeviceRepo::default());
        repo.save(&MobileDevice {
            device_id: uc_core::mobile_sync::MobileDeviceId::new("did_seed"),
            label: "iPhone".into(),
            client_type: uc_core::mobile_sync::MobileClientType::IosShortcut,
            username: username.into(),
            password_hash: format!("phc:{password}"),
            created_at_ms: 1,
            last_seen_at_ms: None,
            last_seen_ip: None,
            reported_name: None,
            reported_os: None,
        })
        .await
        .unwrap();

        Arc::new(MobileSyncFacade::new(MobileSyncFacadeDeps {
            clock: Arc::new(FixedClock),
            credentials_minter: Arc::new(StaticMinter),
            password_hasher: Arc::new(FakeHasher),
            device_repo: repo,
            endpoint_info: Arc::new(FixedEndpoint),
            lan_interface_probe: Arc::new(StubLanProbe),
            settings: Arc::new(InMemorySettings::default()),
        }))
    }

    fn auth_header(username: &str, password: &str) -> String {
        let payload = BASE64_STD.encode(format!("{username}:{password}"));
        format!("basic {payload}")
    }

    #[tokio::test]
    async fn unauthenticated_get_returns_401_with_www_authenticate() {
        let facade = build_facade_with_seeded_device("mobile_alice", "wonderland").await;
        let app = build_app(facade);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/SyncClipboard.json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert!(
            resp.headers().get("www-authenticate").is_some_and(|v| v
                .to_str()
                .unwrap_or("")
                .to_lowercase()
                .contains("basic")),
            "401 必须带 WWW-Authenticate: Basic"
        );
    }

    #[tokio::test]
    async fn wrong_credentials_returns_401() {
        let facade = build_facade_with_seeded_device("mobile_alice", "wonderland").await;
        let app = build_app(facade);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/SyncClipboard.json")
                    .header("Authorization", auth_header("mobile_alice", "WRONG"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn get_before_any_put_returns_404() {
        let facade = build_facade_with_seeded_device("mobile_alice", "wonderland").await;
        let app = build_app(facade);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/SyncClipboard.json")
                    .header("Authorization", auth_header("mobile_alice", "wonderland"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn put_then_get_round_trips_with_sha256_hash() {
        let facade = build_facade_with_seeded_device("mobile_alice", "wonderland").await;
        let app = build_app(facade);

        // PUT with text "hello world"
        let put_body = serde_json::json!({
            "type": "Text",
            "text": "hello world",
            "hasData": false,
            "size": 0,
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/SyncClipboard.json")
                    .header("Authorization", auth_header("mobile_alice", "wonderland"))
                    .header("Content-Type", "application/json")
                    .body(Body::from(put_body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = to_bytes(resp.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(
            json["hash"],
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );

        // GET 应当拿到刚刚那条
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/SyncClipboard.json")
                    .header("Authorization", auth_header("mobile_alice", "wonderland"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = to_bytes(resp.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(json["text"], "hello world");
        assert_eq!(json["type"], "Text");
        assert_eq!(
            json["hash"],
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[tokio::test]
    async fn get_unknown_file_returns_404() {
        let facade = build_facade_with_seeded_device("mobile_alice", "wonderland").await;
        let app = build_app(facade);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/file/missing.png")
                    .header("Authorization", auth_header("mobile_alice", "wonderland"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn put_then_get_file_round_trips() {
        let facade = build_facade_with_seeded_device("mobile_alice", "wonderland").await;
        let app = build_app(facade);

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/file/photo.png")
                    .header("Authorization", auth_header("mobile_alice", "wonderland"))
                    .header("Content-Type", "image/png")
                    .body(Body::from(vec![0xDE_u8, 0xAD, 0xBE, 0xEF]))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/file/photo.png")
                    .header("Authorization", auth_header("mobile_alice", "wonderland"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get("content-type")
                .map(|v| v.to_str().unwrap()),
            Some("image/png")
        );
        let body_bytes = to_bytes(resp.into_body(), 1024).await.unwrap().to_vec();
        assert_eq!(body_bytes, vec![0xDE, 0xAD, 0xBE, 0xEF]);
    }
}
