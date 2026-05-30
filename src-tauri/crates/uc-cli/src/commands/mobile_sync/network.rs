//! `uniclip mobile-sync network ...` —— LAN listener 高级配置。
//!
//! `setup` 已覆盖常用场景;本组面向需要手动调地址/端口或反代部署的进阶用户。
//!
//! 子命令:
//! * `interfaces` —— 读命令,显示 RFC1918 LAN 候选(daemon 跑时也允许)。
//! * `set (--ip <IP> | --url <URL>) [--port <P>] [--accept-network-risk]`
//!   —— 写命令。二选一:
//!   - `--ip <IP>` 写进 install URL 的 LAN IP,得到 `http://<IP>:<port>`;
//!   - `--url <URL>` 写进 install URL 的完整 base URL(如
//!     `https://clip.example.com`),用于 TLS 反向代理前置的公网部署。
//!
//!   两者互斥且必须给其一。daemon socket 始终绑 `0.0.0.0:<port>`。不带
//!   `--accept-network-risk` 时打印安全告警 + 交互确认(SPEC §3.4)。
//! * `off` —— 写命令;只关 LAN listener,总开关与已配对设备不动。要让
//!   mobile-sync 完整下线用顶层 `disable`。

use clap::Subcommand;
use serde::Serialize;

use uc_application::facade::{MobileSyncLanInterfaceOption, UpdateMobileSyncSettingsInput};

use crate::commands::mobile_sync::shared;
use crate::exit_codes;
use crate::ui;

#[derive(Subcommand)]
pub enum NetworkCommands {
    /// List eligible RFC1918 LAN IPv4 interfaces (candidates for
    /// `network set --ip`).
    Interfaces,
    /// Set the LAN listener address and turn it on (binds 0.0.0.0). Exactly
    /// one of --ip / --url decides the address printed in the install URL /
    /// QR given to the phone. Re-run to re-point the address or change the
    /// port.
    #[command(group(
        clap::ArgGroup::new("advertise_target")
            .required(true)
            .args(["ip", "url"])
    ))]
    Set {
        /// LAN IPv4 to embed in the SyncClipboard install URL
        /// (e.g. `192.168.1.5`). Pick one from `network interfaces`. Produces
        /// `http://<IP>:<port>`. Mutually exclusive with --url.
        #[arg(long, value_name = "IP")]
        ip: Option<String>,
        /// Full base URL (scheme + host + optional port) to embed in the
        /// install URL / QR, e.g. `https://clip.example.com`. Use when a
        /// TLS reverse proxy (Caddy, nginx, …) fronts the plain-HTTP LAN
        /// listener for public access. Mutually exclusive with --ip.
        #[arg(long, value_name = "URL")]
        url: Option<String>,
        /// Custom port; default 42720.
        #[arg(long, value_name = "PORT")]
        port: Option<u16>,
        /// Skip the interactive security warning. Required for
        /// non-interactive usage (CI / scripts).
        #[arg(long)]
        accept_network_risk: bool,
    },
    /// Turn off just the LAN listener (master switch and paired devices stay;
    /// use top-level `disable` to take mobile-sync fully offline).
    Off,
}

pub async fn run(command: NetworkCommands, json: bool, verbose: bool) -> i32 {
    match command {
        NetworkCommands::Interfaces => interfaces(json, verbose).await,
        NetworkCommands::Set {
            ip,
            url,
            port,
            accept_network_risk,
        } => set(ip, url, port, accept_network_risk, json, verbose).await,
        NetworkCommands::Off => off(json, verbose).await,
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

async fn interfaces(json: bool, verbose: bool) -> i32 {
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
    lan_advertise_base_url: Option<String>,
    lan_port: Option<u16>,
    restart_required: bool,
}

async fn set(
    ip: Option<String>,
    url: Option<String>,
    port: Option<u16>,
    accept_network_risk: bool,
    json: bool,
    verbose: bool,
) -> i32 {
    if !json {
        ui::header("Mobile-sync network set");
    }

    // Translate the advertise choice into the two persisted fields. The
    // `advertise_target` ArgGroup (required, mutually exclusive) already
    // guarantees exactly one of the flags at the clap level — the other two
    // arms are defensive and only reachable if that contract regresses. The
    // two fields are kept mutually exclusive in storage too: setting one
    // clears the other, so base_url-vs-ip precedence is never ambiguous.
    let (advertise_ip_patch, advertise_url_patch) = match (ip, url) {
        (Some(ip), None) => (Some(Some(ip)), Some(None)),
        (None, Some(url)) => (Some(None), Some(Some(url))),
        (None, None) => {
            ui::error("One of --ip <IP> or --url <URL> is required.");
            return exit_codes::EXIT_ERROR;
        }
        (Some(_), Some(_)) => {
            ui::error("--ip and --url are mutually exclusive.");
            return exit_codes::EXIT_ERROR;
        }
    };

    // Run interactive risk confirmation BEFORE facade wiring, so we can
    // short-circuit cheaply if the user aborts (or the JSON-mode missing-flag
    // check fires).
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
            lan_advertise_ip: advertise_ip_patch,
            lan_advertise_base_url: advertise_url_patch,
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
                    lan_advertise_base_url: out.lan_advertise_base_url.clone(),
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
                    "advertiseUrl",
                    out.lan_advertise_base_url.as_deref().unwrap_or("(unset)"),
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

async fn off(json: bool, verbose: bool) -> i32 {
    let ctx = match shared::enter_write("Mobile-sync network off", json, verbose).await {
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
