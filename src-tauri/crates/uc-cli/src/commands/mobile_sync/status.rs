//! `uniclip mobile-sync status` —— 综合视图(读命令)。
//!
//! 一条命令拼出 settings 总开关 + LAN URL / bind / port + 已配对设备
//! 数量 + install methods 状态;免去用户分别跑 `settings show` /
//! `lan list-interfaces` / `devices list` 的心智成本(P9 痛点)。
//!
//! 这是只读命令,daemon 跑时也允许。view 反映 daemon 运行时状态(LAN
//! URL 经 `endpoint_info` 在 daemon 进程内写入,本进程拿到的副本可能为
//! 空 —— SPEC §1.2.5)。

use serde::Serialize;

use uc_application::facade::{MobileDeviceSummary, MobileSyncSettingsView};

use crate::commands::mobile_sync::shared;
use crate::exit_codes;
use crate::ui;

#[derive(Serialize)]
struct StatusDto {
    enabled: bool,
    lan_listen_enabled: bool,
    lan_advertise_ip: Option<String>,
    lan_port: Option<u16>,
    current_lan_url: Option<String>,
    device_count: usize,
    devices: Vec<DeviceLineDto>,
    shortcut_install_methods: Vec<InstallMethodDto>,
}

#[derive(Serialize)]
struct DeviceLineDto {
    device_id: String,
    label: String,
    last_seen_at_ms: Option<i64>,
}

impl From<&MobileDeviceSummary> for DeviceLineDto {
    fn from(s: &MobileDeviceSummary) -> Self {
        Self {
            device_id: s.device_id.as_str().to_string(),
            label: s.label.clone(),
            last_seen_at_ms: s.last_seen_at_ms,
        }
    }
}

#[derive(Serialize)]
struct InstallMethodDto {
    method: String,
    available: bool,
    disabled_reason: Option<String>,
}

impl From<&MobileSyncSettingsView> for StatusDto {
    fn from(v: &MobileSyncSettingsView) -> Self {
        Self {
            enabled: v.enabled,
            lan_listen_enabled: v.lan_listen_enabled,
            lan_advertise_ip: v.lan_advertise_ip.clone(),
            lan_port: v.lan_port,
            current_lan_url: v.current_lan_url.clone(),
            device_count: 0,
            devices: Vec::new(),
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

pub async fn run(json: bool, verbose: bool) -> i32 {
    let ctx = match shared::enter_read("Mobile-sync status", json, verbose).await {
        Ok(c) => c,
        Err(code) => return code,
    };

    let view = match ctx.facade.get_settings().await {
        Ok(v) => v,
        Err(err) => {
            ui::error(&shared::render_get_settings_error(&err));
            return shared::finish(ctx, exit_codes::EXIT_ERROR).await;
        }
    };
    let devices = match ctx.facade.list_devices().await {
        Ok(d) => d,
        Err(err) => {
            ui::error(&shared::render_list_devices_error(&err));
            return shared::finish(ctx, exit_codes::EXIT_ERROR).await;
        }
    };

    if json {
        let mut dto = StatusDto::from(&view);
        dto.device_count = devices.len();
        dto.devices = devices.iter().map(DeviceLineDto::from).collect();
        shared::finish_json(ctx, &dto).await
    } else {
        ui::info("enabled", &view.enabled.to_string());
        ui::info("lanListenEnabled", &view.lan_listen_enabled.to_string());
        ui::info(
            "lanAdvertise",
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
        ui::bar();
        if devices.is_empty() {
            ui::info(
                "devices",
                "0 — run `uniclip mobile-sync setup` or `devices add` to register one.",
            );
        } else {
            ui::info("devices", &format!("{} paired", devices.len()));
            for d in &devices {
                ui::info(
                    &format!("    {}", d.label),
                    &format!(
                        "id={} last_seen_ms={}",
                        d.device_id.as_str(),
                        d.last_seen_at_ms
                            .map(|x| x.to_string())
                            .unwrap_or_else(|| "never".into()),
                    ),
                );
            }
        }
        shared::finish(ctx, exit_codes::EXIT_SUCCESS).await
    }
}
