//! SyncClipboard v3 历史记录兼容入口。
//!
//! 当前实现分两类：
//! - 真实桥接：`GET /api/history/{profileId}`、`GET /api/history/{profileId}/data`、
//!   `POST /api/history` 会映射到当前最新剪贴板和移动同步入站管线。
//! - 兼容壳：`POST /api/history/query`、`GET /api/history/statistics`、
//!   `PATCH /api/history/{type}/{hash}`、`DELETE /api/history/clear` 只接住
//!   SyncClipboard 客户端流程需要的请求；它们还不是完整历史库的分页、统计、
//!   标星 / 置顶 / 删除持久化或真实清空。
//!
//! 这份边界必须显式保留，避免以后把“客户端不再 404”误读为“已完整实现
//! SyncClipboard 官方历史系统”。

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    body::to_bytes,
    extract::{Extension, FromRequest, Multipart, Path, Request, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use uc_application::facade::{
    AuthenticatedDevice, GetLatestMobileSyncDocError, MobileSyncFacade, SyncClipboardItemType,
    SyncClipboardMeta,
};

use super::common::{map_apply_error, MAX_FILE_BYTES};
use super::file::get_clipboard_file;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct HistoryRecordDoc {
    hash: String,
    #[serde(rename = "type")]
    r#type: String,
    #[serde(default)]
    text: String,
    create_time: String,
    last_modified: String,
    last_accessed: String,
    starred: bool,
    pinned: bool,
    size: u64,
    has_data: bool,
    version: u32,
    is_deleted: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct HistoryStatisticsDoc {
    total_count: u32,
    starred_count: u32,
    deleted_count: u32,
    active_count: u32,
    total_file_size_mb: f64,
}

#[derive(Debug, Clone)]
struct ParsedHistoryUpload {
    fields: HashMap<String, String>,
    file: Option<ParsedHistoryFile>,
}

#[derive(Debug, Clone)]
struct ParsedHistoryFile {
    data_name: String,
    mime: String,
    bytes: Vec<u8>,
}

impl HistoryRecordDoc {
    fn from_meta(meta: SyncClipboardMeta) -> Self {
        let now = Utc::now().to_rfc3339();
        let hash = meta.hash.unwrap_or_default().trim().to_ascii_uppercase();
        Self {
            hash,
            r#type: item_type_to_wire(meta.item_type).to_string(),
            text: meta.text,
            create_time: now.clone(),
            last_modified: now.clone(),
            last_accessed: now,
            starred: false,
            pinned: false,
            size: meta.size,
            has_data: meta.has_data,
            version: 0,
            is_deleted: false,
        }
    }

    fn from_upload_fields(fields: &HashMap<String, String>, data_name_size: Option<u64>) -> Self {
        let now = Utc::now().to_rfc3339();
        let hash = fields
            .get("hash")
            .map(|v| v.trim().to_ascii_uppercase())
            .unwrap_or_default();
        let size = fields
            .get("size")
            .and_then(|v| v.parse::<u64>().ok())
            .or(data_name_size)
            .unwrap_or(0);
        let has_data = fields
            .get("hasData")
            .or_else(|| fields.get("hasdata"))
            .is_some_and(|v| v.eq_ignore_ascii_case("true"));
        Self {
            hash,
            r#type: fields
                .get("type")
                .cloned()
                .unwrap_or_else(|| "Text".to_string()),
            text: fields.get("text").cloned().unwrap_or_default(),
            create_time: fields
                .get("createTime")
                .cloned()
                .unwrap_or_else(|| now.clone()),
            last_modified: fields
                .get("lastModified")
                .cloned()
                .unwrap_or_else(|| now.clone()),
            last_accessed: fields
                .get("lastAccessed")
                .cloned()
                .unwrap_or_else(|| now.clone()),
            starred: parse_bool_field(fields, "starred"),
            pinned: parse_bool_field(fields, "pinned"),
            size,
            has_data,
            version: fields
                .get("version")
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(0),
            is_deleted: parse_bool_field(fields, "isDeleted"),
        }
    }
}

fn item_type_to_wire(item_type: SyncClipboardItemType) -> &'static str {
    match item_type {
        SyncClipboardItemType::Text => "Text",
        SyncClipboardItemType::Image => "Image",
        SyncClipboardItemType::File => "File",
        SyncClipboardItemType::Group => "Group",
    }
}

fn item_type_from_wire(raw: &str) -> Option<SyncClipboardItemType> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "text" => Some(SyncClipboardItemType::Text),
        "image" => Some(SyncClipboardItemType::Image),
        "file" => Some(SyncClipboardItemType::File),
        "group" => Some(SyncClipboardItemType::Group),
        _ => None,
    }
}

