//! Status 命令:直连应用层显示应用状态。

use serde::Serialize;
use std::fmt;

use crate::exit_codes;
use crate::output;

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
    let profile = if verbose {
        Some(uc_observability::LogProfile::Dev)
    } else {
        Some(uc_observability::LogProfile::Cli)
    };

    let app_facade = match uc_bootstrap::build_cli_app_facade(profile).await {
        Ok(facade) => facade,
        Err(err) => {
            eprintln!("Error: failed to build CLI runtime: {err}");
            return exit_codes::EXIT_ERROR;
        }
    };

    let encryption = match app_facade.encryption_state().await {
        Ok(state) => state,
        Err(err) => {
            eprintln!("Error: failed to query application status: {err}");
            return exit_codes::EXIT_ERROR;
        }
    };

    let search = match app_facade.search_status().await {
        Ok(status) => status,
        Err(err) => {
            eprintln!("Error: failed to query search status: {err}");
            return exit_codes::EXIT_ERROR;
        }
    };

    let result = StatusOutput {
        setup_completed: encryption.initialized,
        encryption_ready: encryption.session_ready,
        search_state: search.state,
        search_reason: search.reason,
    };

    if let Err(err) = output::print_result(&result, json) {
        eprintln!("Error: {err}");
        return exit_codes::EXIT_ERROR;
    }

    exit_codes::EXIT_SUCCESS
}
