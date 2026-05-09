//! `uniclip upgrade` —— manual verification entry for the P1 thin upgrade
//! detection module.
//!
//! Subcommands:
//!
//! * `status` —— calls [`UpgradeFacade::detect_on_startup`] with the CLI's
//!   own build version and prints the structured outcome (FreshInstall /
//!   NoChange / Upgraded / Downgraded). Read-only; safe to run alongside
//!   the daemon.
//! * `ack` —— calls [`UpgradeFacade::acknowledge`] to advance the cursor
//!   to the current build version. Subsequent `status` runs report
//!   `NoChange` until the binary version moves.
//!
//! Both subcommands use `build_cli_app_facade` (no iroh / network), so
//! they work even when no space has been initialised on this profile.
//!
//! The version string fed to the facade is `env!("CARGO_PKG_VERSION")` of
//! `uc-cli` itself, which matches the workspace version inherited by
//! `uc-desktop` (the daemon's source of truth). Profile selection happens
//! through the global `--profile` flag in `main.rs`, identical to other
//! standalone CLI commands.

use clap::Subcommand;
use serde::Serialize;
use std::fmt;

use uc_application::facade::{AcknowledgeUpgradeError, DetectUpgradeError, UpgradeStatus};

use crate::exit_codes;
use crate::output;
use crate::ui;

/// CLI build version. Compared against the persisted cursor by the
/// `status` subcommand and written back by `ack`.
const CLI_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Subcommand)]
pub enum UpgradeCommands {
    /// Print the upgrade status detected by comparing the persisted
    /// version cursor against the current CLI build version.
    Status,
    /// Advance the version cursor to the current CLI build, marking
    /// the upgrade as acknowledged. Idempotent.
    Ack,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum StatusOutput {
    FreshInstall { current: String },
    NoChange { current: String },
    Upgraded { from: Option<String>, to: String },
    Downgraded { from: String, to: String },
}

impl From<UpgradeStatus> for StatusOutput {
    fn from(value: UpgradeStatus) -> Self {
        match value {
            UpgradeStatus::FreshInstall => Self::FreshInstall {
                current: CLI_VERSION.to_string(),
            },
            UpgradeStatus::NoChange => Self::NoChange {
                current: CLI_VERSION.to_string(),
            },
            UpgradeStatus::Upgraded { from, to } => Self::Upgraded {
                from: from.map(|v| v.to_string()),
                to: to.to_string(),
            },
            UpgradeStatus::Downgraded { from, to } => Self::Downgraded {
                from: from.to_string(),
                to: to.to_string(),
            },
        }
    }
}

impl fmt::Display for StatusOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FreshInstall { current } => {
                writeln!(f, "Status: fresh install")?;
                write!(f, "Current version: {current}")
            }
            Self::NoChange { current } => {
                writeln!(f, "Status: no change")?;
                write!(f, "Current version: {current}")
            }
            Self::Upgraded { from, to } => {
                writeln!(f, "Status: upgraded")?;
                writeln!(f, "From: {}", from.as_deref().unwrap_or("<unknown>"))?;
                write!(f, "To:   {to}")
            }
            Self::Downgraded { from, to } => {
                writeln!(f, "Status: downgraded")?;
                writeln!(f, "From: {from}")?;
                write!(f, "To:   {to}")
            }
        }
    }
}

#[derive(Serialize)]
struct AckOutput {
    acknowledged: String,
}

impl fmt::Display for AckOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Cursor advanced to {}", self.acknowledged)
    }
}

pub async fn run(subcommand: UpgradeCommands, json: bool, verbose: bool) -> i32 {
    let log_profile = if verbose {
        Some(uc_observability::LogProfile::Dev)
    } else {
        Some(uc_observability::LogProfile::Cli)
    };

    let app_facade = match uc_bootstrap::build_cli_app_facade(log_profile).await {
        Ok(facade) => facade,
        Err(err) => {
            ui::error(&format!("failed to build CLI runtime: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    match subcommand {
        UpgradeCommands::Status => match app_facade.upgrade.detect_on_startup(CLI_VERSION).await {
            Ok(status) => {
                let payload: StatusOutput = status.into();
                if let Err(err) = output::print_result(&payload, json) {
                    ui::error(&err);
                    return exit_codes::EXIT_ERROR;
                }
                exit_codes::EXIT_SUCCESS
            }
            Err(err) => {
                ui::error(&format_detect_error(&err));
                exit_codes::EXIT_ERROR
            }
        },
        UpgradeCommands::Ack => match app_facade.upgrade.acknowledge(CLI_VERSION).await {
            Ok(()) => {
                let payload = AckOutput {
                    acknowledged: CLI_VERSION.to_string(),
                };
                if let Err(err) = output::print_result(&payload, json) {
                    ui::error(&err);
                    return exit_codes::EXIT_ERROR;
                }
                exit_codes::EXIT_SUCCESS
            }
            Err(err) => {
                ui::error(&format_ack_error(&err));
                exit_codes::EXIT_ERROR
            }
        },
    }
}

fn format_detect_error(err: &DetectUpgradeError) -> String {
    match err {
        DetectUpgradeError::CurrentVersionMalformed(s) => {
            format!("current build version is malformed: {s}")
        }
        DetectUpgradeError::ReadCursor(s) => format!("read upgrade cursor failed: {s}"),
        DetectUpgradeError::ReadSetupStatus(s) => format!("read setup status failed: {s}"),
    }
}

fn format_ack_error(err: &AcknowledgeUpgradeError) -> String {
    match err {
        AcknowledgeUpgradeError::CurrentVersionMalformed(s) => {
            format!("current build version is malformed: {s}")
        }
        AcknowledgeUpgradeError::WriteCursor(s) => format!("write upgrade cursor failed: {s}"),
    }
}
