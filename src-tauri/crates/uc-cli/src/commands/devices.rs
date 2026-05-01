//! Devices 命令:直连应用层列出已配对设备。

use serde::Serialize;

use uc_application::facade::roster::{MemberSummary, RosterError};
use uc_application::facade::space_setup::TryResumeSessionError;

use crate::commands::app_session::{build_app_session, refuse_if_daemon_running};
use crate::exit_codes;
use crate::ui;

pub async fn run(json: bool, verbose: bool) -> i32 {
    if let Err(code) = refuse_if_daemon_running().await {
        return code;
    }

    let cli = match build_app_session(verbose).await {
        Ok(cli) => cli,
        Err(code) => return code,
    };

    match cli.app_facade().try_resume_session().await {
        Ok(true) => {}
        Ok(false) => {
            ui::error("No space on this profile — run `init` or `join` first.");
            cli.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
        Err(TryResumeSessionError::CorruptedKeyMaterial) => {
            ui::error("Key material is corrupted — consider resetting this profile.");
            cli.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
        Err(TryResumeSessionError::KeyringMiss) => {
            ui::error("Keychain cannot silently unlock this space.");
            cli.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
        Err(TryResumeSessionError::Internal(msg)) => {
            ui::error(&format!("Resume failed: {msg}"));
            cli.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
    }

    let devices = match cli.app_facade().list_members().await {
        Ok(devices) => devices,
        Err(err) => {
            ui::error(&render_roster_error(&err));
            cli.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
    };

    if json {
        let dtos: Vec<DeviceDto> = devices.iter().map(DeviceDto::from).collect();
        match serde_json::to_string_pretty(&dtos) {
            Ok(value) => println!("{value}"),
            Err(err) => {
                ui::error(&format!("Failed to serialize paired devices: {err}"));
                cli.shutdown().await;
                return exit_codes::EXIT_ERROR;
            }
        }
    } else {
        println!("{}", render_devices_output(&devices));
    }

    cli.shutdown().await;
    exit_codes::EXIT_SUCCESS
}

fn render_roster_error(err: &RosterError) -> String {
    match err {
        RosterError::MemberRepository(message) => format!("list devices failed: {message}"),
        RosterError::LocalIdentity(message) => format!("local identity read failed: {message}"),
        RosterError::PeerAddressRepository(message) => {
            format!("peer address cleanup failed: {message}")
        }
        RosterError::TrustedPeerRepository(message) => {
            format!("trusted peer cleanup failed: {message}")
        }
        RosterError::NotFound(message) => format!("member not found: {message}"),
        RosterError::Unavailable => "member roster unavailable".to_string(),
    }
}

fn render_devices_output(devices: &[MemberSummary]) -> String {
    let mut lines = vec![format!("Paired devices: {}", devices.len())];
    lines.extend(
        devices
            .iter()
            .map(|device| format!("  {} (id: {})", device.device_name, device.device_id)),
    );
    lines.join("\n")
}

#[derive(Serialize)]
struct DeviceDto {
    device_id: String,
    device_name: String,
}

impl From<&MemberSummary> for DeviceDto {
    fn from(value: &MemberSummary) -> Self {
        Self {
            device_id: value.device_id.clone(),
            device_name: value.device_name.clone(),
        }
    }
}
