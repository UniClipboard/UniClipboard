//! `uniclip dev dump-clipboard` —— 调试 / E2E 测试用：读出最近 N 条剪贴板
//! 条目的明文 preview。
//!
//! 通过统一核心读取解密后的历史预览。switch-space 之后跑一次能验证旧
//! 数据被正确重加密成新 master_key 加密的密文（解出来明文一致）。

use serde::Serialize;

use uc_engine::{ListHistoryEntriesInput, Operation, OperationResult};

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

    match bundle.recover_session().await {
        Ok(true) => {}
        Ok(false) => {
            ui::error(
                "This device is not set up yet. Use `uniclip space init` or `uniclip space join` first.",
            );
            bundle.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
        Err(err) => {
            ui::error(&format!("Resume failed: {err}"));
            bundle.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
    }

    let entries = match bundle
        .engine()
        .execute(Operation::ListHistoryEntries(ListHistoryEntriesInput {
            limit: args.limit.min(u32::MAX as usize) as u32,
            offset: 0,
        }))
        .await
    {
        Ok(OperationResult::HistoryEntries(entries)) => entries,
        Ok(_) => {
            ui::error("Failed to list clipboard entries: unexpected engine response");
            bundle.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
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
                entry_id: &e.entry_id,
                preview: &e.preview,
                size_bytes: e.size_bytes,
                captured_at: e.captured_at_ms,
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
            println!("ENTRY {}|{}", entry.entry_id, entry.preview);
        }
    }

    bundle.shutdown().await;
    exit_codes::EXIT_SUCCESS
}
