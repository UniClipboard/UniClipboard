//! Search command -- exposes `search query`, `search status`, and `search rebuild`
//! subcommands that forward the full daemon filter surface through `SearchQueryRequest`.

use std::future::Future;

use clap::Subcommand;
use indicatif::ProgressBar;
use tokio::time::{sleep, Duration};

use crate::exit_codes;
use uc_daemon::api::dto::search::{SearchRebuildAcceptedResponse, SearchStatusResponse};
use uc_daemon_client::{DaemonClientContext, DaemonSearchRequestError, SearchQueryRequest};

/// Subcommands for the grouped `search` CLI command.
#[derive(Subcommand, Debug)]
pub enum SearchCommands {
    /// Query the search index
    Query {
        /// Free-text query string (inline AND/OR are forwarded verbatim to daemon)
        query: String,
        /// Boolean operator: "and" or "or"
        #[arg(long)]
        operator: Option<String>,
        /// Time preset: today, yesterday, last_7d, last_30d
        #[arg(long = "time-preset")]
        time_preset: Option<String>,
        /// Start of absolute time range (milliseconds since epoch)
        #[arg(long = "from-ms")]
        from_ms: Option<i64>,
        /// End of absolute time range (milliseconds since epoch)
        #[arg(long = "to-ms")]
        to_ms: Option<i64>,
        /// Filter by content type (text, html, link, file, image, other); repeatable
        #[arg(long = "type")]
        content_types: Vec<String>,
        /// Filter by file extension (e.g. md, txt); repeatable
        #[arg(long = "ext")]
        extensions: Vec<String>,
        /// Maximum results to return
        #[arg(long, default_value_t = 50)]
        limit: u32,
        /// Result offset (for pagination)
        #[arg(long, default_value_t = 0)]
        offset: u32,
        /// Show detailed metadata for each result
        #[arg(long)]
        detailed: bool,
    },
    /// Show search index availability status
    Status,
    /// Trigger a search index rebuild (stays attached by default)
    Rebuild {
        /// Return immediately after the rebuild is accepted instead of following progress
        #[arg(long)]
        no_wait: bool,
    },
}

/// Run the grouped search command.
pub async fn run(subcommand: SearchCommands, json: bool, verbose: bool) -> i32 {
    let _ = verbose;

    let ctx = match DaemonClientContext::from_env() {
        Ok(ctx) => ctx,
        Err(error) => {
            eprintln!("Error: failed to connect to daemon: {error}");
            return exit_codes::EXIT_DAEMON_UNREACHABLE;
        }
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
            // Local flag-shape validation: --from-ms and --to-ms must come in pairs
            match (from_ms, to_ms) {
                (Some(_), None) | (None, Some(_)) => {
                    eprintln!("Error: --from-ms and --to-ms must be provided together");
                    return exit_codes::EXIT_ERROR;
                }
                _ => {}
            }

            let request = SearchQueryRequest {
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

            let response = match ctx.search_client().query(request).await {
                Ok(response) => response,
                Err(error) => {
                    eprintln!("Error: failed to query search index: {error}");
                    return exit_codes::EXIT_ERROR;
                }
            };

            if json {
                match serde_json::to_string_pretty(&response) {
                    Ok(value) => println!("{value}"),
                    Err(error) => {
                        eprintln!("Error: failed to serialize search query response: {error}");
                        return exit_codes::EXIT_ERROR;
                    }
                }
            } else {
                println!("{}", render_query_output(&response, detailed));
            }

            exit_codes::EXIT_SUCCESS
        }

        SearchCommands::Status => {
            let response = match ctx.search_client().status().await {
                Ok(response) => response,
                Err(error) => {
                    eprintln!("Error: failed to get search status: {error}");
                    return exit_codes::EXIT_ERROR;
                }
            };

            if json {
                match serde_json::to_string_pretty(&response) {
                    Ok(value) => println!("{value}"),
                    Err(error) => {
                        eprintln!("Error: failed to serialize search status response: {error}");
                        return exit_codes::EXIT_ERROR;
                    }
                }
            } else {
                println!("{}", render_status_output(&response));
            }

            exit_codes::EXIT_SUCCESS
        }

        SearchCommands::Rebuild { no_wait } => run_rebuild(json, no_wait, ctx).await,
    }
}