fn parse_bool_field(fields: &HashMap<String, String>, key: &str) -> bool {
    fields
        .get(key)
        .is_some_and(|v| v.eq_ignore_ascii_case("true"))
}

fn parse_profile_id(profile_id: &str) -> Option<(SyncClipboardItemType, String)> {
    let (kind, hash) = profile_id.split_once('-')?;
    let item_type = item_type_from_wire(kind)?;
    if hash.trim().is_empty() {
        return None;
    }
    Some((item_type, hash.trim().to_ascii_uppercase()))
}

fn same_profile(meta: &SyncClipboardMeta, item_type: SyncClipboardItemType, hash: &str) -> bool {
    meta.item_type == item_type
        && meta
            .hash
            .as_deref()
            .is_some_and(|h| h.eq_ignore_ascii_case(hash))
}

fn empty_history_statistics() -> HistoryStatisticsDoc {
    HistoryStatisticsDoc {
        total_count: 0,
        starred_count: 0,
        deleted_count: 0,
        active_count: 0,
        total_file_size_mb: 0.0,
    }
}

fn statistics_from_record(record: &HistoryRecordDoc) -> HistoryStatisticsDoc {
    HistoryStatisticsDoc {
        total_count: 1,
        starred_count: u32::from(record.starred),
        deleted_count: u32::from(record.is_deleted),
        active_count: u32::from(!record.is_deleted),
        total_file_size_mb: record.size as f64 / 1024.0 / 1024.0,
    }
}

