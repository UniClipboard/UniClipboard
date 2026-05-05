//! `/mobile/v1/*` 路由表。
//!
//! - `GET /mobile/v1/handshake` —— 仍保持**无鉴权**,作为 listener 拉起来
//!   时客户端的连通性探测端口。SPEC §4.3 规定所有业务接口都鉴权,但
//!   handshake 在客户端侧"还没拿到 token"也想跑(配置加载阶段),与现有
//!   stub 测试一致,放在 unprotected 集合下。
//! - **protected** 子 router(目前空)挂 [`mobile_auth_middleware`]——子步
//!   骤 5 在这里追加 `/clipboard` / `/shortcut/template` 等业务路由,会
//!   自动套上鉴权。

use std::sync::Arc;

use axum::routing::get;
use axum::Json;
use axum::Router;
use serde::Serialize;

use uc_application::facade::mobile_sync::MobileSyncFacade;

/// `GET /mobile/v1/handshake` 的响应体。
///
/// `version` 标记协议大版本,客户端用它来决定是否兼容(v1 客户端见到 v2
/// 立即停止)。`accepts` 标记当前 daemon 期望的鉴权 / 签名套件,客户端拼
/// 签名前会读它来挑算法。
#[derive(Debug, Serialize, PartialEq, Eq)]
pub(crate) struct HandshakeResponse {
    pub(crate) version: &'static str,
    pub(crate) accepts: &'static str,
}

async fn handshake() -> Json<HandshakeResponse> {
    Json(HandshakeResponse {
        version: "v1",
        accepts: "sha256-bearer-v1",
    })
}

/// 构造 `/mobile/v1/*` 子路由。daemon listener 把它挂到 axum app 根。
///
/// `facade` 在 Phase 3 子步骤 4 已经传入但**还没接到 router 上**——子步骤 5
/// 真业务路由(`/clipboard/*` / `/shortcut/template`)落地时,会在 build_router
/// 内部追加一个**带路由的** protected sub-router 并 `.route_layer(...)` 套
/// [`mobile_auth_middleware`](crate::mobile_lan::middleware)。保持 facade 参
/// 数现在就到位,避免后续动 daemon::app.rs / server.rs 的接口签名。
///
/// 当前 axum 不允许给空 Router 套 `route_layer`(panic "Adding a route_layer
/// before any routes is a no-op"),所以这里暂时只挂 unprotected handshake。
pub(crate) fn build_router(_facade: Arc<MobileSyncFacade>) -> Router {
    // `_facade` 暂未消费;子步骤 5 在此 fn 内部加 protected sub-router 时再接
    // (用 `axum::middleware::from_fn_with_state` + `mobile_auth_middleware`)。
    Router::new().route("/mobile/v1/handshake", get(handshake))
}

#[cfg(test)]
mod tests {
    //! 路由表的烟雾测试。这里只验证"unprotected handshake 仍然可达 + 未知
    //! 路径 404",中间件具体行为已在 `mobile_lan::middleware::tests` 覆盖。

    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    use crate::mobile_lan::middleware::tests_util::test_facade_arc;

    #[tokio::test]
    async fn handshake_returns_200_with_expected_payload() {
        let app = build_router(test_facade_arc());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/mobile/v1/handshake")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // 1 KiB 上限对 handshake 响应(几十字节 JSON)绰绰有余;axum 0.7 用
        // `to_bytes` 替代以前需要 http_body_util 的写法。
        let body_bytes = to_bytes(response.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(json["version"], "v1");
        assert_eq!(json["accepts"], "sha256-bearer-v1");
    }

    #[tokio::test]
    async fn unknown_route_returns_404() {
        let app = build_router(test_facade_arc());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/mobile/v1/nope")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
