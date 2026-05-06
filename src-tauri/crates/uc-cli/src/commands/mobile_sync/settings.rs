//! `uniclip mobile-sync settings show` —— 显示当前移动端同步配置快照。
//!
//! 这是只读命令,daemon 跑时也允许(跳过 `refuse_if_daemon_running`)。但
//! `build_app_session` 仍会先尝试拉起独立 wiring;同 profile daemon 已起
//! 时 sqlite 仍可只读打开,view 反映 daemon 运行时状态(LAN url 经
//! `endpoint_info` 在 daemon 进程内写入,本进程拿到的副本可能为空,这是
//! 项目惯例的边界 —— SPEC §1.2.5)。

use clap::Subcommand;
use serde::Serialize;

use uc_application::facade::MobileSyncSettingsView;

use crate::commands::app_session::build_app_session;
use crate::commands::mobile_sync::shared;
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
    lan_port: Option<u16>,
    current_lan_url: Option<String>,
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
            lan_port: v.lan_port,
            current_lan_url: v.current_lan_url.clone(),
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
    if !json {
        ui::header("Mobile-sync settings");
    }
    let cli = match build_app_session(verbose).await {
        Ok(cli) => cli,
        Err(code) => return code,
    };
    let Some(facade) = cli.app_facade().mobile_sync.clone() else {
        ui::error("Mobile-sync facade is not wired in this build.");
        cli.shutdown().await;
        return exit_codes::EXIT_ERROR;
    };

    let exit = match facade.get_settings().await {
        Ok(view) => {
            if json {
                let dto = SettingsDto::from(&view);
                match serde_json::to_string_pretty(&dto) {
                    Ok(s) => println!("{s}"),
                    Err(err) => {
                        ui::error(&format!("Failed to serialize: {err}"));
                        cli.shutdown().await;
                        return exit_codes::EXIT_ERROR;
                    }
                }
            } else {
                ui::info("enabled", &view.enabled.to_string());
                ui::info("lanListenEnabled", &view.lan_listen_enabled.to_string());
                ui::info(
                    "lanBindIp",
                    view.lan_advertise_ip
                        .as_deref()
                        .unwrap_or("(none, fallback 127.0.0.1)"),
                );
                ui::info(
                    "lanPort",
                    &view
                        .lan_port
                        .map(|p| p.to_string())
                        .unwrap_or_else(|| "(none, default 42720)".into()),
                );
                ui::info(
                    "currentLanUrl",
                    view.current_lan_url
                        .as_deref()
                        .unwrap_or("(listener not running)"),
                );
            }
            exit_codes::EXIT_SUCCESS
        }
        Err(err) => {
            ui::error(&shared::render_get_settings_error(&err));
            exit_codes::EXIT_ERROR
        }
    };

    cli.shutdown().await;
    exit
}