/// Run the `search rebuild` subcommand using the daemon client from context.
async fn run_rebuild(json: bool, no_wait: bool, ctx: DaemonClientContext) -> i32 {
    let search = ctx.search_client();
    let request_rebuild = move || {
        let s = search.clone();
        async move { s.rebuild().await }
    };
    let search = ctx.search_client();
    let fetch_status = move || {
        let s = search.clone();
        async move { s.status().await }
    };
    run_rebuild_with(request_rebuild, fetch_status, json, no_wait).await
}

/// Run the `search rebuild` subcommand with injected rebuild and status closures.
///
/// This testable variant accepts async closures for both the rebuild request and status
/// polling, allowing unit tests to inject mock sequences without real network calls.
pub async fn run_rebuild_with<RFn, RFut, SFn, SFut>(
    request_rebuild: RFn,
    fetch_status: SFn,
    json: bool,
    no_wait: bool,
) -> i32
where
    RFn: FnOnce() -> RFut,
    RFut: Future<Output = anyhow::Result<SearchRebuildAcceptedResponse>>,
    SFn: Fn() -> SFut + Clone,
    SFut: Future<Output = anyhow::Result<SearchStatusResponse>>,
{
    match request_rebuild().await {
        Ok(accepted) => {
            if no_wait {
                if json {
                    if let Ok(s) = serde_json::to_string(&accepted) {
                        println!("{s}");
                    }
                } else {
                    println!("Search rebuild accepted.");
                }
                return exit_codes::EXIT_SUCCESS;
            }
            // Accepted — enter polling loop.
            wait_for_search_ready(fetch_status, json).await
        }
        Err(error) => {
            // Check if this is a structured daemon error we can inspect.
            if let Some(search_err) = error.downcast_ref::<DaemonSearchRequestError>() {
                let code = search_err.code.as_deref().unwrap_or("");
                if code == "rebuild_already_running" {
                    if !json {
                        eprintln!("Search rebuild already running; following current status.");
                    }
                    return wait_for_search_ready(fetch_status, json).await;
                }
                if code == "session_locked" {
                    if !json {
                        eprintln!("{}", render_rebuild_locked_message());
                    } else {
                        if let Ok(s) = serde_json::to_string(&serde_json::json!({
                            "code": "session_locked",
                            "message": render_rebuild_locked_message()
                        })) {
                            eprintln!("{s}");
                        }
                    }
                    return exit_codes::EXIT_ERROR;
                }
            }
            eprintln!("Error: rebuild request failed: {error}");
            exit_codes::EXIT_ERROR
        }
    }
}

/// Poll `fetch_status` every 500ms until the daemon reports `ready` or `unavailable`.
///
/// Human mode uses a spinner and updates only when the `(state, reason)` pair changes.
/// JSON mode emits one compact JSON object per line for each status snapshot change.
pub async fn wait_for_search_ready<F, Fut>(fetch_status: F, json: bool) -> i32
where
    F: Fn() -> Fut + Clone,
    Fut: Future<Output = anyhow::Result<SearchStatusResponse>>,
{
    let spinner = if json {
        None
    } else {
        let pb = ProgressBar::new_spinner();
        pb.set_message("Search rebuild: starting...");
        Some(pb)
    };

    let mut last_state_reason: Option<(String, Option<String>)> = None;

    loop {
        match fetch_status().await {
            Ok(status) => {
                let state = status.data.state.clone();
                let reason = status.data.reason.clone();
                let pair = (state.clone(), reason.clone());

                if last_state_reason.as_ref() != Some(&pair) {
                    last_state_reason = Some(pair);

                    if json {
                        if let Ok(s) = serde_json::to_string(&status) {
                            println!("{s}");
                        }
                    } else if let Some(ref pb) = spinner {
                        let reason_str = reason.as_deref().unwrap_or("none");
                        pb.set_message(format!("Search rebuild: {state} ({reason_str})"));
                        pb.tick();
                    }
                }

                if state == "ready" || state == "unavailable" {
                    if let Some(pb) = spinner {
                        if state == "ready" {
                            pb.finish_with_message("Search rebuild complete.");
                        } else {
                            let reason_str = status.data.reason.as_deref().unwrap_or("none");
                            pb.finish_with_message(format!(
                                "Search rebuild failed or is still blocked: {reason_str}"
                            ));
                        }
                    }

                    return if state == "ready" {
                        exit_codes::EXIT_SUCCESS
                    } else {
                        exit_codes::EXIT_ERROR
                    };
                }
            }
            Err(error) => {
                if let Some(pb) = spinner {
                    pb.finish_with_message(format!("Error polling search status: {error}"));
                } else {
                    eprintln!("Error polling search status: {error}");
                }
                return exit_codes::EXIT_ERROR;
            }
        }

        sleep(Duration::from_millis(500)).await;
    }
}

