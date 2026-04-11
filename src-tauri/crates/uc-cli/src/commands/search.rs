//! Search command -- exposes `search query` and `search status` subcommands
//! that forward the full daemon filter surface through `SearchQueryRequest`.

use clap::Subcommand;

use crate::exit_codes;
use uc_daemon_client::{DaemonClientContext, SearchQueryRequest};

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
        file_types: Vec<String>,
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
            file_types,
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
                file_types,
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
    }
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
        let file_type = format!("{:?}", item.file_type).to_lowercase();
        lines.push(format!("- [{file_type}] {formatted_time}  {preview}"));

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
    use uc_core::search::FileType;
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
                file_type: FileType::Text,
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
