//! SyncClipboard 协议根路径路由(P5a.6 起接真实管线)。
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
//! | GET | `/SyncClipboard.json` | 取最新一条 paste-priority rep,翻成 SyncClipboard 元数据 |
//! | PUT | `/SyncClipboard.json` | 接收元数据, 通过 ApplyInbound 写入剪贴板 |
//! | GET | `/file/:dataName` | 取 dataName 命中的最新 entry 的字节(Image/File) |
//! | PUT | `/file/:dataName` | 把字节暂存进 IncomingMobileBuffer |
//!
//! 所有 4 条路由都通过 [`crate::mobile_lan::middleware::basic_auth`] 校验,
//! 不经 middleware 不会到达 handler;500 / 401 由 middleware 自己回, handler
//! 只处理 happy / 404 / 400。
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
//!
//! ## P5a.6 改动
//!
//! - 4 条路由全部从 `ClipboardDocStub` 切到真实 use case
//! - PUT 路径从 `Extension<AuthenticatedDevice>` 取 `MobileDeviceId` 喂给
//!   `apply_incoming` 的伪 `DeviceId("mobile_sync:<id>")`
//! - PUT 响应改为 `200 OK` 空 body —— SyncClipboard shortcut 客户端只看
//!   status code,无需读 echo meta
//! - GET 路径(meta + file)经 `LatestClipboardSnapshotPort` 真接入剪贴板
//!   存储 —— PUT 后 GET 是真往返
//! - PUT 响应里的 hash 字段从 input.text 自算 SHA-256 后回填到 wire(保留
//!   日志里的 hash_prefix 便于排障)

use std::sync::Arc;

