//! Search 命令:直连应用层查询、查看状态、同步重建本地索引。

use clap::Subcommand;
use serde::Serialize;

use uc_application::facade::{
    SearchFacadeError, SearchPageView, SearchQueryInput, SearchResultView, SearchStatusView,
};

use crate::exit_codes;

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
    /// Rebuild the search index synchronously in this CLI process
    Rebuild,
}

pub async fn run(subcommand: SearchCommands, json: bool, verbose: bool) -> i32 {
    let app_facade = match build_search_facade(verbose).await {
        Ok(facade) => facade,
        Err(code) => return code,
    };

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
                eprintln!("Error: --from-ms and --to-ms must be provided together");
                return exit_codes::EXIT_ERROR;
            }

            let input = SearchQueryInput {
                query,
                operator,
                time_preset,
                from_ms,
                to_ms,
                content_types: join_repeated(content_types),
                extensions: join_repeated(extensions),
                limit,
                offset,
            };

            let page = match app_facade.search_query(input).await {
                Ok(page) => page,
                Err(err) => return render_search_error("query search index", err, json),
            };

            if json {
                let dto = SearchPageDto::from(&page);
                match serde_json::to_string_pretty(&dto) {
                    Ok(value) => println!("{value}"),
                    Err(err) => {
                        eprintln!("Error: failed to serialize search query response: {err}");
                        return exit_codes::EXIT_ERROR;
                    }
                }
            } else {
                println!("{}", render_query_output(&page, detailed));
            }

            exit_codes::EXIT_SUCCESS
        }
        SearchCommands::Status => {
            let status = match app_facade.search_status().await {
                Ok(status) => status,
                Err(err) => return render_search_error("get search status", err, json),
            };

            if json {
                let dto = SearchStatusDto::from(&status);
                match serde_json::to_string_pretty(&dto) {
                    Ok(value) => println!("{value}"),
                    Err(err) => {
                        eprintln!("Error: failed to serialize search status response: {err}");
                        return exit_codes::EXIT_ERROR;
                    }
                }
            } else {
                println!("{}", render_status_output(&status));
            }

            exit_codes::EXIT_SUCCESS
        }
        SearchCommands::Rebuild => {
            match app_facade.rebuild_search_now().await {
                Ok(_) => {}
                Err(err) => return render_search_error("rebuild search index", err, json),
            }

            let status = match app_facade.search_status().await {
                Ok(status) => status,
                Err(err) => return render_search_error("get search status", err, json),
            };

            if json {
                let dto = SearchRebuildDto {
                    accepted: true,
                    status: SearchStatusDto::from(&status),
                };
                match serde_json::to_string_pretty(&dto) {
                    Ok(value) => println!("{value}"),
                    Err(err) => {
                        eprintln!("Error: failed to serialize search rebuild response: {err}");
                        return exit_codes::EXIT_ERROR;
                    }
                }
            } else {
                println!("Search rebuild complete.");
                println!("{}", render_status_output(&status));
            }

            exit_codes::EXIT_SUCCESS
        }
    }
}

async fn build_search_facade(
    verbose: bool,
) -> Result<std::sync::Arc<uc_application::facade::AppFacade>, i32> {
    let profile = if verbose {
        Some(uc_observability::LogProfile::Dev)
    } else {
        Some(uc_observability::LogProfile::Cli)
    };
    uc_bootstrap::build_cli_app_facade(profile)
        .await
        .map_err(|err| {
            eprintln!("Error: failed to build CLI runtime: {err}");
            exit_codes::EXIT_ERROR
        })
}

fn join_repeated(values: Vec<String>) -> Option<String> {
    if values.is_empty() {
        None
    } else {
        Some(values.join(","))
    }
}

fn render_search_error(action: &str, err: SearchFacadeError, json: bool) -> i32 {
    if json {
        let dto = ErrorDto {
            code: search_error_code(&err),
            message: err.to_string(),
        };
        if let Ok(value) = serde_json::to_string(&dto) {
            eprintln!("{value}");
        }
    } else if matches!(err, SearchFacadeError::SessionLocked) {
        eprintln!("{}", render_rebuild_locked_message());
    } else {
        eprintln!("Error: failed to {action}: {err}");
    }
    exit_codes::EXIT_ERROR
}

fn search_error_code(err: &SearchFacadeError) -> &'static str {
    match err {
        SearchFacadeError::InvalidQuery(_) => "invalid_query",
        SearchFacadeError::BadRequest(_) => "bad_request",
        SearchFacadeError::SessionLocked => "session_locked",
        SearchFacadeError::IndexNotReady => "index_not_ready",
        SearchFacadeError::IndexUnavailable => "index_unavailable",
        SearchFacadeError::ServiceUnavailable(_) => "service_unavailable",
        SearchFacadeError::RebuildAlreadyRunning => "rebuild_already_running",
        SearchFacadeError::Internal(_) => "internal",
    }
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

fn render_query_output(response: &SearchPageView, detailed: bool) -> String {
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

fn render_status_output(response: &SearchStatusView) -> String {
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

#[derive(Serialize)]
struct SearchPageDto<'a> {
    total: u32,
    has_more: bool,
    data: Vec<SearchResultDto<'a>>,
}

impl<'a> From<&'a SearchPageView> for SearchPageDto<'a> {
    fn from(value: &'a SearchPageView) -> Self {
        Self {
            total: value.total,
            has_more: value.has_more,
            data: value.items.iter().map(SearchResultDto::from).collect(),
        }
    }
}

#[derive(Serialize)]
struct SearchResultDto<'a> {
    entry_id: &'a str,
    content_type: &'a str,
    active_time_ms: i64,
    text_preview: Option<&'a str>,
    mime_type: &'a str,
    file_extensions: &'a [String],
}

impl<'a> From<&'a SearchResultView> for SearchResultDto<'a> {
    fn from(value: &'a SearchResultView) -> Self {
        Self {
            entry_id: &value.entry_id,
            content_type: &value.content_type,
            active_time_ms: value.active_time_ms,
            text_preview: value.text_preview.as_deref(),
            mime_type: &value.mime_type,
            file_extensions: &value.file_extensions,
        }
    }
}

#[derive(Serialize)]
struct SearchStatusDto<'a> {
    state: &'a str,
    reason: Option<&'a str>,
    last_rebuild_started_at_ms: Option<i64>,
    last_rebuild_completed_at_ms: Option<i64>,
}

impl<'a> From<&'a SearchStatusView> for SearchStatusDto<'a> {
    fn from(value: &'a SearchStatusView) -> Self {
        Self {
            state: &value.state,
            reason: value.reason.as_deref(),
            last_rebuild_started_at_ms: value.last_rebuild_started_at_ms,
            last_rebuild_completed_at_ms: value.last_rebuild_completed_at_ms,
        }
    }
}

#[derive(Serialize)]
struct SearchRebuildDto<'a> {
    accepted: bool,
    status: SearchStatusDto<'a>,
}

#[derive(Serialize)]
struct ErrorDto {
    code: &'static str,
    message: String,
}
