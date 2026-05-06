//! Basic Auth middleware for the mobile LAN listener.
//!
//! 把请求头 `Authorization: basic base64(user:pass)` 翻成"哪台已登记 mobile
//! 设备"的事实, 经 [`MobileSyncFacade::authenticate_basic`] 校验后注入
//! [`AuthenticatedDevice`] extension 给后续 handler。
//!
//! ## 设计取舍
//!
//! 1. **不在 webserver 层做 base64 / scheme 解析** —— 这部分逻辑落在
//!    `uc-application::AuthenticateBasicAuthUseCase`(`uc-application/AGENTS.md`
//!    §11.1 facade 是稳定入口)。本中间件只做 1) 取 `Authorization` 头
//!    2) 调 facade 3) 翻译错误为 HTTP status。
//!
//! 2. **401 通道**:头缺失 / scheme 不对 / 用户名不存在 / 密码不对, 一律
//!    `401 Unauthorized`, 响应头带 `WWW-Authenticate: basic realm="..."`
//!    让 SyncClipboard shortcut 不会卡死。
//!
//! 3. **500 通道**:仓储不可用 / hasher 内部错误, 返回 `500 Internal Server
//!    Error`, 响应里不含细节(细节进 tracing)。

use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::Next,
    response::Response,
};

use uc_application::facade::{
    AuthenticateBasicAuthError, AuthenticateBasicAuthInput, MobileSyncFacade,
};

/// `WWW-Authenticate` 响应头值。realm 指明这是 mobile sync 的鉴权域,
/// 让客户端 / curl 在交互式场景能弹合适的密码框。
const WWW_AUTH_VALUE: &str = "Basic realm=\"uniclipboard-mobile-sync\"";

/// axum middleware: 校验 Basic Auth 头并把 [`AuthenticatedDevice`] 塞进 extensions。
///
/// 上游路由用法:
/// ```ignore
/// Router::new()
///     .route("/SyncClipboard.json", get(handler))
///     .layer(axum::middleware::from_fn_with_state(facade.clone(), basic_auth));
/// ```
pub(crate) async fn basic_auth(
    State(facade): State<Arc<MobileSyncFacade>>,
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

    match facade
        .authenticate_basic(AuthenticateBasicAuthInput {
            authorization_header: header_str,
        })
        .await
    {
        Ok(authed) => {
            tracing::info!(
                method = %method,
                path = %path,
                username = %authed.device.username,
                "mobile_lan: auth ok, dispatching to handler"
            );
            req.extensions_mut().insert(authed);
            Ok(next.run(req).await)
        }
        Err(AuthenticateBasicAuthError::InvalidCredentials) => {
            tracing::warn!(
                method = %method,
                path = %path,
                has_auth_header = has_auth,
                "mobile_lan: 401 invalid credentials"
            );
            Err(unauthorized())
        }
        Err(AuthenticateBasicAuthError::PersistenceFailed(msg)) => {
            tracing::warn!(error = %msg, "mobile basic auth: device repo failure");
            Err(internal_error())
        }
        Err(AuthenticateBasicAuthError::Internal(msg)) => {
            tracing::warn!(error = %msg, "mobile basic auth: hasher internal failure");
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

// AuthenticatedDevice 已通过 middleware 注入到 request extensions。当前 4
// 条 SyncClipboard 协议路由还不需要直接读它(只关心鉴权过没过); P5a.3 的
// `ApplyIncomingMobileClipUseCase` 会通过 `axum::extract::Extension` 拿来
// 源 device_id, 见下方 `tests::happy_path_inserts_authenticated_device`
// 的 smoke 用法。

#[cfg(test)]
mod tests {
    //! Middleware 单测。focus 是"happy path 把 [`AuthenticatedDevice`]
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

    use uc_application::facade::AuthenticatedDevice;

    use crate::mobile_lan::test_support::{auth_header, build_facade_with_seeded_device};

    /// 探针 handler: 把 middleware 注入的 [`AuthenticatedDevice`] 取出
    /// 来, 把 device username 写回 body —— 同时验证两件事:
    /// 1. middleware 在 happy path 把 extension 真的塞进去了;
    /// 2. 下游 handler 能用 `Extension<AuthenticatedDevice>` 提取出来。
    async fn echo_username(Extension(authed): Extension<AuthenticatedDevice>) -> String {
        authed.device.username.clone()
    }

    fn build_probe_app(facade: Arc<MobileSyncFacade>) -> Router {
        Router::new()
            .route("/__probe", get(echo_username))
            .layer(axum::middleware::from_fn_with_state(
                facade.clone(),
                basic_auth,
            ))
            .with_state(facade)
    }

    #[tokio::test]
    async fn happy_path_inserts_authenticated_device() {
        let facade = build_facade_with_seeded_device("mobile_alice", "wonderland").await;
        let app = build_probe_app(facade);

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
            b"mobile_alice",
            "下游 handler 应当能 Extension<AuthenticatedDevice> 取到种子设备 username"
        );
    }
}
