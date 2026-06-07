//! `uniclip mobile-sync network ...` — LAN listener advanced configuration.
//!
//! Routes through daemon HTTP endpoints (P5-2b ADR).

use clap::Subcommand;
use serde::Serialize;

use uc_daemon_contract::api::dto::mobile_sync::{
    LanInterfaceViewDto, UpdateMobileSyncSettingsRequest,
};

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
        /// TLS reverse proxy (Caddy, nginx, ...) fronts the plain-HTTP LAN
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

impl From<&LanInterfaceViewDto> for InterfaceDto {
    fn from(v: &LanInterfaceViewDto) -> Self {
        Self {
            name: v.name.clone(),
            ipv4: v.ipv4.clone(),
        }
    }
}

async fn interfaces(json: bool, verbose: bool) -> i32 {
    let ctx = match shared::enter("LAN interfaces", json, verbose).await {
        Ok(c) => c,
        Err(code) => return code,
    };
    match ctx.client.list_lan_interfaces().await {
        Ok(opts) => {
            if json {
                let dtos: Vec<InterfaceDto> = opts.iter().map(InterfaceDto::from).collect();
                shared::finish_daemon_json(ctx, &dtos).await
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
                shared::finish_daemon(ctx, exit_codes::EXIT_SUCCESS).await
            }
        }
        Err(err) => {
            ui::error(&err.to_string());
            shared::finish_daemon(ctx, exit_codes::EXIT_ERROR).await
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

    // Translate the advertise choice into the two persisted fields.
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

    // Run interactive risk confirmation BEFORE daemon wiring.
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

    // Header already printed; pass json=true to enter to suppress a
    // second header from the helper.
    let ctx = match shared::enter("", true, verbose).await {
        Ok(c) => c,
        Err(code) => return code,
    };

    let result = ctx
        .client
        .update_settings(&UpdateMobileSyncSettingsRequest {
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
                shared::finish_daemon_json(ctx, &dto).await
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
                shared::finish_daemon(ctx, exit_codes::EXIT_SUCCESS).await
            }
        }
        Err(err) => {
            ui::error(&err.to_string());
            shared::finish_daemon(ctx, exit_codes::EXIT_ERROR).await
        }
    }
}

#[derive(Serialize)]
struct DisableResult {
    lan_listen_enabled: bool,
    restart_required: bool,
}

async fn off(json: bool, verbose: bool) -> i32 {
    let ctx = match shared::enter("Mobile-sync network off", json, verbose).await {
        Ok(c) => c,
        Err(code) => return code,
    };
    let result = ctx
        .client
        .update_settings(&UpdateMobileSyncSettingsRequest {
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
                shared::finish_daemon_json(ctx, &dto).await
            } else {
                ui::success("LAN listener disabled in settings.");
                if out.restart_required {
                    ui::warn(shared::restart_hint());
                }
                shared::finish_daemon(ctx, exit_codes::EXIT_SUCCESS).await
            }
        }
        Err(err) => {
            ui::error(&err.to_string());
            shared::finish_daemon(ctx, exit_codes::EXIT_ERROR).await
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
