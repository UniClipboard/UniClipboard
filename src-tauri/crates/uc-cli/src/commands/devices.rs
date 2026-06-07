//! `uniclip devices` — list paired devices (ADR-008 P5-2a).
//!
//! Routes through a running or freshly-spawned daemon. Holds a control-WS
//! lease to keep a transient Oneshot daemon alive for the query sequence.

use serde::Serialize;
use uc_daemon_client::DaemonClientContext;

use crate::commands::app_session::connect_or_spawn_oneshot_daemon;
use crate::exit_codes;
use crate::ui;

pub async fn run(json: bool, verbose: bool) -> i32 {
    let service = match connect_or_spawn_oneshot_daemon(verbose).await {
        Ok(s) => s,
        Err(code) => return code,
    };

    let _lease = match service.hold_control_lease().await {
        Ok(guard) => guard,
        Err(err) => {
            ui::error(&format!("Failed to hold daemon session lease: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    let ctx = match DaemonClientContext::from_env() {
        Ok(ctx) => ctx,
        Err(err) => {
            ui::error(&format!("Failed to connect to daemon: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };
    let query = ctx.query_client();

    // Fetch remote devices from the daemon.
    let remote_devices = match query.get_paired_devices().await {
        Ok(devices) => devices,
        Err(err) => {
            ui::error(&format!("Failed to list paired devices: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    // Fetch local device info (non-fatal if unavailable).
    let local_device = match query.get_local_device_info().await {
        Ok(info) => Some(info),
        Err(err) => {
            tracing::debug!(error = %err, "could not fetch local device info");
            None
        }
    };

    // Build combined list: local device first, then remote devices.
    let mut devices: Vec<DeviceDto> = Vec::with_capacity(1 + remote_devices.len());

    if let Some(local) = local_device {
        devices.push(DeviceDto {
            device_id: local.peer_id,
            device_name: local.device_name,
        });
    }

    for member in &remote_devices {
        devices.push(DeviceDto {
            device_id: member.peer_id.clone(),
            device_name: member.device_name.clone(),
        });
    }

    if json {
        match serde_json::to_string_pretty(&devices) {
            Ok(value) => println!("{value}"),
            Err(err) => {
                ui::error(&format!("Failed to serialize paired devices: {err}"));
                return exit_codes::EXIT_ERROR;
            }
        }
    } else {
        render_devices_output(&devices);
    }

    exit_codes::EXIT_SUCCESS
}

fn render_devices_output(devices: &[DeviceDto]) {
    ui::bar();
    if devices.is_empty() {
        ui::info("devices", "(none)");
    } else {
        ui::info("total", &format!("{}", devices.len()));
        for device in devices {
            let line = format!("{} (id: {})", device.device_name, device.device_id);
            ui::info("·", &line);
        }
    }
    ui::bar();
}

#[derive(Serialize)]
struct DeviceDto {
    device_id: String,
    device_name: String,
}
