//! `uniclip dev dump-clipboard` —— 调试 / E2E 测试用：读出最近 N 条剪贴板
//! 条目的明文 preview。
//!
//! 走 `ClipboardHistoryFacade::list_entries`，背后是
//! `DecryptingClipboardRepresentationRepository`，所以输出的就是用当前
//! session master_key 解密后的明文。switch-space 之后跑一次能验证旧
//! 数据被正确重加密成新 master_key 加密的密文（解出来明文一致）。

use serde::Serialize;

use uc_application::facade::space_setup::TryResumeSessionError;
use uc_application::facade::ClipboardListInput;

use crate::commands::app_session::{build_app_session, refuse_if_daemon_running};
use crate::exit_codes;
use crate::ui;

pub struct DumpClipboardArgs {
    pub limit: usize,
}

#[derive(Serialize)]
struct DumpEntryDto<'a> {
    entry_id: &'a str,
    preview: &'a str,
    size_bytes: i64,
    captured_at: i64,
    content_type: &'a str,
}

pub async fn run(args: DumpClipboardArgs, json: bool, verbose: bool) -> i32 {
    if !json {
        ui::header("Dump clipboard entries");
    }

    if let Err(code) = refuse_if_daemon_running().await {
        return code;
    }

    let bundle = match build_app_session(verbose).await {
        Ok(b) => b,
        Err(code) => return code,
    };

    match bundle.app_facade().try_resume_session().await {
        Ok(true) => {}
        Ok(false) => {
            ui::error("This device is not set up yet. Use `uniclip init` or `uniclip join` first.");
            bundle.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
        Err(TryResumeSessionError::CorruptedKeyMaterial) => {
            ui::error("Key material is corrupted — consider resetting this profile.");
            bundle.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
        Err(TryResumeSessionError::KeyringMiss) => {
            ui::error("Keychain cannot silently unlock this space.");
            bundle.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
        Err(TryResumeSessionError::Internal(msg)) => {
            ui::error(&format!("Resume failed: {msg}"));
            bundle.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
    }

    let entries = match bundle
        .app_facade()
        .clipboard_history
        .list_entries(ClipboardListInput {
            limit: args.limit,
            offset: 0,
        })
        .await
    {
        Ok(v) => v,
        Err(err) => {
            ui::error(&format!("Failed to list clipboard entries: {err}"));
            bundle.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
    };

    if json {
        let dto: Vec<DumpEntryDto<'_>> = entries
            .iter()
            .map(|e| DumpEntryDto {
                entry_id: &e.id,
                preview: &e.preview,
                size_bytes: e.size_bytes,
                captured_at: e.captured_at,
                content_type: &e.content_type,
            })
            .collect();
        match serde_json::to_string_pretty(&dto) {
            Ok(s) => println!("{s}"),
            Err(err) => {
                ui::error(&format!("Failed to serialize entries: {err}"));
                bundle.shutdown().await;
                return exit_codes::EXIT_ERROR;
            }
        }
    } else {
        ui::info("count", &entries.len().to_string());
        for entry in &entries {
            // Each entry as a single grep-friendly line for the e2e shell
            // script: `ENTRY <id>|<preview>`. preview is the decrypted
            // text bytes from `representation_repo.get_representation`.
            println!("ENTRY {}|{}", entry.id, entry.preview);
        }
    }

    bundle.shutdown().await;
    exit_codes::EXIT_SUCCESS
}
