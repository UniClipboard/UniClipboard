//! `uniclip mobile-sync devices ...` —— 已配对 iPhone 设备管理。
//!
//! 子命令:
//! * `list` —— 读命令(daemon 跑时也允许);打印设备摘要,默认人类可读、`--json` 输出稳定结构。
//! * `revoke <device-id>` —— 写命令;立即让该设备的 Basic Auth 凭据失效。

use clap::Subcommand;
use serde::Serialize;

use uc_application::facade::{MobileDeviceSummary, RevokeMobileDeviceInput};
use uc_core::mobile_sync::MobileDeviceId;

use crate::commands::mobile_sync::shared;
use crate::exit_codes;
use crate::ui;

#[derive(Subcommand)]
pub enum DevicesCommands {
    /// List all paired mobile devices.
    List,
    /// Revoke a paired device by its device id.
    Revoke {
        /// Device id printed by `devices list` (e.g. `did_<32hex>`).
        device_id: String,
    },
}

pub async fn run(command: DevicesCommands, json: bool, verbose: bool) -> i32 {
    match command {
        DevicesCommands::List => list(json, verbose).await,
        DevicesCommands::Revoke { device_id } => revoke(device_id, json, verbose).await,
    }
}

#[derive(Serialize)]
struct DeviceDto {
    device_id: String,
    label: String,
    client_type: String,
    created_at_ms: i64,
    last_seen_at_ms: Option<i64>,
    last_seen_ip: Option<String>,
    reported_name: Option<String>,
    reported_os: Option<String>,
}

impl From<&MobileDeviceSummary> for DeviceDto {
    fn from(s: &MobileDeviceSummary) -> Self {
        Self {
            device_id: s.device_id.as_str().to_string(),
            label: s.label.clone(),
            client_type: s.client_type.as_wire_str().to_string(),
            created_at_ms: s.created_at_ms,
            last_seen_at_ms: s.last_seen_at_ms,
            last_seen_ip: s.last_seen_ip.clone(),
            reported_name: s.reported_name.clone(),
            reported_os: s.reported_os.clone(),
        }
    }
}

async fn list(json: bool, verbose: bool) -> i32 {
    let ctx = match shared::enter_read("Paired iPhone devices", json, verbose).await {
        Ok(c) => c,
        Err(code) => return code,
    };
    match ctx.facade.list_devices().await {
        Ok(devs) => {
            if json {
                let dtos: Vec<DeviceDto> = devs.iter().map(DeviceDto::from).collect();
                shared::finish_json(ctx, &dtos).await
            } else {
                if devs.is_empty() {
                    ui::info(
                        "count",
                        "0 — run `mobile-sync shortcut add` to register one.",
                    );
                } else {
                    for d in &devs {
                        ui::info(
                            &d.label,
                            &format!(
                                "id={} client={} last_seen_ms={}",
                                d.device_id.as_str(),
                                d.client_type.as_wire_str(),
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
        Err(err) => {
            ui::error(&shared::render_list_devices_error(&err));
            shared::finish(ctx, exit_codes::EXIT_ERROR).await
        }
    }
}

#[derive(Serialize)]
struct RevokeResult {
    device_id: String,
    revoked: bool,
}

async fn revoke(device_id: String, json: bool, verbose: bool) -> i32 {
    let ctx = match shared::enter_write("Revoke iPhone device", json, verbose).await {
        Ok(c) => c,
        Err(code) => return code,
    };

    let result = ctx
        .facade
        .revoke_device(RevokeMobileDeviceInput {
            device_id: MobileDeviceId::new(device_id.clone()),
        })
        .await;

    match result {
        Ok(()) => {
            if json {
                let dto = RevokeResult {
                    device_id: device_id.clone(),
                    revoked: true,
                };
                shared::finish_json(ctx, &dto).await
            } else {
                ui::success(&format!("Revoked device {device_id}."));
                ui::info("note", "Next request from that device returns 401.");
                shared::finish(ctx, exit_codes::EXIT_SUCCESS).await
            }
        }
        Err(err) => {
            ui::error(&shared::render_revoke_error(&err));
            shared::finish(ctx, exit_codes::EXIT_ERROR).await
        }
    }
}
