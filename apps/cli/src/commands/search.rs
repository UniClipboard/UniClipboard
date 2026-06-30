//! Search command: query the index, inspect status, or trigger a rebuild via the daemon.

use clap::{Args, Subcommand};
use serde::Serialize;

use crate::commands::app_session::connect_with_lease;
use crate::exit_codes;
use crate::output;
use crate::ui;

use uc_daemon_client::{DaemonClientContext, DaemonRequestError, SearchQueryRequest};
use uc_daemon_contract::api::dto::search::{
    SearchQueryResultDto, SearchResultDto, SearchStatusData,
};

/// Query arguments accepted directly on `uniclip search <query>`.
#[derive(Args, Debug)]
pub struct SearchQueryArgs {
    /// Free-text query string
    query: Option<String>,
    /// Boolean operator: "and" or "or"
    #[arg(long)]
    operator: Option<String>,
    /// Time preset: today, yesterday, last_7d, last_30d
    #[arg(long = "time-preset")]
    time_preset: Option<String>,
    /// Start of absolute time range, in milliseconds since epoch
    #[arg(long = "from-ms")]
    from_ms: Option<i64>,
    /// End of absolute time range, in milliseconds since epoch
    #[arg(long = "to-ms")]
    to_ms: Option<i64>,
    /// Filter by content type (text, html, file, image, other); repeatable
    #[arg(long = "type")]
    content_types: Vec<String>,
    /// Filter by tag (link, favorited, or a custom tag id); repeatable
    #[arg(long = "tag")]
    tags: Vec<String>,
    /// Filter by file extension, for example md or txt; repeatable
    #[arg(long = "ext")]
    extensions: Vec<String>,
    /// Filter by source device — the device a clip arrived from. Accepts a
    /// device name (case-insensitive) or a device id; repeatable. Run
    /// `uniclip members` to see paired device names.
    #[arg(long = "source-device")]
    source_devices: Vec<String>,
    /// Maximum results to return
    #[arg(long, default_value_t = 50)]
    limit: u32,
    /// Result offset for pagination
    #[arg(long, default_value_t = 0)]
    offset: u32,
    /// Show detailed metadata for each result
    #[arg(long)]
    detailed: bool,
}

#[derive(Subcommand, Debug)]
pub enum SearchCommands {
    /// Show search index status
    Status,
    /// Trigger a search index rebuild on the daemon
    Rebuild,
}

