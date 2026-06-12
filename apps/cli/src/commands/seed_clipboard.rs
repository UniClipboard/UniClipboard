//! `uniclip dev seed-clipboard` —— 调试 / E2E 测试用：往本地 SQLite 落一条
//! 文本剪贴板条目（用当前 session master_key 加密）。
//!
//! 与生产路径 `CaptureClipboardUseCase` 不同——不走 normalization /
//! representation policy / spool 这些链路；只需要"一条已加密的剪贴板
//! 历史"作为 switch-space 数据完整性测试的种子。
//!
//! 必须先 init 或 join 过（session 要被 keyring 静默解锁），否则
//! `try_resume_session` 会返回 false。

use uc_application::facade::space_setup::TryResumeSessionError;

use crate::commands::app_session::{build_app_session, refuse_if_daemon_running};
use crate::exit_codes;
use crate::ui;

pub struct SeedClipboardArgs {
    pub text: String,
}

pub async fn run(args: SeedClipboardArgs, verbose: bool) -> i32 {
    ui::header("Seed clipboard entry");

    if let Err(code) = refuse_if_daemon_running().await {
        return code;
    }

    let bundle = match build_app_session(verbose).await {
        Ok(b) => b,
        Err(code) => return code,
    };

    // seed 走 EncryptingClipboardEventWriter，要求 session 已解锁。
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

    let result = bundle
        .app_facade()
        .clipboard_history
        .seed_text_entry(&args.text)
        .await;

    let exit = match result {
        Ok(entry_id) => {
            ui::info("entry_id", &entry_id);
            ui::info("size_bytes", &args.text.len().to_string());
            // SEED_ENTRY_ID= grep-friendly line for the e2e shell script.
            println!("SEED_ENTRY_ID={entry_id}");
            exit_codes::EXIT_SUCCESS
        }
        Err(err) => {
            ui::error(&format!("Failed to seed clipboard entry: {err}"));
            exit_codes::EXIT_ERROR
        }
    };

    bundle.shutdown().await;
    exit
}
