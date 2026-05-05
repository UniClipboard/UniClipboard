//! `/mobile/v1/*` 鉴权 middleware。
//!
//! 实现 SPEC §4.3 五步校验的 HTTP 适配层 —— **不重复**业务规则,只做"读
//! header / 哈希 body / 调 facade / 翻 HTTP status"四件事。所有协议级判断
//! 由 [`uc_application::facade::mobile_sync::MobileSyncFacade::authenticate_request`]
//! 收口。
//!
//! ## 头部约定
//!
//! | Header | 内容 |
//! |---|---|
//! | `Authorization` | `Bearer <64-hex>` |
//! | `X-UC-Timestamp` | unix ms (i64) ASCII |
//! | `X-UC-Nonce` | 任意非空字符串 |
//! | `X-UC-Signature` | 64-hex |
//!
//! ## 错误码映射
//!
//! 见 SPEC §5.3。本 middleware 只输出错误代码与 HTTP status,不带额外业务
//! 文案 —— 业务文案归 use case 的 `Display` 与 tracing。
//!
//! ## body 处理
//!
//! Middleware 必须读完整 body 才能算 SHA-256 → 这意味着 mobile_sync 当前
//! 不能支持流式上传。v1 可接受:鉴权后的 handler 处理上限 100 MB(SPEC §5.2);
//! middleware 这里设 8 MiB 软上限,**仅作为哈希读取上限**——大 body 走业务路
//! 由的真实文件上传 handler(子步骤 5)。
//!
//! ## 子步骤 4 暂未挂载
//!
//! `routes::build_router` 当前没有 `route_layer(...)` —— axum 不允许给空
//! Router 套 layer。子步骤 5 第一条 protected 业务路由出现时,直接在
//! router builder 上调 `.route_layer(from_fn_with_state(facade,
//! mobile_auth_middleware))` 即可。所有公共符号已经准备好,届时不需要再
//!改 middleware 实现。
#![allow(dead_code)]

use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use sha2::{Digest, Sha256};
use tracing::warn;

use uc_application::facade::mobile_sync::{
    AuthenticateMobileRequestError, AuthenticateMobileRequestInput, MobileAuthHeaders,
    MobileSyncFacade,
};
use uc_core::mobile_sync::MobileDevice;

/// 8 MiB —— 内存内哈希上限。超过则直接 413,业务路由(文件上传)走自己的
/// streaming 路径,**不**经本 middleware。
pub const MOBILE_AUTH_BODY_HASH_LIMIT: usize = 8 * 1024 * 1024;

/// axum middleware:校验 4 个鉴权 header,通过则把 [`MobileDevice`] 塞进
/// `Request::extensions`,handler 用 `Extension<Arc<MobileDevice>>` 取出。
pub(crate) async fn mobile_auth_middleware(
    State(facade): State<Arc<MobileSyncFacade>>,
    request: Request,
    next: Next,
) -> Response {
    match authenticate(&facade, request).await {
        Ok(req) => next.run(req).await,
        Err(err) => err.into_response(),
    }
}

/// 把鉴权流程跟 next 调用解耦,便于错误统一翻 IntoResponse。返回的
/// `Request` 已经把 body bytes 重组回去,handler 还能正常 read。
async fn authenticate(
    facade: &Arc<MobileSyncFacade>,
    request: Request,
) -> Result<Request, MobileAuthError> {
    let method = request.method().as_str().to_string();
    let path = request
        .uri()
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| request.uri().path().to_string());

    let token_hex = parse_bearer_token(request.headers())?;
    let timestamp_ms = parse_timestamp(request.headers())?;
    let nonce = parse_nonce(request.headers())?;
    let signature_hex = parse_signature(request.headers())?;

    let (parts, body) = request.into_parts();
    let body_bytes = to_bytes(body, MOBILE_AUTH_BODY_HASH_LIMIT)
        .await
        .map_err(|_| MobileAuthError::PayloadTooLarge)?;
    let body_hash_hex = hex::encode(Sha256::digest(&body_bytes));

    let device = facade
        .authenticate_request(AuthenticateMobileRequestInput {
            headers: MobileAuthHeaders {
                token_hex,
                timestamp_ms,
                nonce,
                signature_hex,
            },
            method,
            path,
            body_hash_hex,
        })
        .await
        .map_err(MobileAuthError::from)?;

    let mut req = Request::from_parts(parts, Body::from(body_bytes));
    req.extensions_mut().insert(Arc::new(device));
    Ok(req)
}

// ─── header 解析 ─────────────────────────────────────────────────────────