pub async fn run(
    query: SearchQueryArgs,
    subcommand: Option<SearchCommands>,
    json: bool,
    verbose: bool,
) -> i32 {
    let (_lease, ctx) = match connect_with_lease(verbose).await {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let search = ctx.search_client();

    match subcommand {
        None => {
            // Resolve `--source-device` names/ids to canonical device ids before
            // anything else, so an unknown device fails fast (and a filter-only
            // browse by source still counts as a filter below).
            let source_devices = match resolve_source_devices(&ctx, &query.source_devices).await {
                Ok(ids) => ids,
                Err(code) => return code,
            };

            // An empty query browses; a filter alone (e.g. `--tag favorited`) is
            // enough to narrow it. Only a bare `search` with no query and no
            // filter is rejected, so the user is still guided to a query or the
            // status/rebuild subcommands.
            let has_filter = !query.tags.is_empty()
                || !query.content_types.is_empty()
                || !query.extensions.is_empty()
                || !source_devices.is_empty()
                || query.from_ms.is_some()
                || query.to_ms.is_some();
            let query_string = match query.query {
                Some(q) => q,
                None if has_filter => String::new(),
                None => {
                    ui::error(
                        "Missing search query. Run `uniclip search <query>`, narrow with `--tag`/`--type`/`--ext`, or `search status` / `search rebuild`.",
                    );
                    return exit_codes::EXIT_ERROR;
                }
            };

            if query.from_ms.is_some() != query.to_ms.is_some() {
                ui::error("--from-ms and --to-ms must be provided together");
                return exit_codes::EXIT_ERROR;
            }

            let req = SearchQueryRequest {
                query: query_string,
                operator: query.operator,
                time_preset: query.time_preset,
                from_ms: query.from_ms,
                to_ms: query.to_ms,
                content_types: query.content_types,
                tags: query.tags,
                extensions: query.extensions,
                source_devices,
                limit: query.limit,
                offset: query.offset,
            };

            let page = match search.query(req).await {
                Ok(page) => page,
                Err(err) => return render_search_error("query search index", err, json),
            };

            if json {
                output::emit_json(&SearchPageJsonDto::from(&page), "search query response")
            } else {
                println!("{}", render_query_output(&page, query.detailed));
                exit_codes::EXIT_SUCCESS
            }
        }
        Some(SearchCommands::Status) => {
            let status = match search.status().await {
                Ok(status) => status,
                Err(err) => return render_search_error("get search status", err, json),
            };

            if json {
                output::emit_json(
                    &SearchStatusJsonDto::from(&status),
                    "search status response",
                )
            } else {
                println!("{}", render_status_output(&status));
                exit_codes::EXIT_SUCCESS
            }
        }
        Some(SearchCommands::Rebuild) => {
            if let Err(err) = search.rebuild().await {
                return render_search_error("rebuild search index", err, json);
            }

            let status = match search.status().await {
                Ok(status) => status,
                Err(err) => return render_search_error("get search status", err, json),
            };

            if json {
                output::emit_json(
                    &SearchRebuildJsonDto {
                        accepted: true,
                        status: SearchStatusJsonDto::from(&status),
                    },
                    "search rebuild response",
                )
            } else {
                println!("Search rebuild accepted (runs in background).");
                println!("{}", render_status_output(&status));
                exit_codes::EXIT_SUCCESS
            }
        }
    }
}

fn render_search_error(action: &str, err: anyhow::Error, json: bool) -> i32 {
    if json {
        let (code, message) = match err.downcast_ref::<DaemonRequestError>() {
            Some(req_err) => (
                req_err.code().unwrap_or("unknown"),
                req_err
                    .message()
                    .map(|m| m.to_string())
                    .unwrap_or_else(|| err.to_string()),
            ),
            None => ("unknown", err.to_string()),
        };
        let dto = ErrorDto {
            code: code.to_string(),
            message,
        };
        if let Ok(value) = serde_json::to_string(&dto) {
            eprintln!("{value}");
        }
    } else {
        // Check for session_locked code in the structured error.
        let is_locked = err
            .downcast_ref::<DaemonRequestError>()
            .and_then(|e| e.code())
            .map(|c| c == "session_locked")
            .unwrap_or(false);

        if is_locked {
            ui::error(render_rebuild_locked_message());
        } else {
            ui::error(&format!("Failed to {action}: {err}"));
        }
    }
    exit_codes::EXIT_ERROR
}

fn render_rebuild_locked_message() -> &'static str {
    "Search is unavailable while the encryption session is locked. Unlock first, or run `uniclip status` to inspect application state."
}

/// A device that clips can arrive from: its display name and the canonical id
/// the search index stores. Built from the daemon's roster + mobile devices.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceDeviceEntry {
    id: String,
    name: String,
}

/// Resolution failure for a single `--source-device` value.
#[derive(Debug, PartialEq, Eq)]
enum SourceResolveError {
    /// No device name or id matched the input.
    Unknown(String),
    /// The input name matched more than one device; the user must use an id.
    Ambiguous { input: String, ids: Vec<String> },
}

/// Resolve `--source-device` inputs (a device name or id) to canonical device
/// ids against a known directory. Name matching is case-insensitive; an exact
/// id is also accepted. Pure so it can be unit-tested without a daemon.
fn resolve_source_inputs(
    inputs: &[String],
    directory: &[SourceDeviceEntry],
) -> Result<Vec<String>, SourceResolveError> {
    let mut resolved: Vec<String> = Vec::new();
    for input in inputs {
        let needle = input.to_lowercase();
        let mut name_ids: Vec<String> = directory
            .iter()
            .filter(|e| e.name.to_lowercase() == needle)
            .map(|e| e.id.clone())
            .collect();
        name_ids.sort();
        name_ids.dedup();

        let id = match name_ids.as_slice() {
            [single] => single.clone(),
            [] => {
                if directory.iter().any(|e| e.id == *input) {
                    input.clone()
                } else {
                    return Err(SourceResolveError::Unknown(input.clone()));
                }
            }
            _ => {
                return Err(SourceResolveError::Ambiguous {
                    input: input.clone(),
                    ids: name_ids,
                });
            }
        };

        if !resolved.contains(&id) {
            resolved.push(id);
        }
    }
    Ok(resolved)
}