async fn latest_history_record(
    facade: &MobileSyncFacade,
) -> Result<Option<HistoryRecordDoc>, Response> {
    match facade.get_latest_sync_doc().await {
        Ok(meta) => Ok(Some(HistoryRecordDoc::from_meta(meta))),
        Err(GetLatestMobileSyncDocError::NotFound) => Ok(None),
        Err(GetLatestMobileSyncDocError::Port(err)) => {
            tracing::warn!(error = %err, "GET /api/history: snapshot port failure");
            Err(StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
    }
}

pub(super) async fn query_history_records(
    State(facade): State<Arc<MobileSyncFacade>>,
) -> Result<Json<Vec<HistoryRecordDoc>>, Response> {
    match latest_history_record(&facade).await? {
        Some(record) => Ok(Json(vec![record])),
        None => Ok(Json(Vec::new())),
    }
}

pub(super) async fn get_history_statistics(
    State(facade): State<Arc<MobileSyncFacade>>,
) -> Result<Json<HistoryStatisticsDoc>, Response> {
    match latest_history_record(&facade).await? {
        Some(record) => Ok(Json(statistics_from_record(&record))),
        None => Ok(Json(empty_history_statistics())),
    }
}

pub(super) async fn get_history_record(
    State(facade): State<Arc<MobileSyncFacade>>,
    Path(profile_id): Path<String>,
) -> Result<Json<HistoryRecordDoc>, Response> {
    let Some((item_type, hash)) = parse_profile_id(&profile_id) else {
        return Err((StatusCode::BAD_REQUEST, "Invalid profileId format").into_response());
    };

    let Some(record) = latest_history_record(&facade).await? else {
        return Err(StatusCode::NOT_FOUND.into_response());
    };
    if record.r#type == item_type_to_wire(item_type) && record.hash.eq_ignore_ascii_case(&hash) {
        Ok(Json(record))
    } else {
        Err(StatusCode::NOT_FOUND.into_response())
    }
}

pub(super) async fn get_history_data(
    State(facade): State<Arc<MobileSyncFacade>>,
    Path(profile_id): Path<String>,
) -> Result<Response, Response> {
    let Some((item_type, hash)) = parse_profile_id(&profile_id) else {
        return Err((StatusCode::BAD_REQUEST, "Invalid profileId format").into_response());
    };
    let meta = match facade.get_latest_sync_doc().await {
        Ok(meta) => meta,
        Err(GetLatestMobileSyncDocError::NotFound) => {
            return Err(StatusCode::NOT_FOUND.into_response());
        }
        Err(GetLatestMobileSyncDocError::Port(err)) => {
            tracing::warn!(error = %err, "GET /api/history data: snapshot port failure");
            return Err(StatusCode::INTERNAL_SERVER_ERROR.into_response());
        }
    };
    if !same_profile(&meta, item_type, &hash) {
        return Err(StatusCode::NOT_FOUND.into_response());
    }
    let Some(data_name) = meta.data_name else {
        return Err(StatusCode::NOT_FOUND.into_response());
    };
    get_clipboard_file(State(facade), Path(data_name)).await
}

pub(super) async fn patch_history_record(
    State(facade): State<Arc<MobileSyncFacade>>,
    Path((item_type, hash)): Path<(String, String)>,
) -> Result<Json<HistoryRecordDoc>, Response> {
    let item_type = item_type_from_wire(&item_type)
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "invalid type").into_response())?;
    let hash = hash.trim();
    if hash.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "hash is required").into_response());
    }
    let profile_id = match parse_profile_id(hash) {
        Some((embedded_type, embedded_hash)) if embedded_type == item_type => {
            format!("{}-{}", item_type_to_wire(item_type), embedded_hash)
        }
        Some(_) => {
            return Err((StatusCode::BAD_REQUEST, "profileId type mismatch").into_response());
        }
        None => format!(
            "{}-{}",
            item_type_to_wire(item_type),
            hash.to_ascii_uppercase()
        ),
    };
    get_history_record(State(facade), Path(profile_id)).await
}

pub(super) async fn clear_history_compat() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "deleted": 0 }))
}

