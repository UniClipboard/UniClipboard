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
    extract::{
        multipart::{Field, MultipartError},
        Extension, FromRequest, Multipart, Path, Request, State,
    },
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
use uc_core::mobile_sync::StagingHandle;

use super::common::{map_apply_error, FILE_UPLOAD_DISK_SANITY_LIMIT, MAX_FILE_BYTES};
use super::file::{get_clipboard_file, infer_image_mime, mime_is_unspecific};

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
    size: u64,
    handle: StagingHandle,
    transfer_id: String,
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

fn current_profile_type_allows_hash_drift(item_type: SyncClipboardItemType) -> bool {
    matches!(
        item_type,
        SyncClipboardItemType::Image | SyncClipboardItemType::File
    )
}

fn current_profile_hash_is_compatible(
    item_type: SyncClipboardItemType,
    current_hash: Option<&str>,
    requested_hash: &str,
) -> bool {
    current_hash.is_some_and(|h| h.eq_ignore_ascii_case(requested_hash))
        || current_profile_type_allows_hash_drift(item_type)
}

fn current_profile_meta_matches_request(
    meta: &SyncClipboardMeta,
    item_type: SyncClipboardItemType,
    requested_hash: &str,
) -> bool {
    meta.item_type == item_type
        && current_profile_hash_is_compatible(item_type, meta.hash.as_deref(), requested_hash)
}

