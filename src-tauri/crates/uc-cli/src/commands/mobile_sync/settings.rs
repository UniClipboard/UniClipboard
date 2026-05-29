//! `uniclip mobile-sync settings show` —— 显示当前移动端同步配置快照。
//!
//! 这是只读命令。展示给用户的 listen URL 由持久化设置直接拼接(daemon
//! 永远 bind 在 `0.0.0.0:<lan_port>`),无需运行时探测。bind 失败原因
//! 通过 `lan_listener_error` 上抛。

use clap::Subcommand;
use serde::Serialize;

use uc_application::facade::MobileSyncSettingsView;

use crate::commands::mobile_sync::shared;
use crate::commands::mobile_sync::status::derive_listen_url;
use crate::exit_codes;
use crate::ui;

#[derive(Subcommand)]
pub enum SettingsCommands {
    /// Print the current mobile-sync settings.
    Show,
}

pub async fn run(command: SettingsCommands, json: bool, verbose: bool) -> i32 {
    match command {
        SettingsCommands::Show => show(json, verbose).await,
    }
}

#[derive(Serialize)]
struct SettingsDto {
    enabled: bool,
    lan_listen_enabled: bool,
    lan_advertise_ip: Option<String>,
    lan_advertise_base_url: Option<String>,
    lan_port: Option<u16>,
    /// daemon 端 LAN listener 的 bind 失败原因(`Some` 时表示真的尝试过 bind 但失败)。
    lan_listener_error: Option<String>,
    /// 给用户展示的监听 URL —— 由持久化设置拼接,daemon 永远 bind 在 `0.0.0.0`。
    listen_url: String,
    shortcut_install_methods: Vec<InstallMethodDto>,
}

#[derive(Serialize)]
struct InstallMethodDto {
    method: String,
    available: bool,
    disabled_reason: Option<String>,
}

impl From<&MobileSyncSettingsView> for SettingsDto {
    fn from(v: &MobileSyncSettingsView) -> Self {
        Self {
            enabled: v.enabled,
            lan_listen_enabled: v.lan_listen_enabled,
            lan_advertise_ip: v.lan_advertise_ip.clone(),
            lan_advertise_base_url: v.lan_advertise_base_url.clone(),
            lan_port: v.lan_port,
            lan_listener_error: v.lan_listener_error.clone(),
            listen_url: derive_listen_url(v),
            shortcut_install_methods: v
                .shortcut_install_methods
                .iter()
                .map(|m| InstallMethodDto {
                    method: format!("{:?}", m.method),
                    available: m.available,
                    disabled_reason: m.disabled_reason.clone(),
                })
                .collect(),
        }
    }
}

async fn show(json: bool, verbose: bool) -> i32 {
    let ctx = match shared::enter_read("Mobile-sync settings", json, verbose).await {
        Ok(c) => c,
        Err(code) => return code,
    };

    match ctx.facade.get_settings().await {
        Ok(view) => {
            if json {
                let dto = SettingsDto::from(&view);
                shared::finish_json(ctx, &dto).await
            } else {
                ui::info("enabled", &view.enabled.to_string());
                ui::info("lanListenEnabled", &view.lan_listen_enabled.to_string());
                ui::info(
                    "lanBindIp",
                    view.lan_advertise_ip
                        .as_deref()
                        .unwrap_or("(none, fallback 0.0.0.0)"),
                );
                ui::info(
                    "lanAdvertiseUrl",
                    view.lan_advertise_base_url
                        .as_deref()
                        .unwrap_or("(none, using LAN ip:port)"),
                );
                ui::info(
                    "lanPort",
                    &view
                        .lan_port
                        .map(|p| p.to_string())
                        .unwrap_or_else(|| "(none, default 42720)".into()),
                );
                ui::info("listenUrl", &derive_listen_url(&view));
                if let Some(reason) = view.lan_listener_error.as_deref() {
                    ui::info("listenerError", reason);
                }
                shared::finish(ctx, exit_codes::EXIT_SUCCESS).await
            }
        }
        Err(err) => {
            ui::error(&shared::render_get_settings_error(&err));
            shared::finish(ctx, exit_codes::EXIT_ERROR).await
        }
    }
}