pub(super) async fn post_history_record(
    State(facade): State<Arc<MobileSyncFacade>>,
    Extension(authed): Extension<AuthenticatedDevice>,
    request: Request,
) -> Result<Json<HistoryRecordDoc>, Response> {
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let upload = if content_type
        .to_ascii_lowercase()
        .starts_with("multipart/form-data")
    {
        parse_history_multipart(request).await?
    } else {
        parse_history_urlencoded(request).await?
    };

    let item_type_raw = upload
        .fields
        .get("type")
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "type is required").into_response())?;
    let item_type = item_type_from_wire(item_type_raw)
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "invalid type").into_response())?;
    let hash = upload
        .fields
        .get("hash")
        .map(|v| v.trim().to_ascii_uppercase())
        .unwrap_or_default();
    if hash.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "hash is required").into_response());
    }

    let mut record = HistoryRecordDoc::from_upload_fields(
        &upload.fields,
        upload.file.as_ref().map(|file| file.bytes.len() as u64),
    );
    record.hash = hash.clone();
    record.r#type = item_type_to_wire(item_type).to_string();

    let has_data = record.has_data || upload.file.is_some();
    let data_name = upload
        .file
        .as_ref()
        .map(|file| file.data_name.clone())
        .or_else(|| upload.fields.get("dataName").cloned());

    match (item_type, upload.file) {
        (SyncClipboardItemType::Text, Some(file)) => {
            let full_text = String::from_utf8_lossy(&file.bytes).into_owned();
            let text = if full_text.is_empty() {
                record.text.clone()
            } else {
                full_text
            };
            let meta = SyncClipboardMeta {
                item_type,
                text,
                data_name: None,
                has_data: false,
                size: record.size,
                hash: Some(hash),
            };
            facade
                .put_sync_doc(meta, authed.device.device_id)
                .await
                .map_err(|err| map_apply_error(err, "POST /api/history"))?;
        }
        (SyncClipboardItemType::Image | SyncClipboardItemType::File, Some(file)) => {
            let transfer_id = format!("mobile-lan-history:{}", uuid::Uuid::new_v4());
            facade
                .put_clipboard_file(
                    file.data_name.clone(),
                    file.mime,
                    file.bytes,
                    authed.device.device_id.clone(),
                    transfer_id,
                )
                .await
                .map_err(|err| map_apply_error(err, "POST /api/history file"))?;
            let meta = SyncClipboardMeta {
                item_type,
                text: record.text.clone(),
                data_name: Some(file.data_name),
                has_data: true,
                size: record.size,
                hash: Some(hash),
            };
            facade
                .put_sync_doc(meta, authed.device.device_id)
                .await
                .map_err(|err| map_apply_error(err, "POST /api/history"))?;
        }
        (SyncClipboardItemType::Group, Some(_)) => {
            return Err((StatusCode::BAD_REQUEST, "Group is not supported").into_response());
        }
        (_, None) => {
            let meta = SyncClipboardMeta {
                item_type,
                text: record.text.clone(),
                data_name,
                has_data,
                size: record.size,
                hash: Some(hash),
            };
            facade
                .put_sync_doc(meta, authed.device.device_id)
                .await
                .map_err(|err| map_apply_error(err, "POST /api/history"))?;
        }
    }

    Ok(Json(record))
}

async fn parse_history_urlencoded(request: Request) -> Result<ParsedHistoryUpload, Response> {
    let body_bytes = to_bytes(request.into_body(), MAX_FILE_BYTES)
        .await
        .map_err(|_| (StatusCode::PAYLOAD_TOO_LARGE, "body too large").into_response())?;
    let fields = url::form_urlencoded::parse(&body_bytes)
        .into_owned()
        .collect::<HashMap<String, String>>();
    Ok(ParsedHistoryUpload { fields, file: None })
}

async fn parse_history_multipart(request: Request) -> Result<ParsedHistoryUpload, Response> {
    let mut multipart = Multipart::from_request(request, &()).await.map_err(|err| {
        tracing::warn!(error = %err, "POST /api/history: multipart extractor failed");
        (StatusCode::BAD_REQUEST, "invalid multipart body").into_response()
    })?;
    let mut fields = HashMap::new();
    let mut file = None;
    while let Some(field) = multipart.next_field().await.map_err(|err| {
        tracing::warn!(error = %err, "POST /api/history: multipart field read failed");
        (StatusCode::BAD_REQUEST, "invalid multipart field").into_response()
    })? {
        let name = field.name().unwrap_or("").to_string();
        let file_name = field.file_name().map(|s| s.to_string());
        let mime = field
            .content_type()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "application/octet-stream".to_string());
        if name == "data" || file_name.is_some() {
            let data_name = file_name
                .or_else(|| fields.get("dataName").cloned())
                .unwrap_or_else(|| "clipboard.bin".to_string());
            let bytes = field.bytes().await.map_err(|err| {
                tracing::warn!(error = %err, "POST /api/history: multipart file read failed");
                (StatusCode::BAD_REQUEST, "invalid multipart file").into_response()
            })?;
            file = Some(ParsedHistoryFile {
                data_name,
                mime,
                bytes: bytes.to_vec(),
            });
        } else if !name.is_empty() {
            let value = field.text().await.map_err(|err| {
                tracing::warn!(error = %err, "POST /api/history: multipart text read failed");
                (StatusCode::BAD_REQUEST, "invalid multipart field").into_response()
            })?;
            fields.insert(name, value);
        }
    }
    Ok(ParsedHistoryUpload { fields, file })
}
