//! `/file/{dataName}` 文件数据入口。
//!
//! `GET` 真实读取当前最新剪贴板里匹配 dataName 的图片 / 文件字节；`PUT`
//! 真实接入移动同步 staging + 文件传输生命周期。`/file` 目录本身的探测 /
//! 删除兼容入口在 `compat.rs`，不要和这里的真实文件内容处理混在一起。

use axum::{
    body::Body,
    extract::{Extension, Path, Request, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use futures_util::StreamExt;

use uc_engine::{MobileAuthenticatedSession, MobileSyncFileReadOutcome};

use super::common::{map_apply_error, outcome_kind, FILE_UPLOAD_DISK_SANITY_LIMIT};
use super::MobileLanState;

pub(super) async fn get_clipboard_file(
    State(state): State<MobileLanState>,
    Path(data_name): Path<String>,
) -> Result<Response, Response> {
    match state.core.read_file(data_name).await {
        Ok(MobileSyncFileReadOutcome::Found(out)) => {
            tracing::info!(
                mime = %out.media_type,
                bytes = out.bytes.len(),
                "GET /file: 200"
            );
            let mut resp = Response::new(Body::from(out.bytes));
            *resp.status_mut() = StatusCode::OK;
            resp.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_str(&out.media_type)
                    .unwrap_or(HeaderValue::from_static("application/octet-stream")),
            );
            Ok(resp)
        }
        Ok(MobileSyncFileReadOutcome::NotFound) => {
            tracing::info!("GET /file: 404");
            Err(StatusCode::NOT_FOUND.into_response())
        }
        Err(error) => {
            tracing::warn!(code = error.code(), category = %error.category(), "GET /file: read failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
    }
}

pub(super) async fn put_clipboard_file(
    State(state): State<MobileLanState>,
    Extension(authed): Extension<MobileAuthenticatedSession>,
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
    // Content-Length 头作为 total_bytes 提示。缺失 / 不可解析 → None,
    // 前端进度条退化为"已传 / 未知总量"。
    let total_bytes = request
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok());

    // transfer_id:协议层这次 PUT 的唯一 key,贯穿到 SyncDoc apply 阶段
    // 让 `file_transfer` 表 link + complete 闭环。生成策略:`mobile-lan:<uuid-v4>`,
    // mobile 客户端协议升级后可选通过 `?upload_id=...` 自带,server 端校验
    // 后照用(P5b 增强,本会话只走 server-gen)。
    let transfer_id = format!("mobile-lan:{}", uuid::Uuid::new_v4());
    let device_id = authed.device_id;

    // 用 raw_mime 先 begin_stage,后面 mime sniff 不影响落盘行为。
    let handle = match state
        .core
        .begin_upload(
            data_name.clone(),
            raw_mime.clone(),
            device_id.clone(),
            transfer_id.clone(),
            total_bytes,
        )
        .await
    {
        Ok(handle) => handle,
        Err(err) => {
            tracing::warn!(
                code = err.code(),
                category = %err.category(),
                "PUT /file: begin_stage failed"
            );
            return Err((StatusCode::INTERNAL_SERVER_ERROR, "staging begin failed").into_response());
        }
    };

    // 流式读 body:每个 chunk 边收边喂给 staging,handler 端不再持字节。
    // Progress ownership and throttling live in uc-engine. The HTTP adapter only
    // streams protocol chunks and keeps the MIME sniff window.
    let mut body_stream = request.into_body().into_data_stream();
    let mut bytes_received: u64 = 0;
    let mut sniff_window: Vec<u8> = Vec::with_capacity(64);
    while let Some(chunk_result) = body_stream.next().await {
        match chunk_result {
            Ok(chunk) => {
                bytes_received = bytes_received.saturating_add(chunk.len() as u64);
                if bytes_received > FILE_UPLOAD_DISK_SANITY_LIMIT as u64 {
                    let _ = state.core.abort_upload(handle.clone()).await;
                    tracing::warn!(
                        bytes_received,
                        limit = FILE_UPLOAD_DISK_SANITY_LIMIT,
                        "PUT /file: body exceeded disk sanity limit"
                    );
                    return Err((StatusCode::PAYLOAD_TOO_LARGE, "body too large").into_response());
                }
                if sniff_window.len() < 64 {
                    let take = (64 - sniff_window.len()).min(chunk.len());
                    sniff_window.extend_from_slice(&chunk[..take]);
                }
                if let Err(err) = state
                    .core
                    .append_upload(handle.clone(), chunk.to_vec())
                    .await
                {
                    let _ = state.core.abort_upload(handle.clone()).await;
                    tracing::warn!(
                        code = err.code(),
                        category = %err.category(),
                        "PUT /file: append_stage_chunk failed"
                    );
                    return Err((StatusCode::INTERNAL_SERVER_ERROR, "staging append failed")
                        .into_response());
                }
            }
            Err(e) => {
                let _ = state.core.abort_upload(handle.clone()).await;
                tracing::warn!(error = %e, "put_clipboard_file: body stream failed");
                return Err(
                    (StatusCode::INTERNAL_SERVER_ERROR, "body stream failed").into_response()
                );
            }
        }
    }

    // mime 兜底:某些移动端(iOS Shortcut / 第三方 SyncClipboard 兼容客户端)
    // 上传 .jpg/.png 时不带 Content-Type 或带 application/octet-stream。如果
    // 让 octet-stream 一路下沉到剪贴板写入层,会让 image rep 携带非 image/*
    // mime 落到 macOS NSPasteboard 上,系统识别不出 image type、用户读到的
    // 是原始 JPEG 字节(2026-05-08 真机回归 IMG_20260508_200644.jpg 复现)。
    // sniff_window 在收 body 时已截前 64 字节,够覆盖所有 image 头魔数。
    let effective_mime = if mime_is_unspecific(&raw_mime) {
        match infer_image_mime(&data_name, &sniff_window) {
            Some(sniffed) => {
                tracing::info!(
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

    let log_mime = effective_mime.clone();
    match state.core.finish_upload(handle, effective_mime).await {
        Ok(outcome) => {
            tracing::info!(
                mime = %log_mime,
                bytes = bytes_received,
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
pub(super) fn mime_is_unspecific(mime: &str) -> bool {
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
pub(super) fn infer_image_mime(data_name: &str, body: &[u8]) -> Option<&'static str> {
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
