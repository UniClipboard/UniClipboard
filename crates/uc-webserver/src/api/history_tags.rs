//! Local history tag endpoints (`/history/tags`).
//!
//! Thin transport over the Engine's history-tag operations: the Engine owns the
//! tags, their sealed associations, the search-index membership and the session
//! lock check. Tags stay in this device's history and are never synced.
//!
//! The tag layout — the sidebar's tags in order and each tag's color — is the
//! daemon's own ([`crate::api::tag_layout`]). Its routes ask the Engine for the
//! current tags first, so they are locked whenever the tags are, and the
//! delete and merge handlers keep it in step after the Engine has changed.
//!
//! Error mapping: 1401 → 400, 1402 → 404, 1405 (locked) → 423 `session_locked`
//! (the code the search endpoints already emit), 1406 (legacy profile or no
//! active space) → 503 `runtime_unavailable`, anything else → 500.

use std::collections::HashSet;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, patch, post, put};
use axum::{Json, Router};
use tracing::{debug, info, instrument, warn};
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use uc_daemon_contract::constants::http_route;
use uc_engine::error_codes::{
    HISTORY_FAILED_CODE, HISTORY_INVALID_INPUT_CODE, HISTORY_NOT_FOUND_CODE,
    HISTORY_TAGS_LOCKED_CODE, HISTORY_TAGS_UNAVAILABLE_CODE,
};
use uc_engine::{
    CreateHistoryTagInput, EngineError, HistoryEntryTagsInput, HistoryTagEntriesInput,
    HistoryTagInput, MergeHistoryTagsInput, Operation, OperationResult, RenameHistoryTagInput,
};

use crate::api::dto::error::{log_facade_failure, ApiError};
use crate::api::dto::history_tags::{
    CreateHistoryTagRequest, HistoryEntryTagSummaryDto, HistoryTagBatchResultDto,
    HistoryTagCreatedDto, HistoryTagDeletedDto, HistoryTagDto, HistoryTagEntriesRequest,
    HistoryTagLayoutDto, HistoryTagMergeResultDto, HistoryTagRenameResultDto,
    MergeHistoryTagsRequest, RenameHistoryTagRequest, SetHistoryTagColorRequest,
    SetHistoryTagInSidebarRequest, SetHistoryTagSidebarRequest,
};
use crate::api::projection::IntoApiDto;
use crate::api::server::DaemonApiState;
use crate::api::tag_layout::TagLayoutId;

/// Build the history-tag sub-router. Mounted under the L2+ protected chain;
/// `/history/` is a content route, so the GUI content lock applies too.
pub fn router() -> Router<DaemonApiState> {
    let tag = format!("{}/:tag_id", http_route::HISTORY_TAGS);
    Router::new()
        .route(
            http_route::HISTORY_TAGS,
            get(list_history_tags).post(create_history_tag),
        )
        .route(http_route::HISTORY_TAGS_SUMMARY, post(summarize_entry_tags))
        .route(http_route::HISTORY_TAGS_LAYOUT, get(get_history_tag_layout))
        .route(
            http_route::HISTORY_TAGS_LAYOUT_SIDEBAR,
            put(set_history_tag_sidebar),
        )
        .route(&tag, patch(rename_history_tag).delete(delete_history_tag))
        .route(&format!("{tag}/color"), put(set_history_tag_color))
        .route(&format!("{tag}/sidebar"), put(set_history_tag_in_sidebar))
        .route(&format!("{tag}/entries/add"), post(add_tag_to_entries))
        .route(
            &format!("{tag}/entries/remove"),
            post(remove_tag_from_entries),
        )
        .route(&format!("{tag}/merge"), post(merge_history_tags))
}

fn map_history_tag_engine_err(op: &'static str, error: EngineError) -> ApiError {
    let (variant, api): (&'static str, ApiError) = match error.code() {
        HISTORY_INVALID_INPUT_CODE => ("invalid_input", ApiError::bad_request("invalid input")),
        HISTORY_NOT_FOUND_CODE => ("not_found", ApiError::not_found("history tag not found")),
        HISTORY_TAGS_LOCKED_CODE => (
            "session_locked",
            ApiError {
                status: StatusCode::LOCKED,
                code: "session_locked".to_string(),
                message: "encryption session is locked".to_string(),
                details: None,
            },
        ),
        HISTORY_TAGS_UNAVAILABLE_CODE => (
            "unavailable",
            ApiError::service_unavailable("history tags are unavailable for this profile"),
        ),
        HISTORY_FAILED_CODE => (
            "internal",
            ApiError::internal("history tag operation failed"),
        ),
        _ => (
            "unexpected_engine_error",
            ApiError::internal("history tag operation failed"),
        ),
    };
    log_facade_failure("history_tags", op, variant, api.status, &api.message);
    api
}