use axum::{
    body::{to_bytes, Body},
    extract::{Extension, Path, Request, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use uc_application::facade::{
    ApplyIncomingMobileClipError, ApplyIncomingMobileClipOutcome, AuthenticatedDevice,
    GetLatestMobileSyncDocError, GetMobileSyncFileError, MobileSyncFacade, SyncClipboardItemType,
    SyncClipboardMeta,
};

use crate::mobile_lan::middleware::basic_auth;

/// `PUT /file/{dataName}` 的请求体上限 —— SyncClipboard 桌面端同档 16 MiB。
/// 真生产仍可能有图像 / RTF 大块上传, 后续 P5a.10 可观测后再调。
const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;

// ─── wire DTO ───────────────────────────────────────────────────────────

/// SyncClipboard 协议的 JSON schema(wire-format)。字段大小写**严格**按
/// SyncClipboard 项目 4 年实战定义来, 修改即兼容性破坏。
///
/// `type` 是 SyncClipboard 自定义关键字 —— Rust 用 `r#type` raw identifier
/// 写, serde rename 到 `type`。
#[derive(Debug, Clone, Deserialize, Serialize)]
struct SyncClipboardDoc {
    // iOS Shortcut 实际发的 body 字段名混合大小写,例如:
    //   `{"hasData": true, "Type": "File", "dataName": "...", "text": "..."}`
    // —— `Type` 是 PascalCase,其它是 camelCase。给每个字段都加 PascalCase
    // alias 兼容 Shortcut 客户端的不一致 schema。响应侧 (`Serialize`) 仍按
    // SyncClipboard 桌面端原契约的小写 / camelCase 输出,不动。
    #[serde(rename = "type", alias = "Type")]
    r#type: String, // PascalCase value: "Text" / "Image" / "File" / "Group"
    #[serde(default, alias = "Text")]
    text: String,
    #[serde(
        rename = "dataName",
        alias = "DataName",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    data_name: Option<String>,
    #[serde(rename = "hasData", alias = "HasData", default)]
    has_data: bool,
    #[serde(default, alias = "Size")]
    size: u64,
    /// SHA-256 hex —— 接收侧可缺省(SyncClipboard shortcut 不上传), 响应侧
    /// daemon 一定填(给 SyncClipboard 桌面端兼容用)。
    #[serde(default, alias = "Hash", skip_serializing_if = "Option::is_none")]
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
        Err(GetLatestMobileSyncDocError::NotFound) => {
            tracing::info!("GET /SyncClipboard.json: 404 (no clipboard entry yet)");
            Err(StatusCode::NOT_FOUND.into_response())
        }
        Err(GetLatestMobileSyncDocError::Port(err)) => {
            tracing::warn!(error = %err, "GET /SyncClipboard.json: snapshot port failure");
            Err(StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
    }
}

async fn put_sync_clipboard_json(
    State(facade): State<Arc<MobileSyncFacade>>,
    Extension(authed): Extension<AuthenticatedDevice>,
    request: Request,
) -> Result<StatusCode, Response> {
    // P5a.10 真机诊断:不走 axum `Json<T>` extractor —— 它在 Content-Type
    // 不匹配 / schema 偏差时直接 reject,handler 体没机会执行,日志里只看
    // 到 dispatch 之后立刻沉默,无法定位 iOS Shortcut 实际发的 body 形态。
    // 改用 `Request` + 手动 `serde_json::from_slice`,失败时把 Content-Type
    // 与 body 前缀打到 WARN 日志,下次真机一发就能看到真实 schema。
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let body_bytes = to_bytes(request.into_body(), MAX_FILE_BYTES)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "PUT /SyncClipboard.json: body buffer failed");
            (StatusCode::PAYLOAD_TOO_LARGE, "body too large").into_response()
        })?;
    let body_len = body_bytes.len();
    let doc: SyncClipboardDoc = match serde_json::from_slice(&body_bytes) {
        Ok(d) => d,
        Err(e) => {
            let preview_end = body_bytes.len().min(256);
            let body_preview = String::from_utf8_lossy(&body_bytes[..preview_end]).to_string();
            tracing::warn!(
                content_type = %content_type,
                error = %e,
                body_len,
                body_preview = %body_preview,
                "PUT /SyncClipboard.json: JSON deserialize failed"
            );
            return Err((StatusCode::BAD_REQUEST, "invalid SyncClipboard JSON").into_response());
        }
    };
    let meta = doc.into_meta().map_err(|reason| {
        tracing::warn!(content_type = %content_type, body_len, reason, "PUT /SyncClipboard.json: into_meta failed");
        (StatusCode::BAD_REQUEST, reason).into_response()
    })?;

    let item_type = meta.item_type;
    let has_data = meta.has_data;
    let size = meta.size;
    let text_preview_len = meta.text.len();

    // hash 不在路由层日志里再算 —— ApplyInbound 的 V3 envelope 流程内部
    // 已经把 content_hash 算好(`encode_snapshot_to_v3_bytes` 的副产物),
    // tracing 字段在 use case 层已经打了。重复算 SHA-256 只浪费 CPU。

    let device_id = authed.device.device_id.clone();
    match facade.put_sync_doc(meta, device_id).await {
        Ok(outcome) => {
            tracing::info!(
                item_type = ?item_type,
                has_data,
                size,
                text_len = text_preview_len,
                outcome = ?outcome_kind(&outcome),
                "PUT /SyncClipboard.json: 200"
            );
            Ok(StatusCode::OK)
        }
        Err(err) => Err(map_apply_error(err, "PUT /SyncClipboard.json")),
    }
}

