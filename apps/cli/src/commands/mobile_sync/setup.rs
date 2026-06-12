//! `uniclip mobile-sync setup` — one-shot setup wizard.
//!
//! Routes through daemon HTTP endpoints (P5-2b ADR) instead of in-process
//! facade calls.

use clap::Args;
use serde::Serialize;

use uc_daemon_contract::api::dto::mobile_sync::{
    LanInterfaceViewDto, RegisterMobileDeviceRequest, UpdateMobileSyncSettingsRequest,
};

use crate::commands::mobile_sync::shared;
use crate::exit_codes;
use crate::ui;

/// SPEC §3.2 default LAN port.
const DEFAULT_LAN_PORT: u16 = 42720;

#[derive(Args, Debug)]
pub struct SetupArgs {
    /// Human-readable device label, e.g. "My iPhone 15". Required in
    /// `--non-interactive` / `--json` mode; otherwise prompted.
    #[arg(long)]
    pub label: Option<String>,

    /// LAN IPv4 to embed in the install URL (e.g. `192.168.1.5`).
    /// Required in `--non-interactive` / `--json` mode; otherwise picked
    /// interactively from the RFC1918 candidate list.
    #[arg(long, value_name = "IP")]
    pub ip: Option<String>,

    /// Custom port for the LAN listener; defaults to 42720.
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

    /// Skip all interactive prompts. `--label`, `--ip`,
    /// `--accept-network-risk` must all be given. `--username` /
    /// `--password-stdin` remain optional (default to auto-mint).
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
    advertise_ip: String,
    port: u16,
    restart_required: bool,
}

pub async fn run(args: SetupArgs, json: bool, verbose: bool) -> i32 {
    if !json {
        ui::header("Mobile-sync setup");
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
    // the daemon, so misuse fails cheaply).
    if non_interactive {
        if args.label.is_none() {
            ui::error("--label is required in --non-interactive / --json mode.");
            return exit_codes::EXIT_ERROR;
        }
        if args.ip.is_none() {
            ui::error("--ip is required in --non-interactive / --json mode.");
            return exit_codes::EXIT_ERROR;
        }
    }

    // 4. Connect to daemon, hold lease.
    // Pass json=true to suppress a duplicate header (we printed our own).
    let ctx = match shared::enter("", true, verbose).await {
        Ok(c) => c,
        Err(code) => return code,
    };

    // 5. Resolve advertise IP. --ip wins; otherwise interactive pick.
    let advertise_ip = match args.ip {
        Some(ip) => ip,
        None => {
            // non_interactive case is already rejected above.
            match resolve_advertise_interactively(&ctx).await {
                Ok(ip) => ip,
                Err(code) => return shared::finish_daemon(ctx, code).await,
            }
        }
    };

    // 6. Resolve port (no prompt; default 42720 silent).
    let port = args.port.unwrap_or(DEFAULT_LAN_PORT);

    // 7. Resolve label.
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

    // 8. Resolve username (optional). --username wins; else interactive
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

    // 9. Resolve password. cli_password (from stdin) wins; else interactive
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

    // 10. Apply settings: enable feature + LAN listener + advertise + port.
    let upd = match ctx
        .client
        .update_settings(&UpdateMobileSyncSettingsRequest {
            enabled: Some(true),
            lan_listen_enabled: Some(true),
            lan_advertise_ip: Some(Some(advertise_ip.clone())),
            lan_port: Some(Some(port)),
            // `setup` provisions the LAN ip:port form; clear any prior full
            // base-URL override so the two never coexist (the reverse-proxy
            // path is `network set --url`, see that command).
            lan_advertise_base_url: Some(None),
        })
        .await
    {
        Ok(out) => out,
        Err(err) => {
            ui::error(&err.to_string());
            return shared::finish_daemon(ctx, exit_codes::EXIT_ERROR).await;
        }
    };

    // 11. Register the device.
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

    // 12. Render.
    if json {
        let dto = SetupResult {
            device_id: reg.device_id.clone(),
            label: reg.label.clone(),
            base_url: reg.base_url.clone(),
            username: reg.username.clone(),
            password: reg.password.clone(),
            install_url: reg.install_url.clone(),
            qr_code_ascii: reg.qr_code_ascii.clone(),
            advertise_ip,
            port,
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
        ui::warn("The password above will NOT be shown again. Copy it now.");
        if upd.restart_required {
            ui::warn(shared::restart_hint());
        } else {
            ui::info("note", "Daemon restart not needed (settings unchanged).");
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

/// Interactive picker for the LAN advertise IP. Single-IP case auto-picks
/// silently. Empty list returns an error code — `setup` cannot proceed.
async fn resolve_advertise_interactively(ctx: &shared::MobileSyncDaemonCtx) -> Result<String, i32> {
    let opts = match ctx.client.list_lan_interfaces().await {
        Ok(opts) => opts,
        Err(err) => {
            ui::error(&err.to_string());
            return Err(exit_codes::EXIT_ERROR);
        }
    };
    if opts.is_empty() {
        ui::error("No RFC1918 LAN interface detected. Connect to a private network and retry.");
        return Err(exit_codes::EXIT_ERROR);
    }
    if opts.len() == 1 {
        let only = &opts[0];
        ui::info("interface", &format!("{} ({})", only.name, only.ipv4));
        return Ok(only.ipv4.clone());
    }
    pick_from_list(&opts).map_err(|code| {
        ui::warn("Aborted by user.");
        code
    })
}

fn pick_from_list(opts: &[LanInterfaceViewDto]) -> Result<String, i32> {
    ui::info("LAN interfaces", "");
    for (i, o) in opts.iter().enumerate() {
        ui::info(
            &format!("    {}", i + 1),
            &format!("{} ({})", o.name, o.ipv4),
        );
    }
    loop {
        let s = match ui::input(&format!("Pick interface [1-{}]", opts.len()), true) {
            Ok(s) => s,
            Err(_) => return Err(exit_codes::EXIT_ERROR),
        };
        let trimmed = s.trim();
        let idx_one_based: usize = if trimmed.is_empty() {
            1
        } else {
            match trimmed.parse::<usize>() {
                Ok(n) if (1..=opts.len()).contains(&n) => n,
                _ => {
                    ui::warn(&format!("Invalid choice; expected 1..{}", opts.len()));
                    continue;
                }
            }
        };
        return Ok(opts[idx_one_based - 1].ipv4.clone());
    }
}
