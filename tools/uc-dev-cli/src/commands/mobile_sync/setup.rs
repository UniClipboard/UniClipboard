//! `uniclip mobile setup` — one-shot setup wizard.
//!
//! Routes through daemon HTTP endpoints (P5-2b ADR) instead of in-process
//! facade calls.

use clap::Args;
use serde::Serialize;

use uc_daemon_contract::api::dto::mobile_sync::{
    RegisterMobileDeviceRequest, UpdateMobileSyncSettingsRequest,
};

use crate::commands::mobile_sync::shared;
use crate::exit_codes;
use crate::ui;

#[derive(Args, Debug)]
pub struct SetupArgs {
    /// Human-readable device label, e.g. "My iPhone 15". Required in
    /// `--non-interactive` / `--json` mode; otherwise prompted.
    #[arg(long)]
    pub label: Option<String>,

    /// Optional advanced override: pin one LAN IPv4 (e.g. `192.168.1.5`) to
    /// the front of the QR's address list. Leave unset and the QR carries
    /// every detected LAN interface automatically — the scanning client
    /// probes each in turn, so there is normally nothing to pick.
    #[arg(long, value_name = "IP")]
    pub ip: Option<String>,

    /// Optional advanced override: custom LAN listener port. Leave unset to
    /// keep the existing / default port (42720).
    #[arg(long, value_name = "PORT")]
    pub port: Option<u16>,

    /// Custom username (6-32 chars, `[A-Za-z0-9_]`, must start with a
    /// letter). Leave unset to mint a random `mobile_<8hex>` username.
    #[arg(long, value_name = "U")]
    pub username: Option<String>,

    /// Read the password from one line of stdin. Useful for piping from a
    /// password manager / CI; stays out of shell history. Mutually
    /// exclusive with the interactive prompt.
    #[arg(long)]
    pub password_stdin: bool,

    /// Accept the network exposure warning non-interactively.
    /// **Required** in `--non-interactive` / `--json` mode (no interactive
    /// confirmation possible).
    #[arg(long)]
    pub accept_network_risk: bool,

    /// Skip all interactive prompts. `--label` and `--accept-network-risk`
    /// must be given. `--ip` / `--port` / `--username` / `--password-stdin`
    /// remain optional (IP/port keep defaults, credentials auto-mint).
    #[arg(long)]
    pub non_interactive: bool,
}

#[derive(Serialize)]
struct SetupResult {
    device_id: String,
    label: String,
    base_url: String,
    username: String,
    password: String,
    install_url: String,
    qr_code_ascii: String,
    /// Pinned advertise IP, if `--ip` was given; `null` when the QR relies on
    /// auto-detected interfaces (the common case).
    advertise_ip: Option<String>,
    /// Resulting LAN listener port (resolved by the daemon; `null` ⇒ default).
    port: Option<u16>,
    restart_required: bool,
}