/// Returns the actionable message to display when the rebuild is blocked by a locked session.
fn render_rebuild_locked_message() -> &'static str {
    "Search is unavailable while the encryption session is locked. Unlock first, or run `uniclipboard-cli space-status` to inspect encryption state."
}

/// Format a millisecond timestamp as a human-readable UTC string.
fn format_search_timestamp(ts_ms: i64) -> String {
    use chrono::{TimeZone, Utc};
    match Utc.timestamp_millis_opt(ts_ms) {
        chrono::LocalResult::Single(dt) => dt.format("%Y-%m-%d %H:%M").to_string(),
        _ => format!("<invalid timestamp: {ts_ms}>"),
    }
}

/// Render human-readable output for a search query response.
fn render_query_output(
    response: &uc_daemon::api::dto::search::SearchQueryResponse,
    detailed: bool,
) -> String {
    let total = response.total;
    let showing_from = response.data.len().min(1);
    let showing_to = response.data.len();
    let mut lines = vec![format!(
        "Search results: {total} total (showing {showing_from}-{showing_to})"
    )];

    if response.data.is_empty() {
        lines.push("No search results found.".to_string());
        lines.push("Try widening the time range.".to_string());
        lines.push("Try removing one or more filters.".to_string());
        lines.push("Try a fuller token; search is exact-token in V1.".to_string());
        return lines.join("\n");
    }

    for item in &response.data {
        let formatted_time = format_search_timestamp(item.active_time_ms);
        let preview = item.text_preview.as_deref().unwrap_or("<no preview>");
        let content_type = format!("{:?}", item.content_type).to_lowercase();
        lines.push(format!("- [{content_type}] {formatted_time}  {preview}"));

        if detailed {
            lines.push(format!("    entryId: {}", item.entry_id));
            lines.push(format!("    mimeType: {}", item.mime_type));
            let exts = if item.file_extensions.is_empty() {
                "<none>".to_string()
            } else {
                item.file_extensions.join(",")
            };
            lines.push(format!("    extensions: {exts}"));
        }
    }

    lines.join("\n")
}

