//! `uniclip upgrade` — manual verification entry for the P1 thin upgrade
//! detection module, routed through daemon HTTP endpoints.
//!
//! Subcommands:
//!
//! * `status` (default) — calls `GET /upgrade/status` on the daemon and prints
//!   the structured outcome (FreshInstall / NoChange / Upgraded / Downgraded).
//!   Bare `uniclip upgrade` runs this.
//! * `ack` — calls `POST /upgrade/ack` to advance the cursor to the
//!   daemon's current build version. Subsequent `status` runs report
//!   `NoChange` until the binary version moves.
//!
//! Both subcommands connect to an existing daemon or spawn a transient
//! Oneshot daemon, then hold a control lease for the duration of the call.
//! The version compared is the *daemon's* build version (the daemon uses
//! its own `CARGO_PKG_VERSION` when calling the facade).

use clap::Subcommand;
use serde::Serialize;
use std::fmt;

use uc_daemon_contract::api::dto::upgrade::UpgradeStatusDto;

use crate::commands::app_session::connect_with_lease;
use crate::exit_codes;
use crate::output;
use crate::ui;

#[derive(Subcommand)]
pub enum UpgradeCommands {
    /// Print the upgrade status detected by comparing the persisted
    /// version cursor against the current daemon build version.
    Status,
    /// Advance the version cursor to the current daemon build, marking
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

impl From<UpgradeStatusDto> for StatusOutput {
    fn from(value: UpgradeStatusDto) -> Self {
        match value {
            UpgradeStatusDto::FreshInstall { current } => Self::FreshInstall { current },
            UpgradeStatusDto::NoChange { current } => Self::NoChange { current },
            UpgradeStatusDto::Upgraded { from, to } => Self::Upgraded { from, to },
            UpgradeStatusDto::Downgraded { from, to } => Self::Downgraded { from, to },
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

pub async fn run(subcommand: Option<UpgradeCommands>, json: bool, verbose: bool) -> i32 {
    let (_lease, ctx) = match connect_with_lease(verbose).await {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let upgrade = ctx.upgrade_client();

    // Bare `uniclip upgrade` defaults to the read-only status check.
    match subcommand.unwrap_or(UpgradeCommands::Status) {
        UpgradeCommands::Status => match upgrade.status().await {
            Ok(dto) => {
                let payload: StatusOutput = dto.into();
                if let Err(err) = output::print_result(&payload, json) {
                    ui::error(&err);
                    return exit_codes::EXIT_ERROR;
                }
                exit_codes::EXIT_SUCCESS
            }
            Err(err) => {
                ui::error(&format!("Failed to detect upgrade status: {err}"));
                exit_codes::EXIT_ERROR
            }
        },
        UpgradeCommands::Ack => match upgrade.acknowledge().await {
            Ok(ack) => {
                let payload = AckOutput {
                    acknowledged: ack.acknowledged,
                };
                if let Err(err) = output::print_result(&payload, json) {
                    ui::error(&err);
                    return exit_codes::EXIT_ERROR;
                }
                exit_codes::EXIT_SUCCESS
            }
            Err(err) => {
                ui::error(&format!("Failed to acknowledge upgrade: {err}"));
                exit_codes::EXIT_ERROR
            }
        },
    }
}
