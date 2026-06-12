//! `/SyncClipboard.json` 当前剪贴板元数据入口。
//!
//! `GET` 真实读取 UniClipboard 当前最新剪贴板表示；`PUT` 真实写入现有移动
//! 同步入站管线。当前没有剪贴板内容时，为兼容官方服务端返回空 Text profile。

use std::sync::Arc;

use axum::{
    body::to_bytes,
    extract::{Extension, Request, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};

use uc_application::facade::{
    AuthenticatedDevice, GetLatestMobileSyncDocError, MobileSyncFacade, SyncClipboardItemType,
    SyncClipboardMeta,
};

use super::common::{map_apply_error, outcome_kind, MAX_FILE_BYTES};

// ─── wire DTO ───────────────────────────────────────────────────────────

/// SyncClipboard 协议的 JSON schema(wire-format)。字段大小写**严格**按
/// SyncClipboard 项目 4 年实战定义来, 修改即兼容性破坏。
///
/// `type` 是 SyncClipboard 自定义关键字 —— Rust 用 `r#type` raw identifier
/// 写, serde rename 到 `type`。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub(super) struct SyncClipboardDoc {
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
    fn empty_text() -> Self {
        Self {
            r#type: "Text".to_string(),
            text: String::new(),
            data_name: None,
            has_data: false,
            size: 0,
            hash: None,
        }
    }

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

pub(super) async fn get_sync_clipboard_json(
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
            tracing::info!(
                "GET /SyncClipboard.json: 200 empty Text profile (no clipboard entry yet)"
            );
            Ok(Json(SyncClipboardDoc::empty_text()))
        }
        Err(GetLatestMobileSyncDocError::Port(err)) => {
            tracing::warn!(error = %err, "GET /SyncClipboard.json: snapshot port failure");
            Err(StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
    }
}

pub(super) async fn put_sync_clipboard_json(
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
