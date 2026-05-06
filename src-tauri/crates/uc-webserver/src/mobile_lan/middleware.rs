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

// AuthenticatedDevice 已通过 middleware 注入到 request extensions。当前路由
// 不需要直接读它(只关心鉴权过没过), 后续(Phase 5)如需在 handler 里精确知
// 道是哪台 device 在请求, 在这里加一个 `axum::extract::Extension` 提取的薄
// 包装即可 —— 留作 future work, 不预先引入未使用的代码。