fn unexpected(op: &str) -> ApiError {
    ApiError::internal(format!("engine returned an unexpected {op} result"))
}

fn json_body<T>(body: Result<Json<T>, JsonRejection>) -> Result<T, ApiError> {
    body.map(|Json(body)| body)
        .map_err(|_| ApiError::bad_request("invalid request body"))
}

/// GET /history/tags
///
/// This device's tags with their association counts, most used first.
#[utoipa::path(
    get,
    path = "/history/tags",
    tag = "history-tags",
    operation_id = "listHistoryTags",
    responses(
        (status = 200, description = "Local history tags", body = HistoryTagsEnvelope),
        (status = 423, description = "Encryption session is locked", body = ApiErrorResponse),
        (status = 503, description = "Tags unavailable for this profile", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
#[instrument(name = "api.history_tags.list", level = "info", skip(state))]
async fn list_history_tags(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<Vec<HistoryTagDto>>>, ApiError> {
    let result = state
        .execute(Operation::ListHistoryTags)
        .await
        .map_err(|error| map_history_tag_engine_err("list", error))?;
    let OperationResult::HistoryTags(tags) = result else {
        return Err(unexpected("history-tags"));
    };
    debug!(tag_count = tags.len(), "history tags listed");
    Ok(Json(ApiEnvelope::now(
        tags.into_iter().map(IntoApiDto::into_api_dto).collect(),
    )))
}

/// POST /history/tags
///
/// Create a tag, or return the existing one with the same name.
#[utoipa::path(
    post,
    path = "/history/tags",
    tag = "history-tags",
    operation_id = "createHistoryTag",
    request_body = CreateHistoryTagRequest,
    responses(
        (status = 200, description = "Tag created, or the existing tag", body = HistoryTagCreatedEnvelope),
        (status = 400, description = "Invalid name or tag limit reached", body = ApiErrorResponse),
        (status = 423, description = "Encryption session is locked", body = ApiErrorResponse),
        (status = 503, description = "Tags unavailable for this profile", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
#[instrument(name = "api.history_tags.create", level = "info", skip_all)]
async fn create_history_tag(
    State(state): State<DaemonApiState>,
    body: Result<Json<CreateHistoryTagRequest>, JsonRejection>,
) -> Result<Json<ApiEnvelope<HistoryTagCreatedDto>>, ApiError> {
    let body = json_body(body)?;
    let result = state
        .execute(Operation::CreateHistoryTag(CreateHistoryTagInput {
            name: body.name,
        }))
        .await
        .map_err(|error| map_history_tag_engine_err("create", error))?;
    let OperationResult::HistoryTagCreated(created) = result else {
        return Err(unexpected("history-tag-created"));
    };
    info!(created = created.created, "history tag create-or-get");
    // An existing tag returned by create-or-get keeps its color.
    if let (true, Some(color)) = (created.created, body.color) {
        match TagLayoutId::parse(created.tag.tag_id.as_str()) {
            Some(id) => {
                if let Err(error) = state.tag_layout.set_color(&id, Some(color)).await {
                    warn!(error = %error, "failed to store the new tag's color");
                }
            }
            None => warn!("engine returned a tag id the tag layout cannot hold"),
        }
    }
    Ok(Json(ApiEnvelope::now(created.into_api_dto())))
}

/// PATCH /history/tags/{tag_id}
///
/// Rename a tag. A name held by another tag is not written: the result says
/// `name_conflict` with that tag's id (HTTP 200).
#[utoipa::path(
    patch,
    path = "/history/tags/{tag_id}",
    tag = "history-tags",
    operation_id = "renameHistoryTag",
    params(("tag_id" = String, Path, description = "Tag id")),
    request_body = RenameHistoryTagRequest,
    responses(
        (status = 200, description = "Renamed, or the conflicting tag", body = HistoryTagRenameEnvelope),
        (status = 400, description = "Invalid name", body = ApiErrorResponse),
        (status = 404, description = "Tag not found", body = ApiErrorResponse),
        (status = 423, description = "Encryption session is locked", body = ApiErrorResponse),
        (status = 503, description = "Tags unavailable for this profile", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
#[instrument(name = "api.history_tags.rename", level = "info", skip_all)]
async fn rename_history_tag(
    State(state): State<DaemonApiState>,
    Path(tag_id): Path<String>,
    body: Result<Json<RenameHistoryTagRequest>, JsonRejection>,
) -> Result<Json<ApiEnvelope<HistoryTagRenameResultDto>>, ApiError> {
    let body = json_body(body)?;
    let result = state
        .execute(Operation::RenameHistoryTag(RenameHistoryTagInput {
            tag_id,
            name: body.name,
        }))
        .await
        .map_err(|error| map_history_tag_engine_err("rename", error))?;
    let OperationResult::HistoryTagRenamed(renamed) = result else {
        return Err(unexpected("history-tag-renamed"));
    };
    Ok(Json(ApiEnvelope::now(renamed.into_api_dto())))
}

/// DELETE /history/tags/{tag_id}
///
/// Delete a tag; its entries stay.
#[utoipa::path(
    delete,
    path = "/history/tags/{tag_id}",
    tag = "history-tags",
    operation_id = "deleteHistoryTag",
    params(("tag_id" = String, Path, description = "Tag id")),
    responses(
        (status = 200, description = "Tag deleted", body = HistoryTagDeletedEnvelope),
        (status = 404, description = "Tag not found", body = ApiErrorResponse),
        (status = 423, description = "Encryption session is locked", body = ApiErrorResponse),
        (status = 503, description = "Tags unavailable for this profile", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
#[instrument(name = "api.history_tags.delete", level = "info", skip_all)]
async fn delete_history_tag(
    State(state): State<DaemonApiState>,
    Path(tag_id): Path<String>,
) -> Result<Json<ApiEnvelope<HistoryTagDeletedDto>>, ApiError> {
    let layout_id = TagLayoutId::parse(&tag_id);
    let result = state
        .execute(Operation::DeleteHistoryTag(HistoryTagInput { tag_id }))
        .await
        .map_err(|error| map_history_tag_engine_err("delete", error))?;
    let OperationResult::HistoryTagDeleted(deleted) = result else {
        return Err(unexpected("history-tag-deleted"));
    };
    info!(detached = deleted.detached, "history tag deleted");
    // The next layout read prunes it anyway; this only makes it immediate.
    if let Some(id) = layout_id {
        if let Err(error) = state.tag_layout.forget(&[id]).await {
            warn!(error = %error, "failed to drop a deleted tag from the tag layout");
        }
    }
    Ok(Json(ApiEnvelope::now(deleted.into_api_dto())))
}

/// POST /history/tags/{tag_id}/entries/add
///
/// Attach the tag to entries (1..=1000). Already-tagged entries are
/// `unchanged`; missing entries are skipped and listed.
#[utoipa::path(
    post,
    path = "/history/tags/{tag_id}/entries/add",
    tag = "history-tags",
    operation_id = "addHistoryTagToEntries",
    params(("tag_id" = String, Path, description = "Tag id")),
    request_body = HistoryTagEntriesRequest,
    responses(
        (status = 200, description = "Batch applied", body = HistoryTagBatchEnvelope),
        (status = 400, description = "Empty or oversized batch", body = ApiErrorResponse),
        (status = 404, description = "Tag not found", body = ApiErrorResponse),
        (status = 423, description = "Encryption session is locked", body = ApiErrorResponse),
        (status = 503, description = "Tags unavailable for this profile", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
#[instrument(name = "api.history_tags.add", level = "info", skip_all)]
async fn add_tag_to_entries(
    State(state): State<DaemonApiState>,
    Path(tag_id): Path<String>,
    body: Result<Json<HistoryTagEntriesRequest>, JsonRejection>,
) -> Result<Json<ApiEnvelope<HistoryTagBatchResultDto>>, ApiError> {
    let body = json_body(body)?;
    let result = state
        .execute(Operation::AddHistoryTagToEntries(HistoryTagEntriesInput {
            tag_id,
            entry_ids: body.entry_ids,
        }))
        .await
        .map_err(|error| map_history_tag_engine_err("add", error))?;
    let OperationResult::HistoryTagEntriesChanged(batch) = result else {
        return Err(unexpected("history-tag-entries"));
    };
    Ok(Json(ApiEnvelope::now(batch.into_api_dto())))
}

/// POST /history/tags/{tag_id}/entries/remove
///
/// Detach the tag from entries (1..=1000), with the same batch semantics as add.
#[utoipa::path(
    post,
    path = "/history/tags/{tag_id}/entries/remove",
    tag = "history-tags",
    operation_id = "removeHistoryTagFromEntries",
    params(("tag_id" = String, Path, description = "Tag id")),
    request_body = HistoryTagEntriesRequest,
    responses(
        (status = 200, description = "Batch applied", body = HistoryTagBatchEnvelope),
        (status = 400, description = "Empty or oversized batch", body = ApiErrorResponse),
        (status = 404, description = "Tag not found", body = ApiErrorResponse),
        (status = 423, description = "Encryption session is locked", body = ApiErrorResponse),
        (status = 503, description = "Tags unavailable for this profile", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
#[instrument(name = "api.history_tags.remove", level = "info", skip_all)]
async fn remove_tag_from_entries(
    State(state): State<DaemonApiState>,
    Path(tag_id): Path<String>,
    body: Result<Json<HistoryTagEntriesRequest>, JsonRejection>,
) -> Result<Json<ApiEnvelope<HistoryTagBatchResultDto>>, ApiError> {
    let body = json_body(body)?;
    let result = state
        .execute(Operation::RemoveHistoryTagFromEntries(
            HistoryTagEntriesInput {
                tag_id,
                entry_ids: body.entry_ids,
            },
        ))
        .await
        .map_err(|error| map_history_tag_engine_err("remove", error))?;
    let OperationResult::HistoryTagEntriesChanged(batch) = result else {
        return Err(unexpected("history-tag-entries"));
    };
    Ok(Json(ApiEnvelope::now(batch.into_api_dto())))
}

/// POST /history/tags/summary
///
/// Which tags a selection of entries (1..=1000) carries, for partial tag state.
#[utoipa::path(
    post,
    path = "/history/tags/summary",
    tag = "history-tags",
    operation_id = "summarizeHistoryEntryTags",
    request_body = HistoryTagEntriesRequest,
    responses(
        (status = 200, description = "Tags on the selection", body = HistoryEntryTagSummaryEnvelope),
        (status = 400, description = "Empty or oversized selection", body = ApiErrorResponse),
        (status = 423, description = "Encryption session is locked", body = ApiErrorResponse),
        (status = 503, description = "Tags unavailable for this profile", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
#[instrument(name = "api.history_tags.summary", level = "info", skip_all)]
async fn summarize_entry_tags(
    State(state): State<DaemonApiState>,
    body: Result<Json<HistoryTagEntriesRequest>, JsonRejection>,
) -> Result<Json<ApiEnvelope<HistoryEntryTagSummaryDto>>, ApiError> {
    let body = json_body(body)?;
    let result = state
        .execute(Operation::SummarizeHistoryEntryTags(
            HistoryEntryTagsInput {
                entry_ids: body.entry_ids,
            },
        ))
        .await
        .map_err(|error| map_history_tag_engine_err("summary", error))?;
    let OperationResult::HistoryEntryTags(summary) = result else {
        return Err(unexpected("history-entry-tags"));
    };
    Ok(Json(ApiEnvelope::now(summary.into_api_dto())))
}

/// POST /history/tags/{tag_id}/merge
///
/// Fold the source tags (1..=100) into the path tag, then delete them.
#[utoipa::path(
    post,
    path = "/history/tags/{tag_id}/merge",
    tag = "history-tags",
    operation_id = "mergeHistoryTags",
    params(("tag_id" = String, Path, description = "Target tag id")),
    request_body = MergeHistoryTagsRequest,
    responses(
        (status = 200, description = "Tags merged", body = HistoryTagMergeEnvelope),
        (status = 400, description = "Invalid sources (empty, too many, or the target itself)", body = ApiErrorResponse),
        (status = 404, description = "A tag was not found", body = ApiErrorResponse),
        (status = 423, description = "Encryption session is locked", body = ApiErrorResponse),
        (status = 503, description = "Tags unavailable for this profile", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
#[instrument(name = "api.history_tags.merge", level = "info", skip_all)]
async fn merge_history_tags(
    State(state): State<DaemonApiState>,
    Path(tag_id): Path<String>,
    body: Result<Json<MergeHistoryTagsRequest>, JsonRejection>,
) -> Result<Json<ApiEnvelope<HistoryTagMergeResultDto>>, ApiError> {
    let body = json_body(body)?;
    let layout_target = TagLayoutId::parse(&tag_id);
    let layout_sources: Vec<TagLayoutId> = body
        .source_tag_ids
        .iter()
        .filter_map(|id| TagLayoutId::parse(id))
        .collect();
    let result = state
        .execute(Operation::MergeHistoryTags(MergeHistoryTagsInput {
            source_tag_ids: body.source_tag_ids,
            target_tag_id: tag_id,
        }))
        .await
        .map_err(|error| map_history_tag_engine_err("merge", error))?;
    let OperationResult::HistoryTagsMerged(merged) = result else {
        return Err(unexpected("history-tags-merged"));
    };
    info!(
        moved = merged.moved,
        already_on_target = merged.already_on_target,
        "history tags merged"
    );
    if let Some(target) = layout_target {
        if let Err(error) = state.tag_layout.merge(&target, &layout_sources).await {
            warn!(error = %error, "failed to fold merged tags in the tag layout");
        }
    }
    Ok(Json(ApiEnvelope::now(merged.into_api_dto())))
}

// ── Tag layout ──────────────────────────────────────────────────────────────

/// The Engine's current local tag ids. Also the lock check of the layout
/// routes: a locked session fails here with the tags' own 423.
async fn local_tag_ids(
    state: &DaemonApiState,
    op: &'static str,
) -> Result<HashSet<String>, ApiError> {
    let result = state
        .execute(Operation::ListHistoryTags)
        .await
        .map_err(|error| map_history_tag_engine_err(op, error))?;
    let OperationResult::HistoryTags(tags) = result else {
        return Err(unexpected("history-tags"));
    };
    Ok(tags.into_iter().map(|tag| tag.tag_id).collect())
}

/// `raw` as a layout id of a tag that exists: a builtin tag, or one of `local`.
fn existing_layout_id(raw: &str, local: &HashSet<String>) -> Result<TagLayoutId, ApiError> {
    match TagLayoutId::parse(raw) {
        Some(id) if id.is_builtin() || local.contains(raw) => Ok(id),
        Some(_) => Err(ApiError::not_found("history tag not found")),
        None => Err(ApiError::bad_request("not a tag the layout can hold")),
    }
}

fn layout_failure(op: &'static str, error: std::io::Error) -> ApiError {
    let api = ApiError::internal("tag layout could not be saved");
    log_facade_failure(
        "history_tags",
        op,
        "layout_io",
        api.status,
        &error.to_string(),
    );
    api
}

/// GET /history/tags/layout
///
/// The tags the History sidebar shows, in order, and each tag's color.
#[utoipa::path(
    get,
    path = "/history/tags/layout",
    tag = "history-tags",
    operation_id = "getHistoryTagLayout",
    responses(
        (status = 200, description = "Sidebar tags and colors", body = HistoryTagLayoutEnvelope),
        (status = 423, description = "Encryption session is locked", body = ApiErrorResponse),
        (status = 503, description = "Tags unavailable for this profile", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
#[instrument(name = "api.history_tags.layout", level = "info", skip(state))]
async fn get_history_tag_layout(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<HistoryTagLayoutDto>>, ApiError> {
    let local = local_tag_ids(&state, "layout").await?;
    let layout = state
        .tag_layout
        .snapshot(&local)
        .await
        .map_err(|error| layout_failure("layout", error))?;
    Ok(Json(ApiEnvelope::now(layout)))
}

/// PUT /history/tags/layout/sidebar
///
/// Replace the sidebar's tags and their order. Duplicates are collapsed.
#[utoipa::path(
    put,
    path = "/history/tags/layout/sidebar",
    tag = "history-tags",
    operation_id = "setHistoryTagSidebar",
    request_body = SetHistoryTagSidebarRequest,
    responses(
        (status = 200, description = "The updated layout", body = HistoryTagLayoutEnvelope),
        (status = 400, description = "An id the layout cannot hold", body = ApiErrorResponse),
        (status = 404, description = "A tag was not found", body = ApiErrorResponse),
        (status = 423, description = "Encryption session is locked", body = ApiErrorResponse),
        (status = 503, description = "Tags unavailable for this profile", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
#[instrument(name = "api.history_tags.sidebar", level = "info", skip_all)]
async fn set_history_tag_sidebar(
    State(state): State<DaemonApiState>,
    body: Result<Json<SetHistoryTagSidebarRequest>, JsonRejection>,
) -> Result<Json<ApiEnvelope<HistoryTagLayoutDto>>, ApiError> {
    let body = json_body(body)?;
    let local = local_tag_ids(&state, "sidebar").await?;
    let ids = body
        .tag_ids
        .iter()
        .map(|raw| existing_layout_id(raw, &local))
        .collect::<Result<Vec<_>, _>>()?;
    let layout = state
        .tag_layout
        .set_sidebar(ids)
        .await
        .map_err(|error| layout_failure("sidebar", error))?;
    info!(sidebar_len = layout.sidebar.len(), "tag sidebar replaced");
    Ok(Json(ApiEnvelope::now(layout)))
}

/// PUT /history/tags/{tag_id}/color
///
/// Set a tag's color; `null` clears it (a builtin tag returns to its default).
#[utoipa::path(
    put,
    path = "/history/tags/{tag_id}/color",
    tag = "history-tags",
    operation_id = "setHistoryTagColor",
    params(("tag_id" = String, Path, description = "Tag id (local or builtin)")),
    request_body = SetHistoryTagColorRequest,
    responses(
        (status = 200, description = "The updated layout", body = HistoryTagLayoutEnvelope),
        (status = 400, description = "An id the layout cannot hold", body = ApiErrorResponse),
        (status = 404, description = "Tag not found", body = ApiErrorResponse),
        (status = 423, description = "Encryption session is locked", body = ApiErrorResponse),
        (status = 503, description = "Tags unavailable for this profile", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
#[instrument(name = "api.history_tags.color", level = "info", skip_all)]
async fn set_history_tag_color(
    State(state): State<DaemonApiState>,
    Path(tag_id): Path<String>,
    body: Result<Json<SetHistoryTagColorRequest>, JsonRejection>,
) -> Result<Json<ApiEnvelope<HistoryTagLayoutDto>>, ApiError> {
    let body = json_body(body)?;
    let local = local_tag_ids(&state, "color").await?;
    let id = existing_layout_id(&tag_id, &local)?;
    let layout = state
        .tag_layout
        .set_color(&id, body.color)
        .await
        .map_err(|error| layout_failure("color", error))?;
    Ok(Json(ApiEnvelope::now(layout)))
}

/// PUT /history/tags/{tag_id}/sidebar
///
/// Show a tag in the sidebar (last) or take it out.
#[utoipa::path(
    put,
    path = "/history/tags/{tag_id}/sidebar",
    tag = "history-tags",
    operation_id = "setHistoryTagInSidebar",
    params(("tag_id" = String, Path, description = "Tag id (local or builtin)")),
    request_body = SetHistoryTagInSidebarRequest,
    responses(
        (status = 200, description = "The updated layout", body = HistoryTagLayoutEnvelope),
        (status = 400, description = "An id the layout cannot hold", body = ApiErrorResponse),
        (status = 404, description = "Tag not found", body = ApiErrorResponse),
        (status = 423, description = "Encryption session is locked", body = ApiErrorResponse),
        (status = 503, description = "Tags unavailable for this profile", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
#[instrument(name = "api.history_tags.in_sidebar", level = "info", skip_all)]
async fn set_history_tag_in_sidebar(
    State(state): State<DaemonApiState>,
    Path(tag_id): Path<String>,
    body: Result<Json<SetHistoryTagInSidebarRequest>, JsonRejection>,
) -> Result<Json<ApiEnvelope<HistoryTagLayoutDto>>, ApiError> {
    let body = json_body(body)?;
    let local = local_tag_ids(&state, "in_sidebar").await?;
    let id = existing_layout_id(&tag_id, &local)?;
    let layout = state
        .tag_layout
        .set_in_sidebar(&id, body.in_sidebar)
        .await
        .map_err(|error| layout_failure("in_sidebar", error))?;
    Ok(Json(ApiEnvelope::now(layout)))
}

#[cfg(test)]
mod layout_id_tests {
    use super::*;

    #[test]
    fn layout_routes_accept_builtins_and_existing_local_tags_only() {
        let local_id = "0b9e3c1a-5f2d-4c8e-9a7b-1d2e3f4a5b6c";
        let gone_id = "7c6d5e4f-3a2b-4c1d-8e9f-0a1b2c3d4e5f";
        let local = HashSet::from([local_id.to_string()]);
        assert!(existing_layout_id("code", &local).is_ok());
        assert!(existing_layout_id(local_id, &local).is_ok());
        assert_eq!(
            existing_layout_id(gone_id, &local).unwrap_err().status,
            StatusCode::NOT_FOUND
        );
        for raw in ["favorited", "deploy"] {
            assert_eq!(
                existing_layout_id(raw, &local).unwrap_err().status,
                StatusCode::BAD_REQUEST
            );
        }
    }
}
