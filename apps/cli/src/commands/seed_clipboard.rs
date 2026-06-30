//! `uniclip dev seed-clipboard` inserts one text clipboard entry into the
//! local SQLite store for diagnostics and E2E tests. The payload is encrypted
//! with the current session master key.
//!
//! Unlike the production `CaptureClipboardUseCase` path, this command does not
//! run normalization, representation policy, or spool processing. It only needs
//! one encrypted clipboard-history row as deterministic test data.
//!
//! The profile must already be initialized or joined, and secure storage must
//! be able to silently unlock the session.

use uc_application::facade::EncryptionFacadeError;

use crate::commands::app_session::{
    build_local_app_session, refuse_if_daemon_running, CliAppSession,
};
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

    let bundle = match build_local_app_session(verbose).await {
        Ok(b) => b,
        Err(code) => return code,
    };

    // Seeding uses EncryptingClipboardEventWriter, so the local session must be unlocked.
    match bundle.app_facade().encryption.unlock().await {
        Ok(true) => {}
        Ok(false) => {
            ui::error("This device is not set up yet. Use `uniclip init` or `uniclip join` first.");
            shutdown_seed_session(bundle).await;
            return exit_codes::EXIT_ERROR;
        }
        Err(EncryptionFacadeError::SetupStatus(msg)) => {
            ui::error(&format!("Resume failed: {msg}"));
            shutdown_seed_session(bundle).await;
            return exit_codes::EXIT_ERROR;
        }
        Err(EncryptionFacadeError::SpaceAccess(msg)) => {
            ui::error(&format!("Resume failed: {msg}"));
            shutdown_seed_session(bundle).await;
            return exit_codes::EXIT_ERROR;
        }
        Err(EncryptionFacadeError::AlreadyInitialized) => {
            ui::error("Resume failed: encryption is already initialized.");
            shutdown_seed_session(bundle).await;
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

    shutdown_seed_session(bundle).await;
    exit
}

async fn shutdown_seed_session(bundle: CliAppSession) {
    if tokio::time::timeout(std::time::Duration::from_secs(5), bundle.shutdown())
        .await
        .is_err()
    {
        tracing::warn!("seed clipboard session shutdown timed out");
    }
}
