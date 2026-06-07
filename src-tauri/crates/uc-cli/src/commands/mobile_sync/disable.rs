//! `uniclip mobile-sync disable` — one-shot disable: master switch off + LAN off.
//!
//! Routes through daemon HTTP endpoints (P5-2b ADR).

use serde::Serialize;

use uc_daemon_contract::api::dto::mobile_sync::UpdateMobileSyncSettingsRequest;

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
    let ctx = match shared::enter("Mobile-sync disable", json, verbose).await {
        Ok(c) => c,
        Err(code) => return code,
    };

    let result = ctx
        .client
        .update_settings(&UpdateMobileSyncSettingsRequest {
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
                shared::finish_daemon_json(ctx, &dto).await
            } else {
                ui::success("Mobile-sync disabled (master switch + LAN listener).");
                ui::info(
                    "note",
                    "Paired devices remain registered. Revoke individually with \
                     `uniclip mobile-sync revoke`.",
                );
                if out.restart_required {
                    ui::warn(shared::restart_hint());
                } else {
                    ui::info("note", "Already disabled — no daemon restart needed.");
                }
                shared::finish_daemon(ctx, exit_codes::EXIT_SUCCESS).await
            }
        }
        Err(err) => {
            ui::error(&err.to_string());
            shared::finish_daemon(ctx, exit_codes::EXIT_ERROR).await
        }
    }
}