fn parse_bearer_token(headers: &HeaderMap) -> Result<String, MobileAuthError> {
    let raw = headers
        .get(header::AUTHORIZATION)
        .ok_or(MobileAuthError::MissingToken)?
        .to_str()
        .map_err(|_| MobileAuthError::MissingToken)?;
    let prefix = "Bearer ";
    let token = raw
        .strip_prefix(prefix)
        .or_else(|| raw.strip_prefix("bearer "))
        .ok_or(MobileAuthError::MissingToken)?;
    Ok(token.trim().to_string())
}

fn parse_timestamp(headers: &HeaderMap) -> Result<i64, MobileAuthError> {
    let raw = headers
        .get("x-uc-timestamp")
        .ok_or(MobileAuthError::BadRequest("missing x-uc-timestamp"))?
        .to_str()
        .map_err(|_| MobileAuthError::BadRequest("x-uc-timestamp not ascii"))?;
    raw.parse::<i64>()
        .map_err(|_| MobileAuthError::BadRequest("x-uc-timestamp not i64"))
}

fn parse_nonce(headers: &HeaderMap) -> Result<String, MobileAuthError> {
    let raw = headers
        .get("x-uc-nonce")
        .ok_or(MobileAuthError::BadRequest("missing x-uc-nonce"))?
        .to_str()
        .map_err(|_| MobileAuthError::BadRequest("x-uc-nonce not ascii"))?;
    Ok(raw.trim().to_string())
}

fn parse_signature(headers: &HeaderMap) -> Result<String, MobileAuthError> {
    let raw = headers
        .get("x-uc-signature")
        .ok_or(MobileAuthError::BadRequest("missing x-uc-signature"))?
        .to_str()
        .map_err(|_| MobileAuthError::BadRequest("x-uc-signature not ascii"))?;
    Ok(raw.trim().to_string())
}

// ─── 错误响应 ────────────────────────────────────────────────────────────

#[derive(Debug)]
enum MobileAuthError {
    MissingToken,
    BadRequest(&'static str),
    InvalidToken,
    TimestampDrift,
    NonceReplay,
    InvalidSignature,
    PayloadTooLarge,
    NonceCacheFull,
    Storage(String),
}

impl From<AuthenticateMobileRequestError> for MobileAuthError {
    fn from(value: AuthenticateMobileRequestError) -> Self {
        match value {
            AuthenticateMobileRequestError::InvalidTokenFormat => {
                MobileAuthError::BadRequest("invalid token format")
            }
            AuthenticateMobileRequestError::InvalidBodyHashFormat => {
                MobileAuthError::BadRequest("invalid body hash format")
            }
            AuthenticateMobileRequestError::InvalidNonceFormat => {
                MobileAuthError::BadRequest("invalid nonce format")
            }
            AuthenticateMobileRequestError::InvalidSignatureFormat => {
                MobileAuthError::BadRequest("invalid signature format")
            }
            AuthenticateMobileRequestError::InvalidToken => MobileAuthError::InvalidToken,
            AuthenticateMobileRequestError::TimestampDrift => MobileAuthError::TimestampDrift,
            AuthenticateMobileRequestError::NonceReplay => MobileAuthError::NonceReplay,
            AuthenticateMobileRequestError::NonceCacheFull => MobileAuthError::NonceCacheFull,
            AuthenticateMobileRequestError::InvalidSignature => MobileAuthError::InvalidSignature,
            AuthenticateMobileRequestError::Storage(msg) => MobileAuthError::Storage(msg),
        }
    }
}

impl IntoResponse for MobileAuthError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            MobileAuthError::MissingToken => (
                StatusCode::UNAUTHORIZED,
                "missing_token",
                "Authorization header missing or not Bearer",
            ),
            MobileAuthError::BadRequest(msg) => (StatusCode::BAD_REQUEST, "bad_request", *msg),
            MobileAuthError::InvalidToken => (
                StatusCode::UNAUTHORIZED,
                "invalid_token",
                "token does not match any registered device",
            ),
            MobileAuthError::TimestampDrift => (
                StatusCode::UNAUTHORIZED,
                "timestamp_drift",
                "x-uc-timestamp drift exceeds tolerance",
            ),
            MobileAuthError::NonceReplay => (
                StatusCode::UNAUTHORIZED,
                "nonce_replay",
                "x-uc-nonce was already seen in window",
            ),
            MobileAuthError::InvalidSignature => (
                StatusCode::UNAUTHORIZED,
                "invalid_signature",
                "x-uc-signature does not match expected hex",
            ),
            MobileAuthError::PayloadTooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                "body too large for hash buffer",
            ),
            MobileAuthError::NonceCacheFull => (
                StatusCode::SERVICE_UNAVAILABLE,
                "nonce_cache_full",
                "nonce cache is at capacity; retry shortly",
            ),
            MobileAuthError::Storage(msg) => {
                warn!(error = %msg, "mobile auth: storage failure");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "internal storage failure",
                )
            }
        };
        (
            status,
            Json(serde_json::json!({"error": code, "message": message})),
        )
            .into_response()
    }
}

