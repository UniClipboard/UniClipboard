//! `uniclip mobile-sync disable` —— 一键停用:同时关总开关 + 关 LAN 监听。
//!
//! 老版只关总开关,留 LAN listener 仍处于 `lan_listen_enabled=true` 的不一致
//! 状态(daemon 重启后又会再起)。本步骤(Step 4/5)收窄行为:disable 应当
//! 让 mobile sync 完整下线 —— 总开关关 + LAN 关。已配对设备保留(撤销有
//! `devices revoke`),只是 LAN listener 不会再启。

use serde::Serialize;

use uc_application::facade::UpdateMobileSyncSettingsInput;

use crate::commands::mobile_sync::shared;
use crate::exit_codes;
use crate::ui;

#[derive(Serialize)]
struct DisableResult {
    enabled: bool,
    lan_listen_enabled: bool,
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
            lan_listen_enabled: Some(false),
            ..Default::default()
        })
        .await;

    match result {
        Ok(out) => {
            if json {
                let dto = DisableResult {
                    enabled: out.enabled,
                    lan_listen_enabled: out.lan_listen_enabled,
                    restart_required: out.restart_required,
                };
                shared::finish_json(ctx, &dto).await
            } else {
                ui::success("Mobile-sync disabled (master switch + LAN listener).");
                ui::info(
                    "note",
                    "Paired devices remain registered. Revoke individually with \
                     `uniclip mobile-sync devices revoke`.",
                );
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
