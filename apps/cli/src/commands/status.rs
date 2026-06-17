//! Status 命令:通过 daemon 显示应用状态。

use serde::Serialize;
use std::fmt;

use crate::commands::app_session::connect_with_lease;
use crate::exit_codes;
use crate::output;
use crate::ui;

#[derive(Serialize)]
struct StatusOutput {
    setup_completed: bool,
    encryption_ready: bool,
    search_state: String,
    search_reason: Option<String>,
}

impl fmt::Display for StatusOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let setup = if self.setup_completed { "yes" } else { "no" };
        let encryption = if self.encryption_ready { "yes" } else { "no" };
        let reason = self.search_reason.as_deref().unwrap_or("none");

        writeln!(f, "Setup completed: {setup}")?;
        writeln!(f, "Encryption ready: {encryption}")?;
        writeln!(f, "Search state: {}", self.search_state)?;
        write!(f, "Search reason: {reason}")?;
        Ok(())
    }
}

pub async fn run(json: bool, verbose: bool) -> i32 {
    let (_lease, ctx) = match connect_with_lease(verbose).await {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    // Assumption: a healthy daemon implies setup_complete=true and
    // encryption unlocked (startup_recovery auto-unlocks). If the daemon
    // lifecycle ever allows starting without setup or with locked
    // encryption, this inference must be replaced with a dedicated
    // endpoint query.
    let setup_completed = true;
    let encryption_ready = true;

    let search = ctx.search_client();
    let (search_state, search_reason) = match search.status().await {
        Ok(status) => (status.state, status.reason),
        Err(err) => {
            ui::error(&format!("Failed to query search status: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    let result = StatusOutput {
        setup_completed,
        encryption_ready,
        search_state,
        search_reason,
    };

    if let Err(err) = output::print_result(&result, json) {
        ui::error(&err);
        return exit_codes::EXIT_ERROR;
    }

    exit_codes::EXIT_SUCCESS
}