/// Handler 取设备的便捷类型别名。
///
/// 用法:
/// ```ignore
/// async fn my_handler(
///     axum::Extension(device): axum::Extension<Arc<MobileDevice>>,
///     /* ... */
/// ) -> impl IntoResponse { /* ... */ }
/// ```
pub type AuthenticatedDevice = Arc<MobileDevice>;

#[cfg(test)]
pub(crate) mod tests_util {
    //! 共享的 mobile_sync facade 测试夹具。被 middleware 自身单测和 routes
    //! 烟雾测试复用,免得每处都手写一份 fake ports。

    use std::sync::Arc;

    use async_trait::async_trait;

    use uc_application::facade::mobile_sync::{MobileSyncFacade, MobileSyncFacadeDeps};
    use uc_core::mobile_sync::{
        LanEndpointInfo, LanInterface, MintedToken, MobileClientType, MobileDevice,
        MobileDeviceError, MobileDeviceId, RegisteredDownloadToken, ShortcutDownloadToken,
        TokenHash,
    };
    use uc_core::ports::{
        ClockPort, EndpointInfoError, LanInterfaceProbeError, LanInterfaceProbePort,
        MobileDeviceRepositoryPort, MobileSyncEndpointInfoPort, MobileTokenMinterPort, NonceError,
        NoncePort, SettingsPort, ShortcutDownloadTokenError, ShortcutDownloadTokenStorePort,
    };
    use uc_core::settings::model::Settings;

    pub(crate) struct FixedClock(pub i64);
    impl ClockPort for FixedClock {
        fn now_ms(&self) -> i64 {
            self.0
        }
    }

    pub(crate) struct StaticMinter;
    impl MobileTokenMinterPort for StaticMinter {
        fn mint_token(&self) -> MintedToken {
            unreachable!("auth-middleware tests don't mint tokens")
        }
    }

    pub(crate) struct DeviceByHash(pub MobileDevice);
    #[async_trait]
    impl MobileDeviceRepositoryPort for DeviceByHash {
        async fn save(&self, _: &MobileDevice) -> Result<(), MobileDeviceError> {
            unreachable!()
        }
        async fn find_by_token_hash(
            &self,
            hash: &TokenHash,
        ) -> Result<Option<MobileDevice>, MobileDeviceError> {
            Ok(if &self.0.token_hash == hash {
                Some(self.0.clone())
            } else {
                None
            })
        }
        async fn find_by_device_id(
            &self,
            _: &MobileDeviceId,
        ) -> Result<Option<MobileDevice>, MobileDeviceError> {
            unreachable!()
        }
        async fn list_all(&self) -> Result<Vec<MobileDevice>, MobileDeviceError> {
            unreachable!()
        }
        async fn delete(&self, _: &MobileDeviceId) -> Result<bool, MobileDeviceError> {
            unreachable!()
        }
        async fn record_activity(
            &self,
            _: &MobileDeviceId,
            _: i64,
            _: Option<String>,
            _: Option<String>,
            _: Option<String>,
        ) -> Result<(), MobileDeviceError> {
            unreachable!()
        }
    }

    pub(crate) struct EmptyEndpoint;
    #[async_trait]
    impl MobileSyncEndpointInfoPort for EmptyEndpoint {
        async fn current_lan_endpoint(&self) -> Result<Option<LanEndpointInfo>, EndpointInfoError> {
            Ok(None)
        }
    }

    pub(crate) struct StubDownloadTokens;
    #[async_trait]
    impl ShortcutDownloadTokenStorePort for StubDownloadTokens {
        async fn register(
            &self,
            _: MobileDeviceId,
            _: Vec<u8>,
            _: i64,
        ) -> Result<RegisteredDownloadToken, ShortcutDownloadTokenError> {
            unreachable!()
        }
        async fn consume(
            &self,
            _: &ShortcutDownloadToken,
        ) -> Result<Option<(MobileDeviceId, Vec<u8>)>, ShortcutDownloadTokenError> {
            unreachable!()
        }
    }

