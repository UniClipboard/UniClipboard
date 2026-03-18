//! Devices command -- lists paired devices via direct bootstrap (no daemon required).

use serde::Serialize;
use std::fmt;

use crate::exit_codes;
use crate::output;

#[derive(Serialize)]
struct DeviceInfo {
    peer_id: String,
    name: String,
    pairing_state: String,
    identity_fingerprint: String,
}

#[derive(Serialize)]
struct DeviceListOutput {
    devices: Vec<DeviceInfo>,
    count: usize,
}

impl fmt::Display for DeviceListOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.devices.is_empty() {
            return write!(f, "No paired devices found.");
        }
        writeln!(f, "Paired devices: {}", self.count)?;
        for d in &self.devices {
            writeln!(f, "  {} (id: {})", d.name, d.peer_id)?;
        }
        Ok(())
    }
}

/// Run the devices command.
///
/// Uses `build_cli_context()` + `build_non_gui_runtime()` to query the device
/// list directly from the database without requiring the daemon to be running.
pub async fn run(json: bool, verbose: bool) -> i32 {
    let profile = if verbose {
        Some(uc_observability::LogProfile::Dev)
    } else {
        Some(uc_observability::LogProfile::Cli)
    };

    let ctx = match uc_bootstrap::build_cli_context_with_profile(profile) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: failed to initialize CLI context: {}", e);
            return exit_codes::EXIT_ERROR;
        }
    };

    let storage_paths = match uc_bootstrap::get_storage_paths(&ctx.config) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error: failed to resolve storage paths: {}", e);
            return exit_codes::EXIT_ERROR;
        }
    };

    let runtime = match uc_bootstrap::build_non_gui_runtime(ctx.deps, storage_paths) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error: failed to build runtime: {}", e);
            return exit_codes::EXIT_ERROR;
        }
    };

    let usecases = uc_app::usecases::CoreUseCases::new(&runtime);
    let devices = match usecases.list_paired_devices().execute().await {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Error: failed to list paired devices: {}", e);
            return exit_codes::EXIT_ERROR;
        }
    };

    let device_infos: Vec<DeviceInfo> = devices
        .into_iter()
        .map(|d| DeviceInfo {
            peer_id: d.peer_id.to_string(),
            name: d.device_name,
            pairing_state: format!("{:?}", d.pairing_state),
            identity_fingerprint: d.identity_fingerprint,
        })
        .collect();

    let result = DeviceListOutput {
        count: device_infos.len(),
        devices: device_infos,
    };

    if let Err(e) = output::print_result(&result, json) {
        eprintln!("Error: {}", e);
        return exit_codes::EXIT_ERROR;
    }

    exit_codes::EXIT_SUCCESS
}
