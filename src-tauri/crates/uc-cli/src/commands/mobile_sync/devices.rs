//! Paired iPhone device registration / revocation — `mobile-sync add` /
//! `mobile-sync revoke` implementations.
//!
//! Routes through daemon HTTP endpoints (P5-2b ADR).

use clap::Args;
use serde::Serialize;

use uc_daemon_contract::api::dto::mobile_sync::RegisterMobileDeviceRequest;

use crate::commands::mobile_sync::shared;
use crate::exit_codes;
use crate::ui;

/// Shared arguments for registering a new iPhone. Reused by the top-level
/// `mobile-sync add` and the (hidden, back-compat) `devices add`.
#[derive(Args)]
pub struct AddArgs {
    /// Human-readable label, e.g. "My iPhone 15".
    #[arg(long)]
    pub label: String,
    /// Custom username (6-32 chars, `[A-Za-z0-9_]`, letter-leading).
    /// Leave unset to mint a random `mobile_<8hex>`.
    #[arg(long, value_name = "U")]
    pub username: Option<String>,
    /// Read the password from one line of stdin. Mutually exclusive
    /// with auto-mint; both unset → auto-mint.
    #[arg(long)]
    pub password_stdin: bool,
}

// ── add (top-level `mobile-sync add`) ───────────────────────────────────

#[derive(Serialize)]
struct AddDeviceDto {
    device_id: String,
    label: String,
    base_url: String,
    username: String,
    password: String,
    install_url: String,
    qr_code_ascii: String,
}

pub(crate) async fn add(args: AddArgs, json: bool, verbose: bool) -> i32 {
    let AddArgs {
        label,
        username,
        password_stdin,
    } = args;

    if !json {
        ui::header("Add iPhone (SyncClipboard EX)");
    }

    // Read password from stdin BEFORE wiring the session.
    let cli_password = if password_stdin {
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

    // Connect to daemon with json=true to suppress a duplicate header.
    let ctx = match shared::enter("", true, verbose).await {
        Ok(c) => c,
        Err(code) => return code,
    };

    let result = ctx
        .client
        .register_device(&RegisterMobileDeviceRequest {
            label: label.clone(),
            username,
            password: cli_password,
        })
        .await;

    match result {
        Ok(out) => {
            if json {
                let dto = AddDeviceDto {
                    device_id: out.device_id.clone(),
                    label: out.label.clone(),
                    base_url: out.base_url.clone(),
                    username: out.username.clone(),
                    password: out.password.clone(),
                    install_url: out.install_url.clone(),
                    qr_code_ascii: out.qr_code_ascii.clone(),
                };
                shared::finish_daemon_json(ctx, &dto).await
            } else {
                ui::success(&format!("Registered device: {}", out.label));
                ui::info("deviceId", &out.device_id);
                ui::info("baseUrl", &out.base_url);
                ui::info("username", &out.username);
                ui::info("password (one-time)", &out.password);
                ui::info("installUrl", &out.install_url);
                ui::bar();
                println!();
                println!("{}", out.qr_code_ascii);
                println!();
                ui::info(
                    "next",
                    "Scan the QR with iPhone Camera, install the SyncClipboard \
                     shortcut, then edit url / username / password fields.",
                );
                ui::warn("The password above will NOT be shown again. Copy it now.");
                ui::warn(
                    "Run `uniclip start` so the LAN listener accepts requests \
                     from this device.",
                );
                shared::finish_daemon(ctx, exit_codes::EXIT_SUCCESS).await
            }
        }
        Err(err) => {
            ui::error(&err.to_string());
            shared::finish_daemon(ctx, exit_codes::EXIT_ERROR).await
        }
    }
}

// ── revoke ──────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct RevokeResult {
    device_id: String,
    revoked: bool,
}

pub(crate) async fn revoke(device_id: Option<String>, json: bool, verbose: bool) -> i32 {
    let ctx = match shared::enter("Revoke iPhone device", json, verbose).await {
        Ok(c) => c,
        Err(code) => return code,
    };

    // Resolve target device id: explicit > interactive picker. JSON mode
    // requires explicit id (no interactive prompt safe in scripts).
    let target = match device_id {
        Some(id) => id,
        None => {
            if json {
                ui::error("`<device-id>` is required in --json mode.");
                return shared::finish_daemon(ctx, exit_codes::EXIT_ERROR).await;
            }
            match resolve_device_interactively(&ctx).await {
                Ok(id) => id,
                Err(code) => return shared::finish_daemon(ctx, code).await,
            }
        }
    };

    let result = ctx.client.revoke_device(&target).await;

    match result {
        Ok(_) => {
            if json {
                let dto = RevokeResult {
                    device_id: target.clone(),
                    revoked: true,
                };
                shared::finish_daemon_json(ctx, &dto).await
            } else {
                ui::success(&format!("Revoked device {target}."));
                ui::info("note", "Next request from that device returns 401.");
                shared::finish_daemon(ctx, exit_codes::EXIT_SUCCESS).await
            }
        }
        Err(err) => {
            ui::error(&err.to_string());
            shared::finish_daemon(ctx, exit_codes::EXIT_ERROR).await
        }
    }
}

/// Interactive picker for `revoke` without an explicit id. Lists paired
/// devices on stderr, asks the user to pick by number, returns the
/// selected device id. Empty list / aborted prompt → exit-code error.
async fn resolve_device_interactively(ctx: &shared::MobileSyncDaemonCtx) -> Result<String, i32> {
    let devs = match ctx.client.list_devices().await {
        Ok(d) => d,
        Err(err) => {
            ui::error(&err.to_string());
            return Err(exit_codes::EXIT_ERROR);
        }
    };
    if devs.is_empty() {
        ui::warn("No paired devices to revoke.");
        return Err(exit_codes::EXIT_ERROR);
    }
    ui::info("Paired devices", "");
    for (i, d) in devs.iter().enumerate() {
        ui::info(
            &format!("    {}", i + 1),
            &format!("{} (id={})", d.label, d.device_id),
        );
    }
    loop {
        let s = match ui::input(&format!("Pick device [1-{}]", devs.len()), true) {
            Ok(s) => s,
            Err(_) => return Err(exit_codes::EXIT_ERROR),
        };
        let trimmed = s.trim();
        if trimmed.is_empty() {
            ui::warn("Aborted by user.");
            return Err(exit_codes::EXIT_ERROR);
        }
        match trimmed.parse::<usize>() {
            Ok(n) if (1..=devs.len()).contains(&n) => {
                return Ok(devs[n - 1].device_id.clone());
            }
            _ => {
                ui::warn(&format!("Invalid choice; expected 1..{}", devs.len()));
            }
        }
    }
}
