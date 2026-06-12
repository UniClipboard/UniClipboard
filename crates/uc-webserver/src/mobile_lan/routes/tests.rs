//! 路由 + middleware 集成测试。覆盖 SPEC §14 的 happy path / 401 / 404
//! 三类断言。
//!
//! P5a.6 起,facade 走真实 use case + Noop ports(test_support 装配),
//! PUT 路径调用会因 NoOp `InboundCapture/Write` 被 ApplyInbound 包成
//! 内部错 500;GET `/SyncClipboard.json` 因 noop snapshot 永远空 → 空
//! Text profile,历史记录 GET 仍按"无记录"返回 404。完整往返交给
//! P5a.10 真机回归。

use super::*;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use uc_application::facade::MobileSyncFacade;

use crate::mobile_lan::test_support::{auth_header, build_facade_with_seeded_device};

use super::file::{infer_image_mime, mime_is_unspecific};

fn build_app(facade: Arc<MobileSyncFacade>) -> Router {
    // 测试不装配 `file_transfer` facade,路由会降级为静默 lifecycle:
    // buffer 仍然写入,但没有 Started / Progress 事件。具体 lifecycle
    // 行为由 facade / use case 单测覆盖。
    build_router(facade, None)
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
async fn get_with_no_clipboard_entry_returns_empty_text_profile() {
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
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["type"], "Text");
    assert_eq!(json["text"], "");
    assert_eq!(json["hasData"], false);
    assert_eq!(json["size"], 0);
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
async fn api_time_returns_200_for_official_syncclipboard_client_probe() {
    let facade = build_facade_with_seeded_device("mobile_alice", "wonderland").await;
    let app = build_app(facade);

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/time")
                .header("Authorization", auth_header("mobile_alice", "wonderland"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn api_version_routes_return_200_for_syncclipboard_clients() {
    let facade = build_facade_with_seeded_device("mobile_alice", "wonderland").await;
    let app = build_app(facade);

    for uri in ["/api/version", "/version"] {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(uri)
                    .header("Authorization", auth_header("mobile_alice", "wonderland"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "{uri} should be accepted");
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let version: String = serde_json::from_slice(&body).unwrap();
        assert!(
            version.contains(env!("CARGO_PKG_VERSION")),
            "{uri} should include package version"
        );
    }
}

#[tokio::test]
async fn api_history_query_returns_empty_list_when_clipboard_is_empty() {
    let facade = build_facade_with_seeded_device("mobile_alice", "wonderland").await;
    let app = build_app(facade);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/history/query")
                .header("Authorization", auth_header("mobile_alice", "wonderland"))
                .header("Content-Type", "application/x-www-form-urlencoded")
                .body(Body::from("page=1&types=15"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    assert_eq!(&body[..], b"[]");
}

#[tokio::test]
async fn api_history_query_accepts_multipart_form_probe() {
    let facade = build_facade_with_seeded_device("mobile_alice", "wonderland").await;
    let app = build_app(facade);

    let body = "--x\r\nContent-Disposition: form-data; name=\"page\"\r\n\r\n1\r\n--x\r\nContent-Disposition: form-data; name=\"types\"\r\n\r\n15\r\n--x--\r\n";
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/history/query")
                .header("Authorization", auth_header("mobile_alice", "wonderland"))
                .header("Content-Type", "multipart/form-data; boundary=x")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    assert_eq!(&body[..], b"[]");
}

#[tokio::test]
async fn api_history_invalid_profile_id_returns_400_instead_of_route_404() {
    let facade = build_facade_with_seeded_device("mobile_alice", "wonderland").await;
    let app = build_app(facade);

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/history/not-a-profile-id")
                .header("Authorization", auth_header("mobile_alice", "wonderland"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn api_history_statistics_returns_zero_shape_when_empty() {
    let facade = build_facade_with_seeded_device("mobile_alice", "wonderland").await;
    let app = build_app(facade);

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/history/statistics")
                .header("Authorization", auth_header("mobile_alice", "wonderland"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["totalCount"], 0);
    assert_eq!(json["activeCount"], 0);
    assert_eq!(json["starredCount"], 0);
    assert_eq!(json["deletedCount"], 0);
}

#[tokio::test]
async fn api_history_patch_official_route_reaches_handler() {
    let facade = build_facade_with_seeded_device("mobile_alice", "wonderland").await;
    let app = build_app(facade);

    let resp = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/api/history/Bogus/ABC")
                .header("Authorization", auth_header("mobile_alice", "wonderland"))
                .header("Content-Type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn api_history_clear_accepts_official_delete_route() {
    let facade = build_facade_with_seeded_device("mobile_alice", "wonderland").await;
    let app = build_app(facade);

    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/api/history/clear")
                .header("Authorization", auth_header("mobile_alice", "wonderland"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["deleted"], 0);
}

#[tokio::test]
async fn api_history_upload_text_uses_official_endpoint_shape() {
    let facade = build_facade_with_seeded_device("mobile_alice", "wonderland").await;
    let app = build_app(facade);

    let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/history")
                    .header("Authorization", auth_header("mobile_alice", "wonderland"))
                    .header("Content-Type", "application/x-www-form-urlencoded")
                    .body(Body::from(
                        "hash=2CF24DBA5FB0A30E26E83B2AC5B9E29E1B161E5C1FA7425E73043362938B9824&type=Text&text=hello&hasData=false&size=5",
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
    // 测试装配的入站写入是 NoOp,所以真正写剪贴板会到 500。
    // 这里 pin 的是官方 `/api/history` 路由存在且已解析 body,不再是 404。
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn api_history_upload_multipart_reaches_official_parser() {
    let facade = build_facade_with_seeded_device("mobile_alice", "wonderland").await;
    let app = build_app(facade);

    let body = "--x\r\nContent-Disposition: form-data; name=\"type\"\r\n\r\nText\r\n--x\r\nContent-Disposition: form-data; name=\"text\"\r\n\r\nhello\r\n--x--\r\n";
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/history")
                .header("Authorization", auth_header("mobile_alice", "wonderland"))
                .header("Content-Type", "multipart/form-data; boundary=x")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    assert_eq!(&body[..], b"hash is required");
}

#[tokio::test]
async fn api_history_upload_multipart_accepts_mobile_image_larger_than_axum_default() {
    let facade = build_facade_with_seeded_device("mobile_alice", "wonderland").await;
    let app = build_app(facade);

    let file_bytes = vec![0xAB; 2 * 1024 * 1024 + 1];
    let mut body = Vec::new();
    body.extend_from_slice(
        b"--x\r\nContent-Disposition: form-data; name=\"hash\"\r\n\r\n3B9B02A0796735651B28FEF2F5219C267A710E072E943FC79054900D06585CEF\r\n",
    );
    body.extend_from_slice(
        b"--x\r\nContent-Disposition: form-data; name=\"type\"\r\n\r\nImage\r\n",
    );
    body.extend_from_slice(
        b"--x\r\nContent-Disposition: form-data; name=\"text\"\r\n\r\nphoto.png\r\n",
    );
    body.extend_from_slice(
        b"--x\r\nContent-Disposition: form-data; name=\"hasData\"\r\n\r\ntrue\r\n",
    );
    body.extend_from_slice(
        b"--x\r\nContent-Disposition: form-data; name=\"data\"; filename=\"photo.png\"\r\nContent-Type: image/png\r\n\r\n",
    );
    body.extend_from_slice(&file_bytes);
    body.extend_from_slice(b"\r\n--x--\r\n");

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/history")
                .header("Authorization", auth_header("mobile_alice", "wonderland"))
                .header("Content-Type", "multipart/form-data; boundary=x")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    // 测试装配的入站写入是 NoOp,所以解析成功后会在真正写剪贴板时到 500。
    // 如果卡在 axum multipart 默认 2 MiB 限制,这里会提前返回 400/413。
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn file_folder_delete_accepts_official_route() {
    let facade = build_facade_with_seeded_device("mobile_alice", "wonderland").await;
    let app = build_app(facade);

    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/file")
                .header("Authorization", auth_header("mobile_alice", "wonderland"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn head_file_route_reaches_file_handler() {
    let facade = build_facade_with_seeded_device("mobile_alice", "wonderland").await;
    let app = build_app(facade);

    let resp = app
        .oneshot(
            Request::builder()
                .method("HEAD")
                .uri("/file/missing.png")
                .header("Authorization", auth_header("mobile_alice", "wonderland"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    assert!(body.is_empty());
}

#[tokio::test]
async fn put_file_then_buffered_returns_200() {
    // PUT /file/foo —— 走 BufferFile 分支,只塞进 IncomingMobileBuffer,
    // 不触达 ApplyInbound 真链路 → 200 OK。
    let facade = build_facade_with_seeded_device("mobile_alice", "wonderland").await;
    let app = build_app(facade);

    let resp = app
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
}

#[tokio::test]
async fn put_file_with_octet_stream_jpeg_returns_200() {
    // 2026-05-08 IMG_20260508_200644.jpg 真机回归 pin:某些移动客户端
    // 上传 .jpg 时 Content-Type 发成 application/octet-stream(或不发)。
    // 路由层应吃下,不能 400/500。mime 兜底逻辑 (sniff/扩展名) 会在
    // 内部把 mime 修正为 image/jpeg —— 这条 test 只跨过路由层,确保
    // 接口契约稳定;深层 mime 修正语义靠 `infer_image_mime_*` 单测覆盖。
    let facade = build_facade_with_seeded_device("mobile_alice", "wonderland").await;
    let app = build_app(facade);

    // 真实 JPEG 头(SOI + APP1/Exif marker), 模拟 Xiaomi 14 拍的照片头几字节
    let jpeg_head = vec![0xFF, 0xD8, 0xFF, 0xE1, 0x00, 0x18, b'E', b'x', b'i', b'f'];
    let resp = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/file/IMG_20260508_200644.jpg")
                .header("Authorization", auth_header("mobile_alice", "wonderland"))
                .header("Content-Type", "application/octet-stream")
                .body(Body::from(jpeg_head))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

// ─── mime 兜底纯函数单测 ────────────────────────────────────────────

#[test]
fn mime_is_unspecific_recognizes_known_unspecific_values() {
    assert!(mime_is_unspecific(""));
    assert!(mime_is_unspecific("application/octet-stream"));
    assert!(mime_is_unspecific(
        "application/octet-stream; charset=binary"
    ));
    assert!(mime_is_unspecific("binary/octet-stream"));
    assert!(mime_is_unspecific("application/binary"));
    assert!(mime_is_unspecific("application/"));
}

#[test]
fn mime_is_unspecific_rejects_specific_values() {
    assert!(!mime_is_unspecific("image/jpeg"));
    assert!(!mime_is_unspecific("image/png"));
    assert!(!mime_is_unspecific("text/plain"));
    assert!(!mime_is_unspecific("application/pdf"));
}

#[test]
fn infer_image_mime_prefers_byte_magic_over_extension() {
    // 文件名说 .png 但实际是 JPEG 字节 → 嗅探优先,以字节为准。
    let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
    assert_eq!(infer_image_mime("liar.png", &jpeg), Some("image/jpeg"));
}

#[test]
fn infer_image_mime_falls_back_to_extension_when_magic_misses() {
    // 字节嗅不出来(空 / 非图片头),按扩展名兜底。
    assert_eq!(infer_image_mime("photo.jpg", &[]), Some("image/jpeg"));
    assert_eq!(infer_image_mime("photo.JPEG", &[]), Some("image/jpeg"));
    assert_eq!(infer_image_mime("snap.png", b"junk"), Some("image/png"));
    assert_eq!(infer_image_mime("anim.gif", &[]), Some("image/gif"));
    assert_eq!(infer_image_mime("photo.heic", &[]), Some("image/heic"));
}

#[test]
fn infer_image_mime_returns_none_for_non_image_extension() {
    // 文件不是图片,且字节也嗅不出来 → 不要瞎猜。
    assert_eq!(infer_image_mime("doc.pdf", b"%PDF-1.7"), None);
    assert_eq!(infer_image_mime("noext", &[]), None);
    assert_eq!(infer_image_mime("script.sh", b"#!/bin/sh"), None);
}

#[test]
fn infer_image_mime_xiaomi_jpeg_real_world_case() {
    // 2026-05-08 真机回归:文件名 IMG_20260508_200644.jpg + JPEG 头,
    // 客户端发 application/octet-stream → 路由层应嗅到 image/jpeg。
    let bytes = [0xFF, 0xD8, 0xFF, 0xE1, 0x00, 0x18, b'E', b'x', b'i', b'f'];
    assert_eq!(
        infer_image_mime("IMG_20260508_200644.jpg", &bytes),
        Some("image/jpeg")
    );
}

#[tokio::test]
async fn put_sync_doc_with_unknown_type_returns_400() {
    // wire DTO 的 `type` 字段是 SyncClipboard 协议契约,未知值映射不到
    // SyncClipboardItemType → routes 翻 400。这条路径不进入 ApplyInbound,
    // 与 NoOp capture/write 无关。
    let facade = build_facade_with_seeded_device("mobile_alice", "wonderland").await;
    let app = build_app(facade);

    let put_body = serde_json::json!({
        "type": "Strange",
        "text": "ignored",
        "hasData": false,
        "size": 0,
    });
    let resp = app
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
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

/// iOS Shortcut 真机回归实测:body 用 PascalCase `Type` 字段,其它字段
/// camelCase。serde alias 必须兼容这种混合大小写,DTO 反序列化要成功
/// 走到 facade(NoOp facade 因 inbound 不可写最终返 500,但**不**是 400
/// —— 这是 schema 兼容回归的关键 pin)。
#[tokio::test]
async fn put_sync_doc_accepts_pascal_case_type_field() {
    let facade = build_facade_with_seeded_device("mobile_alice", "wonderland").await;
    let app = build_app(facade);

    let put_body = r#"{"hasData":true,"Type":"File","dataName":"foo.pdf","text":"foo.pdf"}"#;
    let resp = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/SyncClipboard.json")
                .header("Authorization", auth_header("mobile_alice", "wonderland"))
                .header("Content-Type", "application/json")
                .body(Body::from(put_body))
                .unwrap(),
        )
        .await
        .unwrap();
    // schema 兼容 ok → 不是 400; NoOp facade 路径下 ApplyInbound 写不了 →
    // 500。我们要 pin 的是"DTO 不再因 PascalCase Type 拒绝 body"。
    assert_ne!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "PascalCase Type field must be accepted by serde alias"
    );
}
