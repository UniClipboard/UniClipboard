use serde::Serialize;

use crate::commands::app_session::connect_facade_with_lease;
use crate::commands::daemon_error_message;
use crate::exit_codes;
use crate::output;
use crate::ui;

#[derive(Serialize)]
struct ResetSpaceResult {
    ok: bool,
    status: &'static str,
}

pub async fn run(json: bool, verbose: bool) -> i32 {
    if !json {
        ui::header("Reset space");
        ui::warn("All existing device relationships will be permanently discarded.");
        ui::info(
            "kept",
            "Clipboard history, completed files, settings, device identity, and unlock access.",
        );
    }

    let (_lease, facade) = match connect_facade_with_lease(verbose).await {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    if let Err(error) = facade.reset_space().await {
        ui::error(&format!(
            "Failed to reset space: {}",
            daemon_error_message(&error)
        ));
        return exit_codes::EXIT_ERROR;
    }

    if json {
        output::emit_json(
            &ResetSpaceResult {
                ok: true,
                status: "reset",
            },
            "space reset result",
        )
    } else {
        ui::success("Space reset. Pair every device again before resuming sync.");
        exit_codes::EXIT_SUCCESS
    }
}
