//! `uniclip mobile-sync lan ...` —— LAN listener 管理。
//!
//! 子命令:
//! * `list-interfaces` —— 读命令,显示 RFC1918 LAN 候选(daemon 跑时也允许)。
//! * `enable --advertise <IP> [--port <P>] [--accept-network-risk]` —— 写命令。
//!   `--advertise` 决定写进 SyncClipboard install URL 给 iPhone 的 IP
//!   (daemon socket 始终绑 0.0.0.0)。不带 `--accept-network-risk` 时打印
//!   安全告警 + 交互确认(SPEC §3.4)。
//! * `disable` —— 写命令。

use clap::Subcommand;
use serde::Serialize;

use uc_application::facade::{MobileSyncLanInterfaceOption, UpdateMobileSyncSettingsInput};

use crate::commands::mobile_sync::shared;
use crate::exit_codes;
use crate::ui;

#[derive(Subcommand)]
pub enum LanCommands {
    /// List eligible RFC1918 LAN IPv4 interfaces.
    ListInterfaces,
    /// Enable the LAN listener (binds 0.0.0.0; --advertise decides the
    /// IP printed in the install URL given to the iPhone).
    Enable {
        /// LAN IPv4 to embed in the SyncClipboard install URL
        /// (e.g. `192.168.1.5`). Pick one from `list-interfaces`.
        #[arg(long, value_name = "IP")]
        advertise: String,
        /// Custom port; default 42720.
        #[arg(long, value_name = "PORT")]
        port: Option<u16>,
        /// Skip the interactive security warning. Required for
        /// non-interactive usage (CI / scripts).
        #[arg(long)]
        accept_network_risk: bool,
    },
    /// Disable the LAN listener (already paired devices stay registered).
    Disable,
}

pub async fn run(command: LanCommands, json: bool, verbose: bool) -> i32 {
    match command {
        LanCommands::ListInterfaces => list_interfaces(json, verbose).await,
        LanCommands::Enable {
            advertise,
            port,
            accept_network_risk,
        } => enable(advertise, port, accept_network_risk, json, verbose).await,
        LanCommands::Disable => disable(json, verbose).await,
    }
}

#[derive(Serialize)]
struct InterfaceDto {
    name: String,
    ipv4: String,
}

impl From<&MobileSyncLanInterfaceOption> for InterfaceDto {
    fn from(v: &MobileSyncLanInterfaceOption) -> Self {
        Self {
            name: v.name.clone(),
            ipv4: v.ipv4.clone(),
        }
    }
}

async fn list_interfaces(json: bool, verbose: bool) -> i32 {
    let ctx = match shared::enter_read("LAN interfaces", json, verbose).await {
        Ok(c) => c,
        Err(code) => return code,
    };
    match ctx.facade.list_lan_interfaces().await {
        Ok(opts) => {
            if json {
                let dtos: Vec<InterfaceDto> = opts.iter().map(InterfaceDto::from).collect();
                shared::finish_json(ctx, &dtos).await
            } else {
                if opts.is_empty() {
                    ui::warn(
                        "No RFC1918 LAN interface detected. Connect to a private network and retry.",
                    );
                } else {
                    for o in &opts {
                        ui::info(&o.name, &o.ipv4);
                    }
                }
                shared::finish(ctx, exit_codes::EXIT_SUCCESS).await
            }
        }
        Err(err) => {
            ui::error(&shared::render_list_lan_interfaces_error(&err));
            shared::finish(ctx, exit_codes::EXIT_ERROR).await
        }
    }
}

#[derive(Serialize)]
struct EnableResult {
    enabled: bool,
    lan_listen_enabled: bool,
    lan_advertise_ip: Option<String>,
    lan_port: Option<u16>,
    restart_required: bool,
}

async fn enable(
    advertise: String,
    port: Option<u16>,
    accept_network_risk: bool,
    json: bool,
    verbose: bool,
) -> i32 {
    // Print header + run interactive risk confirmation BEFORE facade wiring,
    // so we can short-circuit cheaply if the user aborts (or the JSON-mode
    // missing-flag check fires).
    if !json {
        ui::header("Mobile-sync LAN enable");
    }
    if !accept_network_risk {
        if json {
            ui::error("--accept-network-risk is required in JSON mode (no interactive prompt).");
            return exit_codes::EXIT_ERROR;
        }
        print_network_risk_banner();
        let accepted = ui::confirm("Accept network exposure and continue?", false).unwrap_or(false);
        if !accepted {
            ui::warn("Aborted by user.");
            return exit_codes::EXIT_ERROR;
        }
    }

    // Header already printed; pass json=true to enter_write to suppress a
    // second header from the helper.
    let ctx = match shared::enter_write("", true, verbose).await {
        Ok(c) => c,
        Err(code) => return code,
    };

    let result = ctx
        .facade
        .update_settings(UpdateMobileSyncSettingsInput {
            // 同时把总开关也置 true:用户开 LAN 时大概率也想要 enable=true,
            // 否则 daemon 启动时仍因 enabled=false 跳过 listener。
            enabled: Some(true),
            lan_listen_enabled: Some(true),
            lan_advertise_ip: Some(Some(advertise)),
            lan_port: Some(port),
        })
        .await;

    match result {
        Ok(out) => {
            if json {
                let dto = EnableResult {
                    enabled: out.enabled,
                    lan_listen_enabled: out.lan_listen_enabled,
                    lan_advertise_ip: out.lan_advertise_ip.clone(),
                    lan_port: out.lan_port,
                    restart_required: out.restart_required,
                };
                shared::finish_json(ctx, &dto).await
            } else {
                ui::success("LAN listener enabled in settings.");
                ui::info(
                    "advertise",
                    out.lan_advertise_ip.as_deref().unwrap_or("(unset)"),
                );
                ui::info(
                    "port",
                    &out.lan_port
                        .map(|p| p.to_string())
                        .unwrap_or_else(|| "default (42720)".into()),
                );
                if out.restart_required {
                    ui::warn(shared::restart_hint());
                }
                shared::finish(ctx, exit_codes::EXIT_SUCCESS).await
            }
        }
        Err(err) => {
            ui::error(&shared::render_update_settings_error(&err));
            shared::finish(ctx, exit_codes::EXIT_ERROR).await
        }
    }
}

#[derive(Serialize)]
struct DisableResult {
    lan_listen_enabled: bool,
    restart_required: bool,
}

async fn disable(json: bool, verbose: bool) -> i32 {
    let ctx = match shared::enter_write("Mobile-sync LAN disable", json, verbose).await {
        Ok(c) => c,
        Err(code) => return code,
    };
    let result = ctx
        .facade
        .update_settings(UpdateMobileSyncSettingsInput {
            lan_listen_enabled: Some(false),
            ..Default::default()
        })
        .await;
    match result {
        Ok(out) => {
            if json {
                let dto = DisableResult {
                    lan_listen_enabled: out.lan_listen_enabled,
                    restart_required: out.restart_required,
                };
                shared::finish_json(ctx, &dto).await
            } else {
                ui::success("LAN listener disabled in settings.");
                if out.restart_required {
                    ui::warn(shared::restart_hint());
                }
                shared::finish(ctx, exit_codes::EXIT_SUCCESS).await
            }
        }
        Err(err) => {
            ui::error(&shared::render_update_settings_error(&err));
            shared::finish(ctx, exit_codes::EXIT_ERROR).await
        }
    }
}

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
