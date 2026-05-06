//! `uniclip mobile-sync disable` —— 关闭移动端同步总开关。

use serde::Serialize;

use uc_application::facade::UpdateMobileSyncSettingsInput;

use crate::commands::mobile_sync::shared;
use crate::exit_codes;
use crate::ui;

#[derive(Serialize)]
struct DisableResult {
    enabled: bool,
    restart_required: bool,
}

pub async fn run(json: bool, verbose: bool) -> i32 {
    let ctx = match shared::enter_write("Mobile-sync disable", json, verbose).await {
        Ok(c) => c,
        Err(code) => return code,
    };

    let result = ctx
        .facade
        .update_settings(UpdateMobileSyncSettingsInput {
            enabled: Some(false),
            ..Default::default()
        })
        .await;

    match result {
        Ok(out) => {
            if json {
                let dto = DisableResult {
                    enabled: out.enabled,
                    restart_required: out.restart_required,
                };
                shared::finish_json(ctx, &dto).await
            } else {
                ui::success("Mobile-sync feature disabled.");
                if out.restart_required {
                    ui::warn(shared::restart_hint());
                } else {
                    ui::info("note", "Already disabled — no daemon restart needed.");
                }
                shared::finish(ctx, exit_codes::EXIT_SUCCESS).await
            }
        }
        Err(err) => {
            ui::error(&shared::render_update_settings_error(&err));
            shared::finish(ctx, exit_codes::EXIT_ERROR).await
        }
    }
}
