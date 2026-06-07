//! `uniclip switch-space` — switch an already-set-up device to another
//! sponsor's space via daemon HTTP API.
//!
//! The device must already be set up (`init` or `join` completed). This
//! command collects the new sponsor's invitation code and passphrase,
//! then calls `POST /v2/setup/switch-space` on the daemon. The daemon
//! drives the 4-phase re-encryption migration internally; the CLI shows
//! a spinner and waits (with Ctrl+C handling).

use tokio::select;
use tokio::signal;

use uc_daemon_client::DaemonClientContext;
use uc_daemon_contract::api::dto::v2::setup::SwitchSpaceRequest;

use crate::commands::app_session::connect_or_spawn_oneshot_daemon;
use crate::exit_codes;
use crate::ui;

const EXIT_SIGINT: i32 = 130;

pub struct SwitchSpaceArgs {
    pub code: Option<String>,
    pub new_passphrase: Option<String>,
}

pub async fn run(args: SwitchSpaceArgs, verbose: bool) -> i32 {
    ui::header("Switch to another space");

    // ------------------------------------------------------------------
    // 1. Collect invitation code (before spawning daemon)
    // ------------------------------------------------------------------
    let code_str = match args.code {
        Some(c) if !c.trim().is_empty() => c.trim().to_string(),
        Some(_) => {
            ui::error("--code is empty");
            return exit_codes::EXIT_ERROR;
        }
        None => match ui::password("Invitation code from new sponsor") {
            Ok(c) if !c.trim().is_empty() => c.trim().to_string(),
            Ok(_) => {
                ui::error("Invitation code cannot be empty");
                return exit_codes::EXIT_ERROR;
            }
            Err(e) => {
                ui::error(&e);
                return exit_codes::EXIT_ERROR;
            }
        },
    };

    // ------------------------------------------------------------------
    // 2. Collect new passphrase (before spawning daemon)
    // ------------------------------------------------------------------
    let new_passphrase = match args.new_passphrase {
        Some(p) if !p.trim().is_empty() => p,
        Some(_) => {
            ui::error("--new-passphrase is empty");
            return exit_codes::EXIT_ERROR;
        }
        None => match ui::password("New space passphrase") {
            Ok(p) if !p.trim().is_empty() => p,
            Ok(_) => {
                ui::error("Passphrase cannot be empty");
                return exit_codes::EXIT_ERROR;
            }
            Err(e) => {
                ui::error(&e);
                return exit_codes::EXIT_ERROR;
            }
        },
    };

    // ------------------------------------------------------------------
    // 3. Connect to daemon (device IS set up → normal connect path)
    // ------------------------------------------------------------------
    let service = match connect_or_spawn_oneshot_daemon(verbose).await {
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

    // ------------------------------------------------------------------
    // 4. POST switch-space with Ctrl-C guard
    // ------------------------------------------------------------------
    let spinner = ui::spinner(
        "Migrating local clipboard history to the new space (4 phases \u{2014} this may take a while)...",
    );

    let req = SwitchSpaceRequest {
        code: code_str,
        new_passphrase,
    };

    let setup_client = ctx.setup_v2_client();
    let switch_fut = setup_client.switch_space(&req);
    tokio::pin!(switch_fut);

    select! {
        result = &mut switch_fut => match result {
            Ok(resp) => {
                ui::spinner_finish_success(&spinner, "Switched space");
                ui::info("space_id", &resp.space_id);
                ui::info("self_device_id", &resp.self_device_id);
                ui::info("self_fingerprint", &resp.self_identity_fingerprint);
                ui::info("sponsor_device_id", &resp.sponsor_device_id);
                ui::info("sponsor_fingerprint", &resp.sponsor_identity_fingerprint);
                ui::info("migrated_records", &resp.migrated_records.to_string());
                exit_codes::EXIT_SUCCESS
            }
            Err(err) => {
                ui::spinner_finish_error(&spinner, &format!("Switch-space failed: {err}"));
                exit_codes::EXIT_ERROR
            }
        },
        _ = signal::ctrl_c() => {
            ui::spinner_finish_error(&spinner, "Interrupted by user");
            ui::info(
                "note",
                "Migration may be partially complete. Restart `uniclip` to auto-resume.",
            );
            EXIT_SIGINT
        }
    }
}