    pub(crate) struct StubProbe;
    #[async_trait]
    impl LanInterfaceProbePort for StubProbe {
        async fn list_interfaces(&self) -> Result<Vec<LanInterface>, LanInterfaceProbeError> {
            Ok(vec![])
        }
    }

    pub(crate) struct StubSettings;
    #[async_trait]
    impl SettingsPort for StubSettings {
        async fn load(&self) -> anyhow::Result<Settings> {
            Ok(Settings::default())
        }
        async fn save(&self, _: &Settings) -> anyhow::Result<()> {
            Ok(())
        }
    }

    use std::collections::HashMap;
    use tokio::sync::Mutex as AsyncMutex;
    #[derive(Default)]
    pub(crate) struct WindowedNonces {
        entries: AsyncMutex<HashMap<String, i64>>,
    }
    #[async_trait]
    impl NoncePort for WindowedNonces {
        async fn record_if_new(
            &self,
            nonce: &str,
            observed_at_ms: i64,
        ) -> Result<bool, NonceError> {
            let mut g = self.entries.lock().await;
            if g.contains_key(nonce) {
                return Ok(false);
            }
            g.insert(nonce.to_string(), observed_at_ms);
            Ok(true)
        }
    }

    pub(crate) const FAKE_TOKEN_HEX: &str =
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

    pub(crate) fn token_hash_for_fake() -> TokenHash {
        use sha2::Digest;
        let mut bytes = [0u8; 32];
        hex::decode_to_slice(FAKE_TOKEN_HEX, &mut bytes).unwrap();
        let digest = sha2::Sha256::digest(bytes);
        let mut out = [0u8; 32];
        out.copy_from_slice(&digest);
        TokenHash::new(out)
    }

    pub(crate) fn fake_device() -> MobileDevice {
        MobileDevice {
            device_id: MobileDeviceId::new("did_test"),
            label: "iPhone".into(),
            client_type: MobileClientType::IosShortcut,
            reported_name: None,
            reported_os: None,
            token_hash: token_hash_for_fake(),
            created_at_ms: 0,
            last_seen_at_ms: None,
            last_seen_ip: None,
        }
    }

    pub(crate) fn build_facade(now_ms: i64) -> Arc<MobileSyncFacade> {
        Arc::new(MobileSyncFacade::new(MobileSyncFacadeDeps {
            clock: Arc::new(FixedClock(now_ms)),
            token_minter: Arc::new(StaticMinter),
            device_repo: Arc::new(DeviceByHash(fake_device())),
            endpoint_info: Arc::new(EmptyEndpoint),
            download_tokens: Arc::new(StubDownloadTokens),
            lan_interface_probe: Arc::new(StubProbe),
            settings: Arc::new(StubSettings),
            nonces: Arc::new(WindowedNonces::default()),
        }))
    }

    /// Default-now (1_000 ms) facade Arc 给 routes 烟雾测试用。
    pub(crate) fn test_facade_arc() -> Arc<MobileSyncFacade> {
        build_facade(1_000)
    }
}

#[cfg(test)]
mod tests {
    //! Middleware 黑盒测试 —— 用 mock facade(直接构造 `MobileSyncFacade`,
    //! 喂它"接受所有请求"或"按预定错误返回"的 fake ports)即可独立校验
    //! HTTP 接线层。集成测试在 server.rs 的 e2e 测试中走真 facade。

    use super::*;
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::Request;
    use axum::middleware::from_fn_with_state;
    use axum::routing::post;
    use axum::Router;
    use tower::ServiceExt;

    use super::tests_util::{build_facade, FAKE_TOKEN_HEX};
    use uc_application::facade::mobile_sync::MobileSyncFacade;

    /// Mount middleware on a tiny "echo" route that returns 200 if the
    /// device extension was injected; 500 otherwise. This isolates
    /// middleware behavior without depending on production routes.
    fn build_app(facade: Arc<MobileSyncFacade>) -> Router {
        async fn echo(
            ext: Option<axum::Extension<AuthenticatedDevice>>,
        ) -> (StatusCode, &'static str) {
            match ext {
                Some(_) => (StatusCode::OK, "ok"),
                None => (StatusCode::INTERNAL_SERVER_ERROR, "no device"),
            }
        }
        Router::new()
            .route("/protected", post(echo))
            .layer(from_fn_with_state(facade.clone(), mobile_auth_middleware))
            .with_state(facade)
    }

