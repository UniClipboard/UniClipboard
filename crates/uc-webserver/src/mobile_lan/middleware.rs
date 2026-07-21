//! Basic Auth middleware for the mobile LAN listener.
//!
//! 把请求头 `Authorization: basic base64(user:pass)` 翻成"哪台已登记 mobile
//! 设备"的事实，经共享核心校验后注入已认证会话 extension 给后续 handler。
//!
//! ## 设计取舍
//!
//! 1. **不在 webserver 层做 base64 / scheme 解析** —— 本中间件只取
//!    `Authorization` 头、调用核心并翻译错误为 HTTP status。
//!
//! 2. **401 通道**:头缺失 / scheme 不对 / 用户名不存在 / 密码不对, 一律
//!    `401 Unauthorized`, 响应头带 `WWW-Authenticate: basic realm="..."`
//!    让 SyncClipboard shortcut 不会卡死。
//!
//! 3. **500 通道**:仓储不可用 / hasher 内部错误, 返回 `500 Internal Server
//!    Error`, 响应里不含细节(细节进 tracing)。

use axum::{
    extract::{Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::Next,
    response::Response,
};

use crate::mobile_lan::core::MobileLanCore;

/// `WWW-Authenticate` 响应头值。realm 指明这是 mobile sync 的鉴权域,
/// 让客户端 / curl 在交互式场景能弹合适的密码框。
const WWW_AUTH_VALUE: &str = "Basic realm=\"uniclipboard-mobile-sync\"";

/// axum middleware: 校验 Basic Auth 头并把已认证会话塞进 extensions。
///
/// 上游路由用法:
/// ```ignore
/// Router::new()
///     .route("/SyncClipboard.json", get(handler))
///     .layer(axum::middleware::from_fn_with_state(core.clone(), basic_auth));
/// ```
pub(crate) async fn basic_auth(
    State(core): State<MobileLanCore>,
    mut req: Request,
    next: Next,
) -> Result<Response, Response> {
    // 入口 INFO 日志 —— 记录每一次到达的请求, 方便诊断"iPhone 究竟有
    // 没有打到 daemon"。auth 通过/失败之后还有第二条日志补充结果。
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let has_auth = req.headers().contains_key(header::AUTHORIZATION);
    tracing::info!(
        method = %method,
        path = %path,
        has_auth_header = has_auth,
        "mobile_lan: incoming request"
    );

    let header_str = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_default();

    match core.authenticate(header_str).await {
        Ok(Some(authed)) => {
            tracing::info!(
                method = %method,
                path = %path,
                client_type = ?authed.client_type,
                "mobile_lan: auth ok, dispatching to handler"
            );
            req.extensions_mut().insert(authed);
            Ok(next.run(req).await)
        }
        Ok(None) => {
            tracing::warn!(
                method = %method,
                path = %path,
                has_auth_header = has_auth,
                "mobile_lan: 401 invalid credentials"
            );
            Err(unauthorized())
        }
        Err(error) => {
            tracing::warn!(
                code = error.code(),
                category = %error.category(),
                "mobile basic auth failed"
            );
            Err(internal_error())
        }
    }
}

fn unauthorized() -> Response {
    let mut resp = Response::new(axum::body::Body::empty());
    *resp.status_mut() = StatusCode::UNAUTHORIZED;
    resp.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_static(WWW_AUTH_VALUE),
    );
    resp
}

fn internal_error() -> Response {
    let mut resp = Response::new(axum::body::Body::empty());
    *resp.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
    resp
}

// The authenticated session is injected into request extensions. Current
// 条 SyncClipboard 协议路由还不需要直接读它(只关心鉴权过没过); P5a.3 的
// `ApplyIncomingMobileClipUseCase` 会通过 `axum::extract::Extension` 拿来
// 源 device_id, 见下方 `tests::happy_path_inserts_authenticated_device`
// 的 smoke 用法。

#[cfg(test)]
mod tests {
    //! Middleware 单测。focus 是"happy path 把认证会话
    //! 注入 request extensions, 并能被下游 handler 读到", 这是 P5a.3+
    //! 业务 use case 拿 source device_id 的前置契约。
    //!
    //! 401 / WWW-Authenticate / 错凭据这些断言放在 `routes.rs` 的集成
    //! 测试里(同一个 router + middleware 一并跑), 这里不重复。

    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::extract::Extension;
    use axum::http::{Request as HttpRequest, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use tower::ServiceExt;

    use uc_engine::MobileAuthenticatedSession;

    use crate::mobile_lan::test_support::{auth_header, build_facade_with_seeded_device};

    /// Probe handler: read the authenticated session installed by middleware.
    /// 1. middleware 在 happy path 把 extension 真的塞进去了;
    /// 2. 下游 handler 能用 `Extension<AuthenticatedDevice>` 提取出来。
    async fn echo_device_id(Extension(authed): Extension<MobileAuthenticatedSession>) -> String {
        authed.device_id
    }

    fn build_probe_app(core: MobileLanCore) -> Router {
        Router::new()
            .route("/__probe", get(echo_device_id))
            .layer(axum::middleware::from_fn_with_state(core, basic_auth))
    }

    #[tokio::test]
    async fn happy_path_inserts_authenticated_device() {
        let facade = build_facade_with_seeded_device("mobile_alice", "wonderland").await;
        let app = build_probe_app(facade.into());

        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method("GET")
                    .uri("/__probe")
                    .header("Authorization", auth_header("mobile_alice", "wonderland"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = to_bytes(resp.into_body(), 64).await.unwrap();
        assert_eq!(
            body_bytes.as_ref(),
            b"did_seed",
            "the downstream handler must receive the authenticated device id"
        );
    }
}
