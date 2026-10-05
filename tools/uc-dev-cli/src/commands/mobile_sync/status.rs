//! `uniclip mobile status` — combined settings + devices view.
//!
//! Routes through daemon HTTP endpoints (P5-2b ADR).

use serde::Serialize;

use uc_daemon_contract::api::dto::mobile_sync::{MobileDeviceViewDto, MobileSyncSettingsViewDto};

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
    /// 给用户展示的监听 URL — 由持久化设置拼接,daemon 永远 bind 在 `0.0.0.0`。
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

impl From<&MobileDeviceViewDto> for DeviceLineDto {
    fn from(s: &MobileDeviceViewDto) -> Self {
        Self {
            device_id: s.device_id.clone(),
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

/// User-facing "listen URL": the externally advertised address.
/// - `lan_advertise_base_url=Some(url)` → use that full address (highest priority);
/// - otherwise fall back to LAN form `lan_advertise_ip ?? "0.0.0.0"` + `lan_port ?? 42720`.
pub(crate) fn derive_listen_url(v: &MobileSyncSettingsViewDto) -> String {
    if let Some(base) = v.lan_advertise_base_url.as_deref() {
        return base.to_string();
    }
    let host = v.lan_advertise_ip.as_deref().unwrap_or("0.0.0.0");
    let port = v.lan_port.unwrap_or(42720);
    format!("http://{host}:{port}")
}

impl From<&MobileSyncSettingsViewDto> for StatusDto {
    fn from(v: &MobileSyncSettingsViewDto) -> Self {
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
                    method: m.method.clone(),
                    available: m.available,
                    disabled_reason: m.disabled_reason.clone(),
                })
                .collect(),
        }
    }
}

pub async fn run(json: bool, verbose: bool) -> i32 {
    let ctx = match shared::enter("Mobile status", json, verbose).await {
        Ok(c) => c,
        Err(code) => return code,
    };

    let view = match ctx.client.get_settings().await {
        Ok(v) => v,
        Err(err) => {
            ui::error(&err.to_string());
            return shared::finish_daemon(ctx, exit_codes::EXIT_ERROR).await;
        }
    };
    let devices = match ctx.client.list_devices().await {
        Ok(d) => d,
        Err(err) => {
            ui::error(&err.to_string());
            return shared::finish_daemon(ctx, exit_codes::EXIT_ERROR).await;
        }
    };

    if json {
        let mut dto = StatusDto::from(&view);
        dto.device_count = devices.len();
        dto.devices = devices.iter().map(DeviceLineDto::from).collect();
        shared::finish_daemon_json(ctx, &dto).await
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
                "0 — run `uniclip mobile setup` or `uniclip mobile add` to register one.",
            );
        } else {
            ui::info("devices", &format!("{} paired", devices.len()));
            for d in &devices {
                ui::info(
                    &format!("    {}", d.label),
                    &format!(
                        "id={} last_seen_ms={}",
                        d.device_id,
                        d.last_seen_at_ms
                            .map(|x| x.to_string())
                            .unwrap_or_else(|| "never".into()),
                    ),
                );
            }
        }
        shared::finish_daemon(ctx, exit_codes::EXIT_SUCCESS).await
    }
}
