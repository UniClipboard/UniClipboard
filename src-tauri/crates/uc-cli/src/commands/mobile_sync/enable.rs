//! `uniclip mobile-sync enable` —— 打开移动端同步总开关。

use serde::Serialize;

use uc_application::facade::UpdateMobileSyncSettingsInput;

use crate::commands::app_session::{build_app_session, refuse_if_daemon_running};
use crate::commands::mobile_sync::shared;
use crate::exit_codes;
use crate::ui;

#[derive(Serialize)]
struct EnableResult {
    enabled: bool,
    restart_required: bool,
}

pub async fn run(json: bool, verbose: bool) -> i32 {
    if !json {
        ui::header("Mobile-sync enable");
    }
    if let Err(code) = refuse_if_daemon_running().await {
        return code;
    }
    let cli = match build_app_session(verbose).await {
        Ok(cli) => cli,
        Err(code) => return code,
    };
    let Some(facade) = cli.app_facade().mobile_sync.clone() else {
        ui::error("Mobile-sync facade is not wired in this build.");
        cli.shutdown().await;
        return exit_codes::EXIT_ERROR;
    };

    let result = facade
        .update_settings(UpdateMobileSyncSettingsInput {
            enabled: Some(true),
            ..Default::default()
        })
        .await;

    let exit = match result {
        Ok(out) => {
            if json {
                let dto = EnableResult {
                    enabled: out.enabled,
                    restart_required: out.restart_required,
                };
                match serde_json::to_string_pretty(&dto) {
                    Ok(s) => println!("{s}"),
                    Err(err) => {
                        ui::error(&format!("Failed to serialize: {err}"));
                        cli.shutdown().await;
                        return exit_codes::EXIT_ERROR;
                    }
                }
            } else {
                ui::success("Mobile-sync feature enabled.");
                if out.restart_required {
                    ui::warn(shared::restart_hint());
                } else {
                    ui::info("note", "Already enabled — no daemon restart needed.");
                }
            }
            exit_codes::EXIT_SUCCESS
        }
        Err(err) => {
            ui::error(&shared::render_update_settings_error(&err));
            exit_codes::EXIT_ERROR
        }
    };

    cli.shutdown().await;
    exit
}
