//! 隐藏开发者工具入口。

use std::fmt;
use std::net::IpAddr;

use clap::Subcommand;
use serde::Serialize;
use uc_application::facade::{IssuePairingInvitationError, PairingInvitationAddressCandidate};

use crate::commands::app_session::{build_app_session, refuse_if_daemon_running};
use crate::commands::{dump_clipboard, invite, seed_clipboard};
use crate::exit_codes;
use crate::output;
use crate::ui;

#[derive(Subcommand)]
pub enum DevCommands {
    /// Pairing diagnostics and manual sponsor-side tools.
    Pairing {
        #[command(subcommand)]
        subcommand: DevPairingCommands,
    },
    /// Insert one text clipboard entry encrypted with the current session
    /// master key. Used by switch-space data-integrity tests as a seeding
    /// helper. Not part of the user-facing surface.
    SeedClipboard {
        /// Plaintext to seed.
        #[arg(long)]
        text: String,
    },
    /// Print the latest decrypted clipboard entries (preview field is
    /// plaintext after decryption). Pair with `dev seed-clipboard` to verify
    /// switch-space preserves data round-trip.
    DumpClipboard {
        /// Maximum number of entries to print (default 10).
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
}

#[derive(Subcommand)]
pub enum DevPairingCommands {
    /// List invitation addresses after the normal pairing filter.
    Addrs,
    /// Issue an invitation constrained to one local IP address.
    Issue {
        /// Local IP to issue against. Must appear in `dev pairing addrs`
        /// (the same product filter applies — overlay-network rules,
        /// link-local, and Clash fake-ip are dropped regardless of what
        /// you pass here).
        #[arg(long)]
        addr: IpAddr,
    },
}

#[derive(Serialize)]
struct PairingAddressView {
    ip: String,
    port: u16,
    socket: String,
}

#[derive(Serialize)]
struct PairingAddressListView {
    addresses: Vec<PairingAddressView>,
}

impl fmt::Display for PairingAddressListView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for address in &self.addresses {
            writeln!(f, "{}", address.socket)?;
        }
        Ok(())
    }
}

impl From<PairingInvitationAddressCandidate> for PairingAddressView {
    fn from(candidate: PairingInvitationAddressCandidate) -> Self {
        let ip = candidate.ip.to_string();
        let socket = format!("{}:{}", candidate.ip, candidate.port);
        Self {
            ip,
            port: candidate.port,
            socket,
        }
    }
}

pub async fn run(command: DevCommands, json: bool, verbose: bool) -> i32 {
    match command {
        DevCommands::Pairing { subcommand } => run_pairing(subcommand, json, verbose).await,
        DevCommands::SeedClipboard { text } => {
            seed_clipboard::run(seed_clipboard::SeedClipboardArgs { text }, verbose).await
        }
        DevCommands::DumpClipboard { limit } => {
            dump_clipboard::run(dump_clipboard::DumpClipboardArgs { limit }, json, verbose).await
        }
    }
}

async fn run_pairing(command: DevPairingCommands, json: bool, verbose: bool) -> i32 {
    match command {
        DevPairingCommands::Addrs => list_pairing_addrs(json, verbose).await,
        DevPairingCommands::Issue { addr } => invite::run_for_address(addr, verbose).await,
    }
}

async fn list_pairing_addrs(json: bool, verbose: bool) -> i32 {
    if !json {
        ui::header("Pairing invitation addresses");
    }

    if let Err(code) = refuse_if_daemon_running().await {
        return code;
    }

    let cli = match build_app_session(verbose).await {
        Ok(bundle) => bundle,
        Err(code) => return code,
    };

    let candidates = match cli.app_facade().list_pairing_invitation_addresses().await {
        Ok(candidates) => candidates,
        Err(IssuePairingInvitationError::NetworkNotStarted) => {
            ui::error("Network not started — run `init` first.");
            cli.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
        Err(IssuePairingInvitationError::AddressNotAvailable(addr)) => {
            ui::error(&format!("Address is not available: {addr}"));
            cli.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
        Err(IssuePairingInvitationError::ServiceUnavailable) => {
            ui::error("Pairing invitation service unavailable.");
            cli.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
        Err(IssuePairingInvitationError::Internal(msg)) => {
            ui::error(&format!("Failed to list pairing addresses: {msg}"));
            cli.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
    };

    let view = PairingAddressListView {
        addresses: candidates.into_iter().map(Into::into).collect(),
    };

    if json {
        if let Err(err) = output::print_result(&view, true) {
            ui::error(&err);
            cli.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
    } else if view.addresses.is_empty() {
        ui::warn("No pairing addresses are currently available.");
    } else {
        for address in &view.addresses {
            ui::info("candidate", &address.socket);
        }
    }

    cli.shutdown().await;
    exit_codes::EXIT_SUCCESS
}
