//! `uniclip init` — initialize a new encrypted space via daemon HTTP API.
//!
//! Prompts for (or accepts `--passphrase`) + optional `--device-name`,
//! then calls `POST /v2/setup/initialize` on the daemon. Spawns a
//! transient Oneshot daemon when none is running (skipping the setup
//! gate, since this IS the setup command).

use uc_daemon_client::DaemonClientContext;
use uc_daemon_contract::api::dto::v2::setup::InitializeSpaceRequest;

use crate::commands::app_session::{default_device_name, ensure_daemon_for_setup};
use crate::exit_codes;
use crate::ui;

pub struct InitArgs {
    pub passphrase: Option<String>,
    pub device_name: Option<String>,
}

pub async fn run(args: InitArgs, verbose: bool) -> i32 {
    ui::header("Initialize space");

    // Collect passphrase: --passphrase wins; otherwise prompt with
    // confirmation. Empty strings are always rejected.
    let passphrase_str = match args.passphrase {
        Some(ref p) if p.trim().is_empty() => {
            ui::error("--passphrase is empty");
            return exit_codes::EXIT_ERROR;
        }
        Some(p) => p,
        None => match ui::password_with_confirm("New space passphrase", "Confirm passphrase") {
            Ok(p) if p.trim().is_empty() => {
                ui::error("Passphrase cannot be empty");
                return exit_codes::EXIT_ERROR;
            }
            Ok(p) => p,
            Err(e) => {
                ui::error(&e);
                return exit_codes::EXIT_ERROR;
            }
        },
    };

    let device_name = args.device_name.or_else(default_device_name);
    let device_name = match device_name {
        Some(name) => name,
        None => {
            ui::error("Device name is required (--device-name or auto-detected hostname)");
            return exit_codes::EXIT_ERROR;
        }
    };

    // Ensure daemon is running (no setup gate — we ARE the setup command).
    let service = match ensure_daemon_for_setup(verbose).await {
        Ok(s) => s,
        Err(code) => return code,
    };
    let _lease = match service.hold_control_lease().await {
        Ok(g) => g,
        Err(err) => {
            ui::error(&format!("Failed to acquire control lease: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    let ctx = match DaemonClientContext::from_env() {
        Ok(c) => c,
        Err(err) => {
            ui::error(&format!("Failed to build daemon client context: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    let spinner = ui::spinner("Creating encrypted space...");
    let req = InitializeSpaceRequest {
        passphrase: passphrase_str.clone(),
        passphrase_confirm: passphrase_str,
        device_name: Some(device_name),
    };

    match ctx.setup_v2_client().initialize_space(&req).await {
        Ok(resp) => {
            ui::spinner_finish_success(&spinner, "Space initialized");
            ui::info("space_id", &resp.space_id);
            ui::info("device_id", &resp.self_device_id);
            ui::info("fingerprint", &resp.fingerprint);
            exit_codes::EXIT_SUCCESS
        }
        Err(err) => {
            ui::spinner_finish_error(&spinner, &crate::commands::daemon_error_message(&err));
            exit_codes::EXIT_ERROR
        }
    }
}