async fn get_clipboard_file(
    State(facade): State<Arc<MobileSyncFacade>>,
    Path(data_name): Path<String>,
) -> Result<Response, Response> {
    match facade.get_clipboard_file(&data_name).await {
        Ok(out) => {
            tracing::info!(
                data_name = %data_name,
                mime = %out.mime,
                bytes = out.bytes.len(),
                "GET /file: 200"
            );
            let mut resp = Response::new(Body::from(out.bytes));
            *resp.status_mut() = StatusCode::OK;
            resp.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_str(&out.mime)
                    .unwrap_or(HeaderValue::from_static("application/octet-stream")),
            );
            Ok(resp)
        }
        Err(GetMobileSyncFileError::NotFound) => {
            tracing::info!(data_name = %data_name, "GET /file: 404");
            Err(StatusCode::NOT_FOUND.into_response())
        }
        Err(GetMobileSyncFileError::Port(err)) => {
            tracing::warn!(error = %err, "GET /file: snapshot port failure");
            Err(StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Err(GetMobileSyncFileError::Staging(msg)) => {
            // P5a.3.5: File 出站读 staging 文件出 IO 错误(权限 / 中途盘错)。
            tracing::warn!(error = %msg, "GET /file: staging IO failure");
            Err(StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
    }
}

async fn put_clipboard_file(
    State(facade): State<Arc<MobileSyncFacade>>,
    Extension(authed): Extension<AuthenticatedDevice>,
    Path(data_name): Path<String>,
    request: Request,
) -> Result<StatusCode, Response> {
    // mime 走 Content-Type 头;客户端不带就回退 application/octet-stream
    // (与 SyncClipboard shortcut 上传 PNG / RTF 等场景一致)。
    let raw_mime = request
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

    // mime 兜底:某些移动端(iOS Shortcut / 第三方 SyncClipboard 兼容客户端)
    // 上传 .jpg/.png 时不带 Content-Type 或带 application/octet-stream。如果
    // 让 octet-stream 一路下沉到剪贴板写入层,会让 image rep 携带非 image/*
    // mime 落到 macOS NSPasteboard 上,系统识别不出 image type、用户读到的
    // 是原始 JPEG 字节(2026-05-08 真机回归 IMG_20260508_200644.jpg 复现)。
    // 这里在最外层基于 data_name 扩展名 + 文件头魔数嗅探,把 image 类的
    // mime 修正回正确的 image/* 形态;无法识别时保持原 mime 不动。
    let effective_mime = if mime_is_unspecific(&raw_mime) {
        match infer_image_mime(&data_name, &body_bytes) {
            Some(sniffed) => {
                tracing::info!(
                    data_name = %data_name,
                    raw_mime = %raw_mime,
                    sniffed_mime = sniffed,
                    "PUT /file: overrode unspecific Content-Type with sniffed image mime"
                );
                sniffed.to_string()
            }
            None => raw_mime.clone(),
        }
    } else {
        raw_mime.clone()
    };

    let bytes_len = body_bytes.len();
    let log_data_name = data_name.clone();
    let log_mime = effective_mime.clone();
    let device_id = authed.device.device_id.clone();
    match facade
        .put_clipboard_file(data_name, effective_mime, body_bytes, device_id)
        .await
    {
        Ok(outcome) => {
            tracing::info!(
                data_name = %log_data_name,
                mime = %log_mime,
                bytes = bytes_len,
                outcome = ?outcome_kind(&outcome),
                "PUT /file: 200"
            );
            Ok(StatusCode::OK)
        }
        Err(err) => Err(map_apply_error(err, "PUT /file")),
    }
}

/// 判断客户端给的 Content-Type 是否"没说人话",也就是值得进一步嗅探。
///
/// 命中条件:空、application/octet-stream、binary/octet-stream、application/binary,
/// 或纯 `application/*` 而无更具体的子类型(部分客户端会发 `application/`)。
fn mime_is_unspecific(mime: &str) -> bool {
    let trimmed = mime.split(';').next().unwrap_or("").trim();
    matches!(
        trimmed,
        "" | "application/octet-stream"
            | "binary/octet-stream"
            | "application/binary"
            | "application/"
    )
}

/// 嗅探图片 mime:**优先文件头魔数**(防止 .jpg 改名为 .png 等),魔数无法
/// 识别时回退扩展名。返回值是 `image/*` 中桌面端剪贴板能消费的形式。
///
/// 字节嗅探只看前 12 字节,无所有权拷贝。
fn infer_image_mime(data_name: &str, body: &[u8]) -> Option<&'static str> {
    if let Some(by_magic) = sniff_image_magic(body) {
        return Some(by_magic);
    }
    let lower = data_name.to_ascii_lowercase();
    let ext = std::path::Path::new(&lower)
        .extension()
        .and_then(|e| e.to_str())?;
    match ext {
        "jpg" | "jpeg" | "jpe" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "bmp" => Some("image/bmp"),
        "tif" | "tiff" => Some("image/tiff"),
        "heic" | "heif" => Some("image/heic"),
        _ => None,
    }
}

/// 文件头魔数嗅探。覆盖桌面剪贴板真实会遇到的 6 种格式;其余返回 None 让
/// 调用方决定是否回退扩展名。
fn sniff_image_magic(body: &[u8]) -> Option<&'static str> {
    // JPEG: FF D8 FF (SOI + 第一段 marker 高字节)
    if body.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    // PNG: 89 50 4E 47 0D 0A 1A 0A
    if body.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some("image/png");
    }
    // GIF87a / GIF89a
    if body.starts_with(b"GIF87a") || body.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    // WEBP: RIFF....WEBP
    if body.len() >= 12 && body.starts_with(b"RIFF") && &body[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    // BMP: 42 4D
    if body.starts_with(&[0x42, 0x4D]) {
        return Some("image/bmp");
    }
    // TIFF little-endian (II*\0) / big-endian (MM\0*)
    if body.starts_with(&[0x49, 0x49, 0x2A, 0x00]) || body.starts_with(&[0x4D, 0x4D, 0x00, 0x2A]) {
        return Some("image/tiff");
    }
    None
}

// ─── helpers ────────────────────────────────────────────────────────────

/// 把 outcome 翻成日志用的简短串(避免日志里出现 entry id 等敏感字段)。
fn outcome_kind(outcome: &ApplyIncomingMobileClipOutcome) -> &'static str {
    match outcome {
        ApplyIncomingMobileClipOutcome::Applied { .. } => "applied",
        ApplyIncomingMobileClipOutcome::DuplicateSkipped { .. } => "duplicate_skipped",
        ApplyIncomingMobileClipOutcome::DecodeFailed { .. } => "decode_failed",
        ApplyIncomingMobileClipOutcome::Buffered => "buffered",
    }
}