/// Build the source-device directory from the daemon: the local device, paired
/// peers, and mobile-sync devices. Each sub-source is best-effort — a disabled
/// or empty subsystem contributes nothing rather than failing the whole lookup.
async fn fetch_source_directory(ctx: &DaemonClientContext) -> Vec<SourceDeviceEntry> {
    let mut directory: Vec<SourceDeviceEntry> = Vec::new();
    let query = ctx.query_client();

    if let Ok(local) = query.get_local_device_info().await {
        directory.push(SourceDeviceEntry {
            id: local.peer_id,
            name: local.device_name,
        });
    }

    match query.get_paired_devices().await {
        Ok(members) => {
            for member in members {
                directory.push(SourceDeviceEntry {
                    id: member.peer_id,
                    name: member.device_name,
                });
            }
        }
        Err(err) => {
            tracing::debug!(error = %err, "could not list paired devices for source resolution");
        }
    }

    match ctx.mobile_sync_client().list_devices().await {
        Ok(devices) => {
            for device in devices {
                directory.push(SourceDeviceEntry {
                    id: format!("mobile_sync:{}", device.device_id),
                    name: device.label,
                });
            }
        }
        Err(err) => {
            tracing::debug!(error = %err, "could not list mobile devices for source resolution");
        }
    }

    directory
}

/// Fetch the source-device directory and resolve `inputs` to device ids,
/// rendering a helpful error (and returning a non-zero exit code) on failure.
/// An empty `inputs` short-circuits without contacting the daemon.
async fn resolve_source_devices(
    ctx: &DaemonClientContext,
    inputs: &[String],
) -> Result<Vec<String>, i32> {
    if inputs.is_empty() {
        return Ok(Vec::new());
    }

    let directory = fetch_source_directory(ctx).await;

    match resolve_source_inputs(inputs, &directory) {
        Ok(ids) => Ok(ids),
        Err(SourceResolveError::Unknown(input)) => {
            ui::error(&format!("Unknown source device: '{input}'"));
            render_available_sources(&directory);
            Err(exit_codes::EXIT_ERROR)
        }
        Err(SourceResolveError::Ambiguous { input, ids }) => {
            ui::error(&format!(
                "Source device name '{input}' is ambiguous; pass an id instead."
            ));
            for id in &ids {
                ui::info("id", id);
            }
            Err(exit_codes::EXIT_ERROR)
        }
    }
}

/// List the known source devices (name → id) to help the user correct an
/// unknown `--source-device` value.
fn render_available_sources(directory: &[SourceDeviceEntry]) {
    if directory.is_empty() {
        ui::info("devices", "no known source devices; run `uniclip members`");
        return;
    }
    ui::info("devices", "available source devices (name → id):");
    for entry in directory {
        ui::info(&entry.name, &entry.id);
    }
}

fn format_search_timestamp(ts_ms: i64) -> String {
    use chrono::{TimeZone, Utc};
    match Utc.timestamp_millis_opt(ts_ms) {
        chrono::LocalResult::Single(dt) => dt.format("%Y-%m-%d %H:%M").to_string(),
        _ => format!("<invalid timestamp: {ts_ms}>"),
    }
}

fn render_query_output(response: &SearchQueryResultDto, detailed: bool) -> String {
    let total = response.total;
    let showing_from = response.items.len().min(1);
    let showing_to = response.items.len();
    let mut lines = vec![format!(
        "Search results: {total} total (showing {showing_from}-{showing_to})"
    )];

    if response.items.is_empty() {
        lines.push("No search results found.".to_string());
        lines.push("Try widening the time range.".to_string());
        lines.push("Try removing one or more filters.".to_string());
        lines.push("Try a fuller token; search is exact-token in V1.".to_string());
        return lines.join("\n");
    }

    for item in &response.items {
        let formatted_time = format_search_timestamp(item.active_time_ms);
        let preview = item.text_preview.as_deref().unwrap_or("<no preview>");
        lines.push(format!(
            "- [{}] {}  {}",
            item.content_type, formatted_time, preview
        ));

        if detailed {
            lines.push(format!("    entryId: {}", item.entry_id));
            lines.push(format!("    mimeType: {}", item.mime_type));
            let extensions = if item.file_extensions.is_empty() {
                "<none>".to_string()
            } else {
                item.file_extensions.join(",")
            };
            lines.push(format!("    extensions: {extensions}"));
            if let Some(source) = &item.source_device {
                lines.push(format!("    source: {source}"));
            }
        }
    }

    lines.join("\n")
}