fn current_profile_record_for_request(
    mut record: HistoryRecordDoc,
    item_type: SyncClipboardItemType,
    requested_hash: &str,
) -> Option<HistoryRecordDoc> {
    if record.r#type != item_type_to_wire(item_type) {
        return None;
    }
    if !current_profile_hash_is_compatible(item_type, Some(record.hash.as_str()), requested_hash) {
        return None;
    }

    let requested_hash = requested_hash.trim().to_ascii_uppercase();
    if !record.hash.eq_ignore_ascii_case(&requested_hash) {
        tracing::debug!(
            item_type = ?item_type,
            current_hash = %record.hash,
            requested_hash = %requested_hash,
            "GET /api/history: serving current data-bearing record for client profile hash"
        );
        record.hash = requested_hash;
    }
    Some(record)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn record(item_type: SyncClipboardItemType, hash: &str) -> HistoryRecordDoc {
        HistoryRecordDoc {
            hash: hash.to_string(),
            r#type: item_type_to_wire(item_type).to_string(),
            text: "photo.jpg".to_string(),
            create_time: "2026-05-13T13:43:38Z".to_string(),
            last_modified: "2026-05-13T13:43:38Z".to_string(),
            last_accessed: "2026-05-13T13:43:38Z".to_string(),
            starred: false,
            pinned: false,
            size: 1184433,
            has_data: true,
            version: 0,
            is_deleted: false,
        }
    }

    #[test]
    fn current_profile_record_accepts_mobile_upload_hash_drift() {
        let current = record(SyncClipboardItemType::Image, "SERVER_RECOMPUTED_HASH");

        let resolved = current_profile_record_for_request(
            current,
            SyncClipboardItemType::Image,
            "0B13A2265544DE3C8C1286E4B854D39833A49BDAA3F82114AE19F55B7F08FBB2",
        )
        .expect("same-type current image should satisfy the requested profile");

        assert_eq!(
            resolved.hash,
            "0B13A2265544DE3C8C1286E4B854D39833A49BDAA3F82114AE19F55B7F08FBB2"
        );
    }

    #[test]
    fn current_profile_record_rejects_wrong_type() {
        let current = record(SyncClipboardItemType::Text, "TEXT_HASH");

        assert!(current_profile_record_for_request(
            current,
            SyncClipboardItemType::Image,
            "0B13A2265544DE3C8C1286E4B854D39833A49BDAA3F82114AE19F55B7F08FBB2",
        )
        .is_none());
    }

    #[test]
    fn current_profile_meta_accepts_same_type_even_when_hash_drifted() {
        let meta = SyncClipboardMeta {
            item_type: SyncClipboardItemType::Image,
            text: "clipboard_ce8ee62d.jpg".to_string(),
            data_name: Some("clipboard_ce8ee62d.jpg".to_string()),
            has_data: true,
            size: 1184433,
            hash: Some("SERVER_RECOMPUTED_HASH".to_string()),
        };

        assert!(current_profile_meta_matches_request(
            &meta,
            SyncClipboardItemType::Image,
            "0B13A2265544DE3C8C1286E4B854D39833A49BDAA3F82114AE19F55B7F08FBB2",
        ));
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
    match current_profile_record_for_request(record, item_type, &hash) {
        Some(record) => Ok(Json(record)),
        None => Err(StatusCode::NOT_FOUND.into_response()),
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
    if !current_profile_meta_matches_request(&meta, item_type, &hash) {
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
    let mut upload = if content_type
        .to_ascii_lowercase()
        .starts_with("multipart/form-data")
    {
        parse_history_multipart(request, &facade).await?
    } else {
        parse_history_urlencoded(request).await?
    };

    let item_type_raw = match upload.fields.get("type") {
        Some(value) => value,
        None => {
            abort_parsed_history_file(&facade, upload.file.take()).await;
            return Err((StatusCode::BAD_REQUEST, "type is required").into_response());
        }
    };
    let item_type = match item_type_from_wire(item_type_raw) {
        Some(item_type) => item_type,
        None => {
            abort_parsed_history_file(&facade, upload.file.take()).await;
            return Err((StatusCode::BAD_REQUEST, "invalid type").into_response());
        }
    };
    let hash = upload
        .fields
        .get("hash")
        .map(|v| v.trim().to_ascii_uppercase())
        .unwrap_or_default();
    if hash.is_empty() {
        abort_parsed_history_file(&facade, upload.file.take()).await;
        return Err((StatusCode::BAD_REQUEST, "hash is required").into_response());
    }

    let mut record = HistoryRecordDoc::from_upload_fields(
        &upload.fields,
        upload.file.as_ref().map(|file| file.size),
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
            tracing::warn!(
                data_name = %file.data_name,
                "POST /api/history: text upload unexpectedly contained file data"
            );
            facade.abort_file_upload(file.handle).await;
            return Err(
                (StatusCode::BAD_REQUEST, "Text file part is not supported").into_response()
            );
        }
        (SyncClipboardItemType::Image | SyncClipboardItemType::File, Some(file)) => {
            let data_name = file.data_name.clone();
            let size = file.size;
            facade
                .finalize_file_upload(
                    file.handle,
                    file.data_name,
                    file.mime,
                    authed.device.device_id.clone(),
                    file.transfer_id,
                )
                .await
                .map_err(|err| map_apply_error(err, "POST /api/history file"))?;
            let meta = SyncClipboardMeta {
                item_type,
                text: record.text.clone(),
                data_name: Some(data_name),
                has_data: true,
                size,
                hash: Some(hash),
            };
            facade
                .put_sync_doc(meta, authed.device.device_id)
                .await
                .map_err(|err| map_apply_error(err, "POST /api/history"))?;
        }
        (SyncClipboardItemType::Group, Some(file)) => {
            facade.abort_file_upload(file.handle).await;
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

async fn parse_history_multipart(
    request: Request,
    facade: &MobileSyncFacade,
) -> Result<ParsedHistoryUpload, Response> {
    let mut multipart = Multipart::from_request(request, &()).await.map_err(|err| {
        tracing::warn!(error = %err, "POST /api/history: multipart extractor failed");
        err.into_response()
    })?;
    let mut fields = HashMap::new();
    let mut file = None;
    loop {
        let Some(field) = (match multipart.next_field().await {
            Ok(field) => field,
            Err(err) => {
                abort_parsed_history_file(facade, file.take()).await;
                return Err(map_multipart_error(err, "multipart field read failed"));
            }
        }) else {
            break;
        };
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
            abort_parsed_history_file(facade, file.take()).await;
            file = Some(
                stage_history_file_field(facade, data_name, mime, field)
                    .await
                    .map_err(|err| map_route_error(err, "multipart file stream failed"))?,
            );
        } else if !name.is_empty() {
            let value = match field.text().await {
                Ok(value) => value,
                Err(err) => {
                    abort_parsed_history_file(facade, file.take()).await;
                    return Err(map_multipart_error(err, "multipart text read failed"));
                }
            };
            fields.insert(name, value);
        }
    }
    Ok(ParsedHistoryUpload { fields, file })
}

async fn stage_history_file_field(
    facade: &MobileSyncFacade,
    data_name: String,
    mime: String,
    mut field: Field<'_>,
) -> Result<ParsedHistoryFile, Response> {
    let transfer_id = format!("mobile-lan-history:{}", uuid::Uuid::new_v4());
    let scope_id = uc_application::facade::mobile_sync_streaming_scope_nonce();
    let handle = facade
        .begin_file_upload(&scope_id, &data_name, &mime)
        .await
        .map_err(|err| {
            tracing::warn!(
                data_name = %data_name,
                error = %err,
                "POST /api/history: begin file staging failed"
            );
            (StatusCode::INTERNAL_SERVER_ERROR, "staging begin failed").into_response()
        })?;

    let mut bytes_received = 0_u64;
    let mut sniff_window = Vec::with_capacity(64);
    loop {
        let chunk = match field.chunk().await {
            Ok(Some(chunk)) => chunk,
            Ok(None) => break,
            Err(err) => {
                facade.abort_file_upload(handle).await;
                return Err(map_multipart_error(err, "multipart file read failed"));
            }
        };
        bytes_received = bytes_received.saturating_add(chunk.len() as u64);
        if sniff_window.len() < 64 {
            let take = (64 - sniff_window.len()).min(chunk.len());
            sniff_window.extend_from_slice(&chunk[..take]);
        }
        if bytes_received > FILE_UPLOAD_DISK_SANITY_LIMIT as u64 {
            facade.abort_file_upload(handle).await;
            tracing::warn!(
                data_name = %data_name,
                bytes_received,
                limit = FILE_UPLOAD_DISK_SANITY_LIMIT,
                "POST /api/history: multipart file exceeded disk sanity limit"
            );
            return Err((StatusCode::PAYLOAD_TOO_LARGE, "body too large").into_response());
        }
        if let Err(err) = facade.append_file_chunk(&handle, &chunk).await {
            facade.abort_file_upload(handle).await;
            tracing::warn!(
                data_name = %data_name,
                error = %err,
                "POST /api/history: append file staging chunk failed"
            );
            return Err(
                (StatusCode::INTERNAL_SERVER_ERROR, "staging append failed").into_response()
            );
        }
    }

    let effective_mime = if mime_is_unspecific(&mime) {
        match infer_image_mime(&data_name, &sniff_window) {
            Some(sniffed) => {
                tracing::info!(
                    data_name = %data_name,
                    raw_mime = %mime,
                    sniffed_mime = sniffed,
                    "POST /api/history: overrode unspecific multipart image mime"
                );
                sniffed.to_string()
            }
            None => mime,
        }
    } else {
        mime
    };

    Ok(ParsedHistoryFile {
        data_name,
        mime: effective_mime,
        size: bytes_received,
        handle,
        transfer_id,
    })
}

async fn abort_parsed_history_file(facade: &MobileSyncFacade, file: Option<ParsedHistoryFile>) {
    if let Some(file) = file {
        facade.abort_file_upload(file.handle).await;
    }
}

fn map_multipart_error(err: MultipartError, context: &'static str) -> Response {
    let status = err.status();
    let detail = err.body_text();
    tracing::warn!(
        error = %err,
        error_detail = %detail,
        status = status.as_u16(),
        "POST /api/history: {context}"
    );
    (status, detail).into_response()
}

fn map_route_error(response: Response, context: &'static str) -> Response {
    tracing::warn!(
        status = response.status().as_u16(),
        "POST /api/history: {context}"
    );
    response
}
