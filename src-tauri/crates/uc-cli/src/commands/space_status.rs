//! Space status command -- shows encryption state via direct bootstrap (no daemon required).

use serde::Serialize;
use std::fmt;

use crate::exit_codes;
use crate::output;

#[derive(Serialize)]
struct SpaceStatusOutput {
    encryption_ready: bool,
    encryption_state: String,
}

impl fmt::Display for SpaceStatusOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ready_str = if self.encryption_ready { "yes" } else { "no" };
        writeln!(f, "Encryption ready: {}", ready_str)?;
        write!(f, "Encryption state: {}", self.encryption_state)?;
        Ok(())
    }
}

/// Run the space-status command.
///
/// Uses `build_cli_context()` + `build_non_gui_runtime()` to query encryption
/// state directly without requiring the daemon to be running.
pub async fn run(json: bool) -> i32 {
    let ctx = match uc_bootstrap::build_cli_context() {
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

    let encryption_ready = runtime.is_encryption_ready().await;
    let encryption_state = match runtime.encryption_state().await {
        Ok(state) => format!("{:?}", state),
        Err(e) => {
            eprintln!("Error: failed to query encryption state: {}", e);
            return exit_codes::EXIT_ERROR;
        }
    };

    let result = SpaceStatusOutput {
        encryption_ready,
        encryption_state,
    };

    if let Err(e) = output::print_result(&result, json) {
        eprintln!("Error: {}", e);
        return exit_codes::EXIT_ERROR;
    }

    exit_codes::EXIT_SUCCESS
}