pub async fn run(args: SetupArgs, json: bool, verbose: bool) -> i32 {
    if !json {
        ui::header("Mobile setup");
    }

    // JSON mode is implicitly non-interactive — no terminal prompt is safe
    // in a script context. Treat the two flags as one effective bit.
    let non_interactive = args.non_interactive || json;

    // 1. Network risk warning + acceptance.
    if !args.accept_network_risk {
        if non_interactive {
            ui::error(
                "--accept-network-risk is required in --non-interactive / --json mode \
                 (no interactive prompt).",
            );
            return exit_codes::EXIT_ERROR;
        }
        print_network_risk_banner();
        let accepted = ui::confirm("Accept network exposure and continue?", false).unwrap_or(false);
        if !accepted {
            ui::warn("Aborted by user.");
            return exit_codes::EXIT_ERROR;
        }
    }

    // 2. Read password from stdin BEFORE wiring the session — keeps any
    // pipe / heredoc input handling out of the facade lifetime.
    let cli_password = if args.password_stdin {
        match shared::read_password_stdin() {
            Ok(p) => Some(p),
            Err(e) => {
                ui::error(&format!("Failed to read password from stdin: {e}"));
                return exit_codes::EXIT_ERROR;
            }
        }
    } else {
        None
    };

    // 3. Validate non-interactive required flags up front (before we touch
    // the daemon, so misuse fails cheaply). Only --label is required now:
    // the QR auto-carries every detected interface, so the user no longer
    // has to pick an address (mirrors the GUI's one-click enable).
    if non_interactive && args.label.is_none() {
        ui::error("--label is required in --non-interactive / --json mode.");
        return exit_codes::EXIT_ERROR;
    }

    // 4. Connect to daemon, hold lease.
    // Pass json=true to suppress a duplicate header (we printed our own).
    let ctx = match shared::enter("", true, verbose).await {
        Ok(c) => c,
        Err(code) => return code,
    };

    // 5. Resolve label.
    let label = match args.label {
        Some(l) => l,
        None => {
            // Interactive — non_interactive case rejected above.
            match ui::input("Device label (e.g. \"My iPhone 15\")", false) {
                Ok(s) => s.trim().to_string(),
                Err(e) => {
                    ui::error(&format!("Failed to read label: {e}"));
                    return shared::finish_daemon(ctx, exit_codes::EXIT_ERROR).await;
                }
            }
        }
    };

    // 6. Resolve username (optional). --username wins; else interactive
    // [Enter for auto]; non_interactive without flag → auto.
    let custom_username = match args.username {
        Some(u) => Some(u),
        None if non_interactive => None,
        None => match ui::input(
            "Username (6-32 chars, [A-Za-z0-9_], letter-leading) [Enter for auto]",
            true,
        ) {
            Ok(s) if s.trim().is_empty() => None,
            Ok(s) => Some(s.trim().to_string()),
            Err(e) => {
                ui::error(&format!("Failed to read username: {e}"));
                return shared::finish_daemon(ctx, exit_codes::EXIT_ERROR).await;
            }
        },
    };

    // 7. Resolve password. cli_password (from stdin) wins; else interactive
    // hidden prompt [Enter for auto]; non_interactive without flag → auto.
    let custom_password = match cli_password {
        Some(p) => Some(p),
        None if non_interactive => None,
        None => match ui::password("Password (min 8 chars) [Enter for auto]:") {
            Ok(s) if s.is_empty() => None,
            Ok(s) => Some(s),
            Err(e) => {
                ui::error(&format!("Failed to read password: {e}"));
                return shared::finish_daemon(ctx, exit_codes::EXIT_ERROR).await;
            }
        },
    };

    // 8. Apply settings: enable feature + LAN listener. Mirrors the GUI's
    // one-click enable — we flip the switches and let the QR carry every
    // detected interface. `--ip` / `--port` are optional advanced pins:
    // patched only when given (`None` ⇒ leave unchanged), so re-running
    // `setup` never resets a previously configured port. An unset port falls
    // back to the SPEC default (42720) at advertise time.
    // `lan_advertise_base_url` is left untouched — multi-address QRs happily
    // carry a reverse-proxy entry alongside the LAN IPs, so there is no longer
    // anything to clear.
    let upd = match ctx
        .client
        .update_settings(&UpdateMobileSyncSettingsRequest {
            enabled: Some(true),
            lan_listen_enabled: Some(true),
            lan_advertise_ip: args.ip.clone().map(Some),
            lan_port: args.port.map(Some),
            lan_advertise_base_url: None,
        })
        .await
    {
        Ok(out) => out,
        Err(err) => {
            ui::error(&err.to_string());
            return shared::finish_daemon(ctx, exit_codes::EXIT_ERROR).await;
        }
    };

    // 8a. Abort before minting credentials if the listener failed to bind —
    // otherwise we'd hand the user a QR for a socket that never came up
    // (port in use / permission / unassignable IP). Mirrors the GUI guard.
    if let Some(reason) = upd.lan_listener_bind_error.as_deref() {
        ui::error(&format!(
            "LAN listener failed to bind: {reason}. Free the port (or pick \
             another with `--port`) and re-run setup."
        ));
        return shared::finish_daemon(ctx, exit_codes::EXIT_ERROR).await;
    }

    // 9. Register the device.
    let reg = match ctx
        .client
        .register_device(&RegisterMobileDeviceRequest {
            label: label.clone(),
            username: custom_username,
            password: custom_password,
        })
        .await
    {
        Ok(out) => out,
        Err(err) => {
            ui::error(&err.to_string());
            return shared::finish_daemon(ctx, exit_codes::EXIT_ERROR).await;
        }
    };

    // 10. Render.
    if json {
        let dto = SetupResult {
            device_id: reg.device_id.clone(),
            label: reg.label.clone(),
            base_url: reg.base_url.clone(),
            username: reg.username.clone(),
            password: reg.password.clone(),
            install_url: reg.install_url.clone(),
            qr_code_ascii: reg.qr_code_ascii.clone(),
            advertise_ip: upd.lan_advertise_ip.clone(),
            port: upd.lan_port,
            restart_required: upd.restart_required,
        };
        shared::finish_daemon_json(ctx, &dto).await
    } else {
        ui::success(&format!("Registered device: {}", reg.label));
        ui::info("deviceId", &reg.device_id);
        ui::info("baseUrl", &reg.base_url);
        ui::info("username", &reg.username);
        ui::info("password (one-time)", &reg.password);
        ui::info("installUrl", &reg.install_url);
        ui::bar();
        println!();
        println!("{}", reg.qr_code_ascii);
        println!();
        ui::info(
            "next",
            "Scan the QR with iPhone Camera, install the SyncClipboard \
             shortcut, then edit url / username / password fields.",
        );
        ui::info(
            "note",
            "The QR carries every detected network address — the phone tries \
             each until one connects, so no address picking is needed.",
        );
        ui::warn("The password above will NOT be shown again. Copy it now.");
        if upd.restart_required {
            ui::warn(shared::restart_hint());
        }
        shared::finish_daemon(ctx, exit_codes::EXIT_SUCCESS).await
    }
}

// ── Interactive helpers ──────────────────────────────────────────────────

/// Print the SPEC §3.4 LAN exposure warning. Mirrors the banner used by
/// `network set` so users see consistent wording across both entry points.
fn print_network_risk_banner() {
    ui::warn("Enabling LAN listener exposes clipboard data over your local network.");
    ui::info("•", "Body is unencrypted in v1 (HTTPS comes in v2).");
    ui::info(
        "•",
        "Only enable on trusted networks (home / private office).",
    );
    ui::info("•", "Strongly discouraged on public WiFi.");
    ui::info("•", "Anyone on the same LAN can sniff your data.");
}