/// `apply_incoming` 的错误映射。decode 失败按 wire-protocol 契约违反翻成
/// 400,内部错误翻成 500;outcome 维度的 `DecodeFailed` 路由层不直接收
/// (use case 已经把 decode 错包成 `Ok(DecodeFailed)`),但保留映射以防协议
/// 演进引入新错误变体。
fn map_apply_error(err: ApplyIncomingMobileClipError, route: &'static str) -> Response {
    match err {
        ApplyIncomingMobileClipError::EncodeFailed(msg) => {
            tracing::warn!(error = %msg, route, "apply_incoming: encode failed");
            (StatusCode::BAD_REQUEST, msg).into_response()
        }
        ApplyIncomingMobileClipError::Inbound(err) => {
            tracing::warn!(error = %err, route, "apply_incoming: inbound pipeline failure");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        ApplyIncomingMobileClipError::Internal(msg) => {
            tracing::warn!(error = %msg, route, "apply_incoming: internal");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
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
    //! 三类断言。
    //!
    //! P5a.6 起,facade 走真实 use case + Noop ports(test_support 装配),
    //! PUT 路径调用会因 NoOp `InboundCapture/Write` 被 ApplyInbound 包成
    //! 内部错 500;GET 路径因 noop snapshot 永远空 → 404。完整往返交给
    //! P5a.10 真机回归。

    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    use uc_application::facade::MobileSyncFacade;

    use crate::mobile_lan::test_support::{auth_header, build_facade_with_seeded_device};

    fn build_app(facade: Arc<MobileSyncFacade>) -> Router {
        build_router(facade)
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
    async fn get_with_no_clipboard_entry_returns_404() {
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
}