fn render_status_output(response: &SearchStatusData) -> String {
    let reason = response.reason.as_deref().unwrap_or("none");
    let last_started = response
        .last_rebuild_started_at_ms
        .map(format_search_timestamp)
        .unwrap_or_else(|| "never".to_string());
    let last_completed = response
        .last_rebuild_completed_at_ms
        .map(format_search_timestamp)
        .unwrap_or_else(|| "never".to_string());

    vec![
        format!("Search state: {}", response.state),
        format!("Reason: {reason}"),
        format!("Last rebuild started: {last_started}"),
        format!("Last rebuild completed: {last_completed}"),
    ]
    .join("\n")
}

/// JSON output DTO for query results — preserves field names for backwards compatibility.
#[derive(Serialize)]
struct SearchPageJsonDto {
    total: u32,
    has_more: bool,
    data: Vec<SearchResultItemJsonDto>,
}

impl From<&SearchQueryResultDto> for SearchPageJsonDto {
    fn from(value: &SearchQueryResultDto) -> Self {
        Self {
            total: value.total,
            has_more: value.has_more,
            data: value
                .items
                .iter()
                .map(SearchResultItemJsonDto::from)
                .collect(),
        }
    }
}

#[derive(Serialize)]
struct SearchResultItemJsonDto {
    entry_id: String,
    content_type: String,
    active_time_ms: i64,
    text_preview: Option<String>,
    mime_type: String,
    file_extensions: Vec<String>,
    /// Originating device id, or `null` when the source is unknown / local.
    source_device: Option<String>,
}

impl From<&SearchResultDto> for SearchResultItemJsonDto {
    fn from(value: &SearchResultDto) -> Self {
        Self {
            entry_id: value.entry_id.clone(),
            content_type: value.content_type.clone(),
            active_time_ms: value.active_time_ms,
            text_preview: value.text_preview.clone(),
            mime_type: value.mime_type.clone(),
            file_extensions: value.file_extensions.clone(),
            source_device: value.source_device.clone(),
        }
    }
}

#[derive(Serialize)]
struct SearchStatusJsonDto {
    state: String,
    reason: Option<String>,
    last_rebuild_started_at_ms: Option<i64>,
    last_rebuild_completed_at_ms: Option<i64>,
}

impl From<&SearchStatusData> for SearchStatusJsonDto {
    fn from(value: &SearchStatusData) -> Self {
        Self {
            state: value.state.clone(),
            reason: value.reason.clone(),
            last_rebuild_started_at_ms: value.last_rebuild_started_at_ms,
            last_rebuild_completed_at_ms: value.last_rebuild_completed_at_ms,
        }
    }
}

#[derive(Serialize)]
struct SearchRebuildJsonDto {
    accepted: bool,
    status: SearchStatusJsonDto,
}

#[derive(Serialize)]
struct ErrorDto {
    code: String,
    message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directory() -> Vec<SourceDeviceEntry> {
        vec![
            SourceDeviceEntry {
                id: "peer-laptop".into(),
                name: "Laptop".into(),
            },
            SourceDeviceEntry {
                id: "mobile_sync:abc".into(),
                name: "Pixel".into(),
            },
            SourceDeviceEntry {
                id: "peer-a".into(),
                name: "Shared".into(),
            },
            SourceDeviceEntry {
                id: "peer-b".into(),
                name: "Shared".into(),
            },
        ]
    }

    #[test]
    fn resolves_name_case_insensitively() {
        let got = resolve_source_inputs(&["laptop".into()], &directory()).unwrap();
        assert_eq!(got, vec!["peer-laptop".to_string()]);
    }

    #[test]
    fn accepts_exact_id() {
        let got = resolve_source_inputs(&["mobile_sync:abc".into()], &directory()).unwrap();
        assert_eq!(got, vec!["mobile_sync:abc".to_string()]);
    }

    #[test]
    fn rejects_unknown_device() {
        let err = resolve_source_inputs(&["nope".into()], &directory()).unwrap_err();
        assert_eq!(err, SourceResolveError::Unknown("nope".into()));
    }

    #[test]
    fn rejects_ambiguous_name() {
        let err = resolve_source_inputs(&["shared".into()], &directory()).unwrap_err();
        assert_eq!(
            err,
            SourceResolveError::Ambiguous {
                input: "shared".into(),
                ids: vec!["peer-a".into(), "peer-b".into()],
            }
        );
    }

    #[test]
    fn dedups_resolved_ids() {
        let got =
            resolve_source_inputs(&["Laptop".into(), "peer-laptop".into()], &directory()).unwrap();
        assert_eq!(got, vec!["peer-laptop".to_string()]);
    }

    #[test]
    fn empty_inputs_resolve_to_empty() {
        let got = resolve_source_inputs(&[], &directory()).unwrap();
        assert!(got.is_empty());
    }
}