    fn compute_signature(
        token: &str,
        ts: i64,
        nonce: &str,
        method: &str,
        path: &str,
        body_hash: &str,
    ) -> String {
        let canonical = format!(
            "{}\n{}\n{}\n{}\n{}\n{}",
            token, ts, nonce, method, path, body_hash
        );
        hex::encode(sha2::Sha256::digest(canonical.as_bytes()))
    }

    fn build_signed_request(
        method: &str,
        path: &str,
        body: &[u8],
        token: &str,
        ts: i64,
        nonce: &str,
    ) -> Request<Body> {
        let body_hash = hex::encode(sha2::Sha256::digest(body));
        let sig = compute_signature(token, ts, nonce, method, path, &body_hash);
        let mut builder = Request::builder()
            .method(method)
            .uri(path)
            .header("authorization", format!("Bearer {token}"))
            .header("x-uc-timestamp", ts.to_string())
            .header("x-uc-nonce", nonce)
            .header("x-uc-signature", &sig);
        // empty body still works
        let _ = &mut builder;
        builder.body(Body::from(body.to_vec())).unwrap()
    }

    // ── tests ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn happy_path_passes_through_with_device() {
        let facade = build_facade(1_000);
        let app = build_app(facade);

        let resp = app
            .oneshot(build_signed_request(
                "POST",
                "/protected",
                b"hello",
                FAKE_TOKEN_HEX,
                1_000,
                "nonce-1",
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn missing_authorization_header_returns_401_missing_token() {
        let facade = build_facade(1_000);
        let app = build_app(facade);

        let req = Request::builder()
            .method("POST")
            .uri("/protected")
            .header("x-uc-timestamp", "1000")
            .header("x-uc-nonce", "n")
            .header(
                "x-uc-signature",
                "0000000000000000000000000000000000000000000000000000000000000000",
            )
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let body = to_bytes(resp.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "missing_token");
    }

    #[tokio::test]
    async fn unknown_token_returns_401_invalid_token() {
        let facade = build_facade(1_000);
        let app = build_app(facade);
        let other_token = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let resp = app
            .oneshot(build_signed_request(
                "POST",
                "/protected",
                b"",
                other_token,
                1_000,
                "n",
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let body = to_bytes(resp.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "invalid_token");
    }

    #[tokio::test]
    async fn timestamp_drift_returns_401_timestamp_drift() {
        let facade = build_facade(10_000_000);
        let app = build_app(facade);
        // header ts 偏离 60_001 ms（超出 60s 容忍）
        let resp = app
            .oneshot(build_signed_request(
                "POST",
                "/protected",
                b"",
                FAKE_TOKEN_HEX,
                10_000_000 - 60_001,
                "n",
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let body = to_bytes(resp.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "timestamp_drift");
    }

    #[tokio::test]
    async fn nonce_replay_returns_401_nonce_replay() {
        let facade = build_facade(1_000);
        let app = build_app(facade.clone());

        // 第一次成功。
        let r1 = app
            .clone()
            .oneshot(build_signed_request(
                "POST",
                "/protected",
                b"",
                FAKE_TOKEN_HEX,
                1_000,
                "nonce-replay",
            ))
            .await
            .unwrap();
        assert_eq!(r1.status(), StatusCode::OK);

        // 同 nonce 重放。
        let r2 = app
            .oneshot(build_signed_request(
                "POST",
                "/protected",
                b"",
                FAKE_TOKEN_HEX,
                1_000,
                "nonce-replay",
            ))
            .await
            .unwrap();
        assert_eq!(r2.status(), StatusCode::UNAUTHORIZED);
        let body = to_bytes(r2.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "nonce_replay");
    }

    #[tokio::test]
    async fn bad_signature_returns_401_invalid_signature() {
        let facade = build_facade(1_000);
        let app = build_app(facade);

        // 手工构造一个签名错的请求(用错误的 path 算出 sig 后又改 path)。
        let body = b"";
        let body_hash = hex::encode(sha2::Sha256::digest(body));
        let bad_sig = compute_signature(
            FAKE_TOKEN_HEX,
            1_000,
            "n",
            "POST",
            "/something-else",
            &body_hash,
        );
        let req = Request::builder()
            .method("POST")
            .uri("/protected")
            .header("authorization", format!("Bearer {FAKE_TOKEN_HEX}"))
            .header("x-uc-timestamp", "1000")
            .header("x-uc-nonce", "n")
            .header("x-uc-signature", bad_sig)
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let body = to_bytes(resp.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "invalid_signature");
    }
}