/// Render human-readable output for a search status response.
fn render_status_output(response: &uc_daemon::api::dto::search::SearchStatusResponse) -> String {
    let data = &response.data;
    let reason = data.reason.as_deref().unwrap_or("none");
    let last_started = data
        .last_rebuild_started_at_ms
        .map(format_search_timestamp)
        .unwrap_or_else(|| "never".to_string());
    let last_completed = data
        .last_rebuild_completed_at_ms
        .map(format_search_timestamp)
        .unwrap_or_else(|| "never".to_string());

    vec![
        format!("Search state: {}", data.state),
        format!("Reason: {reason}"),
        format!("Last rebuild started: {last_started}"),
        format!("Last rebuild completed: {last_completed}"),
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use uc_core::search::ContentType;
    use uc_daemon::api::dto::search::{
        SearchQueryResponse, SearchResultDto, SearchStatusData, SearchStatusResponse,
    };

    #[test]
    fn search_query_help_lists_filter_flags() {
        use clap::CommandFactory;

        // Build the CLI just for the search query subcommand to check help output
        #[derive(clap::Parser)]
        struct TestCli {
            #[command(subcommand)]
            search: SearchCommands,
        }

        let mut cmd = TestCli::command();
        // Get help for the query subcommand
        let query_cmd = cmd
            .find_subcommand_mut("query")
            .expect("query subcommand not found");

        let help = query_cmd.render_help().to_string();
        assert!(
            help.contains("--time-preset"),
            "missing --time-preset: {help}"
        );
        assert!(help.contains("--from-ms"), "missing --from-ms: {help}");
        assert!(help.contains("--to-ms"), "missing --to-ms: {help}");
        assert!(help.contains("--type"), "missing --type: {help}");
        assert!(help.contains("--ext"), "missing --ext: {help}");
        assert!(help.contains("--detailed"), "missing --detailed: {help}");
    }

    #[test]
    fn render_query_output_compact_and_detailed_modes() {
        let response = SearchQueryResponse {
            data: vec![SearchResultDto {
                entry_id: "entry-abc".to_string(),
                content_type: ContentType::Text,
                active_time_ms: 1_744_300_800_000, // 2026-04-10 08:00:00 UTC
                text_preview: Some("hello world".to_string()),
                mime_type: "text/plain".to_string(),
                file_extensions: vec!["txt".to_string()],
            }],
            total: 1,
            has_more: false,
            ts: 0,
        };

        let compact = render_query_output(&response, false);
        assert!(
            compact.contains("Search results: 1 total"),
            "compact missing header: {compact}"
        );
        assert!(
            compact.contains("[text]"),
            "compact missing file type: {compact}"
        );
        assert!(
            compact.contains("hello world"),
            "compact missing preview: {compact}"
        );
        assert!(
            !compact.contains("entryId:"),
            "compact should not contain entryId: {compact}"
        );

        let detailed = render_query_output(&response, true);
        assert!(
            detailed.contains("entryId: entry-abc"),
            "detailed missing entryId: {detailed}"
        );
        assert!(
            detailed.contains("mimeType: text/plain"),
            "detailed missing mimeType: {detailed}"
        );
        assert!(
            detailed.contains("extensions: txt"),
            "detailed missing extensions: {detailed}"
        );
    }

    #[test]
    fn render_query_output_no_results_includes_guidance() {
        let response = SearchQueryResponse {
            data: vec![],
            total: 0,
            has_more: false,
            ts: 0,
        };

        let output = render_query_output(&response, false);
        assert!(
            output.contains("No search results found."),
            "missing no-results message: {output}"
        );
        assert!(
            output.contains("Try widening the time range."),
            "missing time range guidance: {output}"
        );
        assert!(
            output.contains("Try removing one or more filters."),
            "missing filter guidance: {output}"
        );
        assert!(
            output.contains("Try a fuller token; search is exact-token in V1."),
            "missing token guidance: {output}"
        );
    }

    /// RED: verify the `Rebuild` variant exists and can be destructured.
    /// This test will fail to compile until Task 1 is implemented.
    #[test]
    fn rebuild_variant_is_reachable() {
        let cmd = SearchCommands::Rebuild { no_wait: true };
        match cmd {
            SearchCommands::Rebuild { no_wait } => assert!(no_wait),
            _ => panic!("wrong variant"),
        }
    }

    fn make_status(state: &str, reason: Option<&str>) -> SearchStatusResponse {
        SearchStatusResponse {
            data: uc_daemon::api::dto::search::SearchStatusData {
                state: state.to_string(),
                reason: reason.map(|s| s.to_string()),
                last_rebuild_started_at_ms: None,
                last_rebuild_completed_at_ms: None,
            },
            ts: 0,
        }
    }

    fn make_accepted() -> anyhow::Result<uc_daemon::api::dto::search::SearchRebuildAcceptedResponse>
    {
        Ok(uc_daemon::api::dto::search::SearchRebuildAcceptedResponse {
            data: uc_daemon::api::dto::search::SearchRebuildAcceptedData { accepted: true },
            ts: 0,
        })
    }

    /// rebuild_accepted_waits_until_ready: status sequence rebuilding -> ready, returns EXIT_SUCCESS
    #[tokio::test]
    async fn rebuild_accepted_waits_until_ready() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_clone = call_count.clone();

        let fetch_status = move || {
            let n = call_count_clone.fetch_add(1, Ordering::SeqCst);
            async move {
                if n == 0 {
                    Ok(make_status("rebuilding", Some("manual_rebuild")))
                } else {
                    Ok(make_status("ready", None))
                }
            }
        };

        let result =
            run_rebuild_with(|| async move { make_accepted() }, fetch_status, true, false).await;
        assert_eq!(result, exit_codes::EXIT_SUCCESS, "expected EXIT_SUCCESS");
    }

    /// rebuild_conflict_attaches_to_existing_status: conflict -> follow rebuilding -> ready
    #[tokio::test]
    async fn rebuild_conflict_attaches_to_existing_status() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_clone = call_count.clone();

        let fetch_status = move || {
            let n = call_count_clone.fetch_add(1, Ordering::SeqCst);
            async move {
                if n == 0 {
                    Ok(make_status("rebuilding", Some("manual_rebuild")))
                } else {
                    Ok(make_status("ready", None))
                }
            }
        };

        let request_rebuild = || async move {
            Err::<uc_daemon::api::dto::search::SearchRebuildAcceptedResponse, _>(anyhow::anyhow!(
                DaemonSearchRequestError {
                    path: "/search/rebuild".to_string(),
                    status: reqwest::StatusCode::CONFLICT,
                    code: Some("rebuild_already_running".to_string()),
                    message: "rebuild already running".to_string(),
                }
            ))
        };

        let result = run_rebuild_with(request_rebuild, fetch_status, true, false).await;
        assert_eq!(
            result,
            exit_codes::EXIT_SUCCESS,
            "expected EXIT_SUCCESS after following existing rebuild"
        );
    }

    /// rebuild_locked_human_error_is_actionable: session_locked returns EXIT_ERROR with guidance
    #[tokio::test]
    async fn rebuild_locked_human_error_is_actionable() {
        let request_rebuild = || async move {
            Err::<uc_daemon::api::dto::search::SearchRebuildAcceptedResponse, _>(anyhow::anyhow!(
                DaemonSearchRequestError {
                    path: "/search/rebuild".to_string(),
                    status: reqwest::StatusCode::FORBIDDEN,
                    code: Some("session_locked".to_string()),
                    message: "encryption session is locked".to_string(),
                }
            ))
        };
        let fetch_status = || async move { Ok(make_status("unavailable", Some("session_locked"))) };

        // The locked message should be in render_rebuild_locked_message()
        let locked_msg = render_rebuild_locked_message();
        assert!(
            locked_msg.contains("encryption session is locked"),
            "locked message missing key phrase: {locked_msg}"
        );
        assert!(
            locked_msg.contains("uniclipboard-cli space-status"),
            "locked message should mention space-status: {locked_msg}"
        );

        let result = run_rebuild_with(request_rebuild, fetch_status, false, false).await;
        assert_eq!(
            result,
            exit_codes::EXIT_ERROR,
            "expected EXIT_ERROR for locked session"
        );
    }

    /// rebuild_json_wait_mode_emits_status_snapshots: JSON mode emits newline-delimited snapshots
    #[tokio::test]
    async fn rebuild_json_wait_mode_emits_status_snapshots() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_clone = call_count.clone();

        let fetch_status = move || {
            let n = call_count_clone.fetch_add(1, Ordering::SeqCst);
            async move {
                if n == 0 {
                    Ok(make_status("rebuilding", Some("manual_rebuild")))
                } else {
                    Ok(make_status("ready", None))
                }
            }
        };

        // JSON mode: run_rebuild_with with json=true and no_wait=false
        // We can't easily capture stdout in tests, but we can verify the function returns correctly
        // and that the fetch_status is called multiple times (indicating polling happened).
        let result =
            run_rebuild_with(|| async move { make_accepted() }, fetch_status, true, false).await;
        assert_eq!(
            result,
            exit_codes::EXIT_SUCCESS,
            "JSON wait mode should return EXIT_SUCCESS on ready"
        );
        // The function should have called fetch_status at least twice (rebuilding -> ready)
        assert!(
            call_count.load(Ordering::SeqCst) >= 2,
            "fetch_status should have been called at least twice"
        );
    }

    #[test]
    fn render_status_output_includes_reason_and_timestamps() {
        let response = SearchStatusResponse {
            data: SearchStatusData {
                state: "ready".to_string(),
                reason: Some("manual_rebuild".to_string()),
                last_rebuild_started_at_ms: Some(1_744_300_800_000),
                last_rebuild_completed_at_ms: Some(1_744_300_860_000),
            },
            ts: 0,
        };

        let output = render_status_output(&response);
        assert!(
            output.contains("Search state: ready"),
            "missing state: {output}"
        );
        assert!(
            output.contains("Reason: manual_rebuild"),
            "missing reason: {output}"
        );
        assert!(
            output.contains("Last rebuild started:"),
            "missing started: {output}"
        );
        assert!(
            output.contains("Last rebuild completed:"),
            "missing completed: {output}"
        );
        // Verify timestamps are formatted (not just milliseconds)
        assert!(
            !output.contains("1744300800000"),
            "timestamps should be formatted, not raw ms: {output}"
        );
    }
}
