//! `uniclip mobile-sync status` —— 综合视图(读命令)。
//!
//! 一条命令拼出 settings 总开关 + LAN advertise IP / port + 已配对设备
//! 数量 + install methods 状态;免去用户分别跑 `settings show` /
//! `lan list-interfaces` / `devices list` 的心智成本(P9 痛点)。
//!
//! 这是只读命令。展示给用户的 listen URL 由持久化设置直接拼接(daemon
//! 永远 bind 在 `0.0.0.0:<lan_port>`),无需运行时探测。bind 失败原因
//! 通过 `lan_listener_error` 上抛。

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
    lan_advertise_base_url: Option<String>,
    lan_port: Option<u16>,
    /// daemon 端 LAN listener 的 bind 失败原因(`Some` 时表示真的尝试过 bind 但失败)。
    lan_listener_error: Option<String>,
    /// 给用户展示的监听 URL —— 由持久化设置拼接,daemon 永远 bind 在 `0.0.0.0`。
    listen_url: String,
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

/// 用户视角的"监听 URL"(即写进 install URL / 二维码的对外地址):
/// - `lan_advertise_base_url=Some(url)` → 直接用该完整地址(优先级最高);
/// - 否则回退 LAN 形态 `lan_advertise_ip ?? "0.0.0.0"` + `lan_port ?? 42720`。
///
/// daemon 永远 bind `0.0.0.0:<lan_port>`,这里展示的是对外公布地址,不是
/// 实际 bind 地址。
pub(crate) fn derive_listen_url(v: &MobileSyncSettingsView) -> String {
    if let Some(base) = v.lan_advertise_base_url.as_deref() {
        return base.to_string();
    }
    let host = v.lan_advertise_ip.as_deref().unwrap_or("0.0.0.0");
    let port = v.lan_port.unwrap_or(42720);
    format!("http://{host}:{port}")
}

impl From<&MobileSyncSettingsView> for StatusDto {
    fn from(v: &MobileSyncSettingsView) -> Self {
        Self {
            enabled: v.enabled,
            lan_listen_enabled: v.lan_listen_enabled,
            lan_advertise_ip: v.lan_advertise_ip.clone(),
            lan_advertise_base_url: v.lan_advertise_base_url.clone(),
            lan_port: v.lan_port,
            lan_listener_error: v.lan_listener_error.clone(),
            listen_url: derive_listen_url(v),
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
