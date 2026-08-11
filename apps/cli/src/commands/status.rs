//! Status 命令:通过 daemon 显示应用状态。

use serde::Serialize;
use std::fmt;
use uc_daemon_contract::api::dto::member::WorkspaceConvergencePhaseDto;

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
    workspace_convergence: WorkspaceConvergencePhaseDto,
}

impl fmt::Display for StatusOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let setup = if self.setup_completed { "yes" } else { "no" };
        let encryption = if self.encryption_ready { "yes" } else { "no" };
        let reason = self.search_reason.as_deref().unwrap_or("none");

        writeln!(f, "Setup completed: {setup}")?;
        writeln!(f, "Encryption ready: {encryption}")?;
        writeln!(f, "Search state: {}", self.search_state)?;
        writeln!(f, "Search reason: {reason}")?;
        write!(
            f,
            "Workspace convergence: {}",
            workspace_convergence_label(self.workspace_convergence)
        )?;
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

    let workspace_convergence = match ctx.member_client().workspace_convergence().await {
        Ok(status) => status.phase,
        Err(err) => {
            ui::error(&format!(
                "Failed to query workspace convergence status: {err}"
            ));
            return exit_codes::EXIT_ERROR;
        }
    };

    let result = StatusOutput {
        setup_completed,
        encryption_ready,
        search_state,
        search_reason,
        workspace_convergence,
    };

    if let Err(err) = output::print_result(&result, json) {
        ui::error(&err);
        return exit_codes::EXIT_ERROR;
    }

    exit_codes::EXIT_SUCCESS
}

fn workspace_convergence_label(state: WorkspaceConvergencePhaseDto) -> &'static str {
    match state {
        WorkspaceConvergencePhaseDto::LocallyApplied => "locally_applied",
        WorkspaceConvergencePhaseDto::Converging => "converging",
        WorkspaceConvergencePhaseDto::WaitingForOfflineMember => "waiting_for_offline_member",
        WorkspaceConvergencePhaseDto::Complete => "complete",
        WorkspaceConvergencePhaseDto::RecoveryRequired => "recovery_required",
    }
}

#[cfg(test)]
mod tests {
    use super::StatusOutput;
    use serde_json::json;
    use uc_daemon_contract::api::dto::member::WorkspaceConvergencePhaseDto;

    #[test]
    fn json_includes_workspace_convergence_phase() {
        let output = StatusOutput {
            setup_completed: true,
            encryption_ready: true,
            search_state: "ready".to_string(),
            search_reason: None,
            workspace_convergence: WorkspaceConvergencePhaseDto::Converging,
        };

        let value = serde_json::to_value(output).expect("serialize status output");
        assert_eq!(value["workspace_convergence"], json!("converging"));
    }
}
