use clap::Subcommand;
use serde::Serialize;

use crate::commands::app_session;
use crate::commands::daemon_error_message;
use crate::exit_codes;
use crate::ui;
use uc_daemon_client::DaemonClientContext;
use uc_daemon_contract::api::dto::diagnostics::{
    DiagnosticCaptureModeDto, DiagnosticSignalResultDto, DiagnosticStatusDto,
};

#[derive(Subcommand)]
pub enum DebugCommands {
    /// Show persistent debug-mode status
    Status,
    /// Enable persistent debug-mode logging
    On,
    /// Disable persistent debug-mode logging
    Off,
    /// Control the daemon-owned detailed connection capture
    Capture {
        #[command(subcommand)]
        command: CaptureCommands,
    },
    /// Export recent GUI, daemon, and CLI logs to Downloads
    #[command(name = "export-logs")]
    ExportLogs {
        /// Number of hours to include
        #[arg(long, default_value_t = 24)]
        since_hours: u32,
    },
}

#[derive(Subcommand)]
pub enum CaptureCommands {
    /// Show the active capture and remaining time
    Status,
    /// Start or reuse one bounded detailed capture
    Start {
        /// Capture duration in minutes
        #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u16).range(1..=15))]
        minutes: u16,
    },
    /// Stop the matching active capture
    Stop {
        /// Capture identifier returned by start or status
        capture_id: String,
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
    engine_flush: String,
    unreadable_files: Vec<String>,
    truncated_files: Vec<String>,
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
        DebugCommands::Capture { command } => match command {
            CaptureCommands::Status => match client.capture_status().await {
                Ok(status) => print_capture_status(&status, json),
                Err(err) => print_daemon_error("Failed to read capture status", &err),
            },
            CaptureCommands::Start { minutes } => {
                match client.start_capture(minutes.saturating_mul(60)).await {
                    Ok(status) => print_capture_status(&status, json),
                    Err(err) => print_daemon_error("Failed to start detailed capture", &err),
                }
            }
            CaptureCommands::Stop { capture_id } => match client.stop_capture(capture_id).await {
                Ok(result) => {
                    if json {
                        print_json(&result)
                    } else {
                        ui::success(&format!("Capture stop result: {result:?}"));
                        0
                    }
                }
                Err(err) => print_daemon_error("Failed to stop detailed capture", &err),
            },
        },
        DebugCommands::ExportLogs { since_hours } => {
            match client.export_logs(Some(since_hours)).await {
                Ok(result) => {
                    if json {
                        return print_json(&result);
                    }
                    let output = LogExportOutput {
                        path: result.path,
                        included_files: result.included_files,
                        since: result.since.to_rfc3339(),
                        engine_flush: signal_name(result.engine_preparation.flush).to_string(),
                        unreadable_files: result.collection.unreadable_files,
                        truncated_files: result.collection.truncated_files,
                    };
                    print_export(&output)
                }
                Err(err) => print_daemon_error("Failed to export logs", &err),
            }
        }
    }
}

fn print_capture_status(status: &DiagnosticStatusDto, json: bool) -> i32 {
    if json {
        return print_json(status);
    }
    ui::header("Detailed connection capture");
    ui::info(
        "mode",
        match status.capture.mode {
            DiagnosticCaptureModeDto::Standard => "standard",
            DiagnosticCaptureModeDto::Detailed => "detailed",
        },
    );
    if let Some(capture_id) = &status.capture.capture_id {
        ui::info("captureId", capture_id);
        ui::info("remainingMs", &status.capture.remaining_ms.to_string());
    }
    ui::info("runId", &status.run_id);
    ui::info(
        "localFile",
        &format!("{:?}", status.local_file).to_lowercase(),
    );
    0
}

fn signal_name(result: DiagnosticSignalResultDto) -> &'static str {
    match result {
        DiagnosticSignalResultDto::Completed => "completed",
        DiagnosticSignalResultDto::Failed => "failed",
        DiagnosticSignalResultDto::TimedOut => "timedOut",
        DiagnosticSignalResultDto::AlreadyShutdown => "alreadyShutdown",
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

fn print_export(output: &LogExportOutput) -> i32 {
    ui::success("Logs exported.");
    ui::info("path", &output.path);
    ui::info("includedFiles", &output.included_files.len().to_string());
    ui::info("since", &output.since);
    ui::info("engineFlush", &output.engine_flush);
    ui::info(
        "unreadableFiles",
        &output.unreadable_files.len().to_string(),
    );
    ui::info("truncatedFiles", &output.truncated_files.len().to_string());
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
