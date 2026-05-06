//! `/mobile/v1/*` 路由表。
//!
//! Phase 3 子步骤 3:仅 `GET /mobile/v1/handshake` 一条 stub 路由,用于
//! daemon listener 起来后让客户端探测连通性。后续子步骤 5 在此 router 上
//! 追加 `/clipboard` / `/shortcut/install` 等业务路由。

use axum::routing::get;
use axum::Json;
use axum::Router;
use serde::Serialize;

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
pub(crate) fn build_router() -> Router {
    Router::new().route("/mobile/v1/handshake", get(handshake))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn handshake_returns_200_with_expected_payload() {
        let app = build_router();
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
        let app = build_router();
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
