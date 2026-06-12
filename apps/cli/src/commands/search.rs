//! Search 命令:通过 daemon 查询、查看状态、触发重建本地索引。

use clap::Subcommand;
use serde::Serialize;

use crate::commands::app_session::connect_or_spawn_oneshot_daemon;
use crate::exit_codes;
use crate::ui;

use uc_daemon_client::{DaemonClientContext, DaemonRequestError, SearchQueryRequest};
use uc_daemon_contract::api::dto::search::{
    SearchQueryResultDto, SearchResultDto, SearchStatusData,
};

#[derive(Subcommand, Debug)]
pub enum SearchCommands {
    /// Query the search index
    Query {
        /// Free-text query string
        query: String,
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
        /// Filter by content type (text, html, link, file, image, other); repeatable
        #[arg(long = "type")]
        content_types: Vec<String>,
        /// Filter by file extension, for example md or txt; repeatable
        #[arg(long = "ext")]
        extensions: Vec<String>,
        /// Maximum results to return
        #[arg(long, default_value_t = 50)]
        limit: u32,
        /// Result offset for pagination
        #[arg(long, default_value_t = 0)]
        offset: u32,
        /// Show detailed metadata for each result
        #[arg(long)]
        detailed: bool,
    },
    /// Show search index status
    Status,
    /// Trigger a search index rebuild on the daemon
    Rebuild,
}

pub async fn run(subcommand: SearchCommands, json: bool, verbose: bool) -> i32 {
    let service = match connect_or_spawn_oneshot_daemon(verbose).await {
        Ok(s) => s,
        Err(code) => return code,
    };

    let _lease = match service.hold_control_lease().await {
        Ok(guard) => guard,
        Err(err) => {
            ui::error(&format!("Failed to hold daemon session lease: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    let ctx = match DaemonClientContext::from_env() {
        Ok(ctx) => ctx,
        Err(err) => {
            ui::error(&format!("Failed to connect to daemon: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };
    let search = ctx.search_client();

    match subcommand {
        SearchCommands::Query {
            query,
            operator,
            time_preset,
            from_ms,
            to_ms,
            content_types,
            extensions,
            limit,
            offset,
            detailed,
        } => {
            if from_ms.is_some() != to_ms.is_some() {
                ui::error("--from-ms and --to-ms must be provided together");
                return exit_codes::EXIT_ERROR;
            }

            let req = SearchQueryRequest {
                query,
                operator,
                time_preset,
                from_ms,
                to_ms,
                content_types,
                extensions,
                limit,
                offset,
            };

            let page = match search.query(req).await {
                Ok(page) => page,
                Err(err) => return render_search_error("query search index", err, json),
            };

            if json {
                let dto = SearchPageJsonDto::from(&page);
                match serde_json::to_string_pretty(&dto) {
                    Ok(value) => println!("{value}"),
                    Err(err) => {
                        ui::error(&format!("Failed to serialize search query response: {err}"));
                        return exit_codes::EXIT_ERROR;
                    }
                }
            } else {
                println!("{}", render_query_output(&page, detailed));
            }

            exit_codes::EXIT_SUCCESS
        }
        SearchCommands::Status => {
            let status = match search.status().await {
                Ok(status) => status,
                Err(err) => return render_search_error("get search status", err, json),
            };

            if json {
                let dto = SearchStatusJsonDto::from(&status);
                match serde_json::to_string_pretty(&dto) {
                    Ok(value) => println!("{value}"),
                    Err(err) => {
                        ui::error(&format!(
                            "Failed to serialize search status response: {err}"
                        ));
                        return exit_codes::EXIT_ERROR;
                    }
                }
            } else {
                println!("{}", render_status_output(&status));
            }

            exit_codes::EXIT_SUCCESS
        }
        SearchCommands::Rebuild => {
            match search.rebuild().await {
                Ok(_) => {}
                Err(err) => return render_search_error("rebuild search index", err, json),
            }

            let status = match search.status().await {
                Ok(status) => status,
                Err(err) => return render_search_error("get search status", err, json),
            };

            if json {
                let dto = SearchRebuildJsonDto {
                    accepted: true,
                    status: SearchStatusJsonDto::from(&status),
                };
                match serde_json::to_string_pretty(&dto) {
                    Ok(value) => println!("{value}"),
                    Err(err) => {
                        ui::error(&format!(
                            "Failed to serialize search rebuild response: {err}"
                        ));
                        return exit_codes::EXIT_ERROR;
                    }
                }
            } else {
                println!("Search rebuild accepted (runs in background).");
                println!("{}", render_status_output(&status));
            }

            exit_codes::EXIT_SUCCESS
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
