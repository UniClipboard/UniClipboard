use clap::Subcommand;
use serde::Serialize;

use crate::commands::app_session;
use crate::commands::daemon_error_message;
use crate::exit_codes;
use crate::ui;
use uc_daemon_client::DaemonClientContext;

#[derive(Subcommand)]
pub enum DebugCommands {
    /// Show persistent debug-mode status
    Status,
    /// Enable persistent debug-mode logging
    On,
    /// Disable persistent debug-mode logging
    Off,
    /// Export recent GUI, daemon, and CLI logs to Downloads
    #[command(name = "export-logs")]
    ExportLogs {
        /// Number of hours to include
        #[arg(long, default_value_t = 24)]
        since_hours: u32,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DebugStatusOutput {
    debug_mode: bool,
    effective_log_profile: String,
    restart_required: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DebugUpdateOutput {
    debug_mode: bool,
    restart_required: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LogExportOutput {
    path: String,
    included_files: Vec<String>,
    since: String,
}

pub async fn run(command: DebugCommands, json: bool, verbose: bool) -> i32 {
    if let Err(code) = app_session::connect_or_spawn_oneshot_daemon(verbose).await {
        return code;
    }
    let client = match DaemonClientContext::from_env() {
        Ok(ctx) => ctx.diagnostics_client(),
        Err(err) => {
            ui::error(&format!("Daemon is running but failed to connect: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    match command {
        DebugCommands::Status => match client.debug_status().await {
            Ok(status) => {
                let output = DebugStatusOutput {
                    debug_mode: status.debug_mode,
                    effective_log_profile: status.effective_log_profile,
                    restart_required: status.restart_required,
                };
                print_status(&output, json)
            }
            Err(err) => print_daemon_error("Failed to read debug status", &err),
        },
        DebugCommands::On => match client.set_debug_mode(true).await {
            Ok(result) => {
                let output = DebugUpdateOutput {
                    debug_mode: result.debug_mode,
                    restart_required: result.restart_required,
                };
                print_update(&output, json)
            }
            Err(err) => print_daemon_error("Failed to enable debug mode", &err),
        },
        DebugCommands::Off => match client.set_debug_mode(false).await {
            Ok(result) => {
                let output = DebugUpdateOutput {
                    debug_mode: result.debug_mode,
                    restart_required: result.restart_required,
                };
                print_update(&output, json)
            }
            Err(err) => print_daemon_error("Failed to disable debug mode", &err),
        },
        DebugCommands::ExportLogs { since_hours } => {
            match client.export_logs(Some(since_hours)).await {
                Ok(result) => {
                    let output = LogExportOutput {
                        path: result.path,
                        included_files: result.included_files,
                        since: result.since.to_rfc3339(),
                    };
                    print_export(&output, json)
                }
                Err(err) => print_daemon_error("Failed to export logs", &err),
            }
        }
    }
}

fn print_status(output: &DebugStatusOutput, json: bool) -> i32 {
    if json {
        return print_json(output);
    }
    ui::header("Debug");
    ui::info("debugMode", if output.debug_mode { "on" } else { "off" });
    ui::info("effectiveLogProfile", &output.effective_log_profile);
    ui::info(
        "restartRequired",
        if output.restart_required {
            "true"
        } else {
            "false"
        },
    );
    0
}

fn print_update(output: &DebugUpdateOutput, json: bool) -> i32 {
    if json {
        return print_json(output);
    }
    if output.debug_mode {
        ui::success("Debug mode enabled.");
    } else {
        ui::success("Debug mode disabled.");
    }
    if output.restart_required {
        ui::warn("Restart the daemon for the logging profile change to fully take effect.");
        ui::info("command", "uniclip stop && uniclip start");
    }
    0
}

fn print_export(output: &LogExportOutput, json: bool) -> i32 {
    if json {
        return print_json(output);
    }
    ui::success("Logs exported.");
    ui::info("path", &output.path);
    ui::info("includedFiles", &output.included_files.len().to_string());
    ui::info("since", &output.since);
    0
}

fn print_json<T: Serialize>(value: &T) -> i32 {
    match serde_json::to_string_pretty(value) {
        Ok(s) => {
            println!("{s}");
            0
        }
        Err(err) => {
            ui::error(&format!("Failed to serialize JSON: {err}"));
            exit_codes::EXIT_ERROR
        }
    }
}

fn print_daemon_error(prefix: &str, err: &anyhow::Error) -> i32 {
    ui::error(&format!("{prefix}: {}", daemon_error_message(err)));
    exit_codes::EXIT_ERROR
}
