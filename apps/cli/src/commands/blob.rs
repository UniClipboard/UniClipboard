//! `uniclip blob` —— 大 payload 发布 / 拉取诊断命令。
//!
//! 这组命令走和后续 daemon/UI 相同的应用层门面:先恢复空间会话,再执行
//! hash 去重、业务加解密和 iroh-blobs 发布/拉取。`publish` 输出 ticket
//! 与 entry_id,`fetch` 带回二者:ticket 定位内容,entry_id 登记归属。

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use bytes::Bytes;
use clap::Subcommand;
use serde::Serialize;

use uc_application::facade::space_setup::TryResumeSessionError;
use uc_application::facade::{FetchBlobCommand, PublishBlobCommand};
use uc_core::ids::EntryId;
use uc_core::ports::blob::BlobTicket;

use crate::commands::app_session::{build_app_session, refuse_if_daemon_running, CliAppSession};
use crate::exit_codes;
use crate::ui;

#[derive(Subcommand)]
pub enum BlobCommands {
    /// Publish a local file and print the information needed to fetch it.
    Publish {
        /// File to publish.
        path: PathBuf,
    },
    /// Fetch a blob and write the decrypted content to a local file.
    Fetch {
        /// Base64 ticket printed by `blob publish`.
        ticket: String,
        /// Entry id printed by `blob publish`.
        #[arg(long)]
        entry_id: String,
        /// Output file path.
        #[arg(long)]
        out: PathBuf,
    },
}

pub async fn run(command: BlobCommands, json: bool, verbose: bool) -> i32 {
    if !json {
        ui::header("Blob");
    }

    if let Err(code) = refuse_if_daemon_running().await {
        return code;
    }

    match command {
        BlobCommands::Publish { path } => publish(path, json, verbose).await,
        BlobCommands::Fetch {
            ticket,
            entry_id,
            out,
        } => fetch(ticket, entry_id, out, json, verbose).await,
    }
}

async fn publish(path: PathBuf, json: bool, verbose: bool) -> i32 {
    let plaintext = match tokio::fs::read(&path).await {
        Ok(bytes) if bytes.is_empty() => {
            ui::error("File is empty — nothing to publish.");
            return exit_codes::EXIT_ERROR;
        }
        Ok(bytes) => bytes,
        Err(err) => {
            ui::error(&format!("Failed to read file: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    let cli = match build_ready_session(verbose).await {
        Ok(cli) => cli,
        Err(code) => return code,
    };

    let spinner = ui::spinner("Publishing blob...");
    let result = cli
        .app_facade()
        .publish_blob(PublishBlobCommand {
            plaintext: Bytes::from(plaintext),
            entry_id: None,
        })
        .await;

    let result = match result {
        Ok(result) => {
            ui::spinner_finish_success(&spinner, "Blob published");
            result
        }
        Err(err) => {
            ui::spinner_finish_error(&spinner, &format!("Publish failed: {err}"));
            cli.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
    };

    let dto = PublishBlobDto {
        ticket: STANDARD.encode(result.ticket.as_bytes()),
        entry_id: result.entry_id.to_string(),
        plaintext_hash: format_hex(result.plaintext_hash.as_bytes()),
        digest: format_hex(result.digest.as_bytes()),
        reused_existing: result.reused_existing,
    };
    let code = print_publish(dto, json);
    cli.shutdown().await;
    code
}

async fn fetch(ticket: String, entry_id: String, out: PathBuf, json: bool, verbose: bool) -> i32 {
    let ticket = match STANDARD.decode(ticket.trim()) {
        Ok(bytes) => BlobTicket::from_bytes(bytes),
        Err(err) => {
            ui::error(&format!("Invalid ticket: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };
    let entry_id = EntryId::from_str(entry_id.trim());

    let cli = match build_ready_session(verbose).await {
        Ok(cli) => cli,
        Err(code) => return code,
    };

    let spinner = ui::spinner("Fetching blob...");
    let result = cli
        .app_facade()
        .fetch_blob(FetchBlobCommand {
            ticket,
            entry_id,
            transfer_context: None,
        })
        .await;

    let result = match result {
        Ok(result) => {
            ui::spinner_finish_success(&spinner, "Blob fetched");
            result
        }
        Err(err) => {
            ui::spinner_finish_error(&spinner, &format!("Fetch failed: {err}"));
            cli.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
    };

    if let Err(err) = ensure_parent_dir(&out).await {
        ui::error(&format!("Failed to prepare output path: {err}"));
        cli.shutdown().await;
        return exit_codes::EXIT_ERROR;
    }
    if let Err(err) = tokio::fs::write(&out, &result.plaintext).await {
        ui::error(&format!("Failed to write output file: {err}"));
        cli.shutdown().await;
        return exit_codes::EXIT_ERROR;
    }

    let dto = FetchBlobDto {
        out: out.display().to_string(),
        entry_id: result.entry_id.to_string(),
        plaintext_hash: format_hex(result.plaintext_hash.as_bytes()),
        digest: format_hex(result.digest.as_bytes()),
        bytes_written: result.plaintext.len(),
    };
    let code = print_fetch(dto, json);
    cli.shutdown().await;
    code
}

async fn build_ready_session(verbose: bool) -> Result<CliAppSession, i32> {
    let cli = build_app_session(verbose).await?;
    let resume_spinner = ui::spinner("Resuming space session...");
    match cli.app_facade().try_resume_session().await {
        Ok(true) => {
            ui::spinner_finish_success(&resume_spinner, "Session resumed");
            Ok(cli)
        }
        Ok(false) => {
            ui::spinner_finish_error(
                &resume_spinner,
                "No space on this profile — run `init` or `join` first.",
            );
            cli.shutdown().await;
            Err(exit_codes::EXIT_ERROR)
        }
        Err(TryResumeSessionError::CorruptedKeyMaterial) => {
            ui::spinner_finish_error(
                &resume_spinner,
                "Key material is corrupted — consider resetting this profile.",
            );
            cli.shutdown().await;
            Err(exit_codes::EXIT_ERROR)
        }
        Err(TryResumeSessionError::KeyringMiss) => {
            ui::spinner_finish_error(
                &resume_spinner,
                "Keychain cannot silently unlock this space.",
            );
            cli.shutdown().await;
            Err(exit_codes::EXIT_ERROR)
        }
        Err(TryResumeSessionError::Internal(msg)) => {
            ui::spinner_finish_error(&resume_spinner, &format!("Resume failed: {msg}"));
            cli.shutdown().await;
            Err(exit_codes::EXIT_ERROR)
        }
    }
}

async fn ensure_parent_dir(path: &Path) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        tokio::fs::create_dir_all(parent).await?;
    }
    Ok(())
}

fn print_publish(dto: PublishBlobDto, json: bool) -> i32 {
    if json {
        match serde_json::to_string_pretty(&dto) {
            Ok(s) => println!("{s}"),
            Err(err) => {
                ui::error(&format!("Failed to serialize publish result: {err}"));
                return exit_codes::EXIT_ERROR;
            }
        }
    } else {
        println!("ticket: {}", dto.ticket);
        println!("entry_id: {}", dto.entry_id);
        println!("plaintext_hash: {}", dto.plaintext_hash);
        println!("digest: {}", dto.digest);
        println!("reused_existing: {}", dto.reused_existing);
    }
    exit_codes::EXIT_SUCCESS
}

fn print_fetch(dto: FetchBlobDto, json: bool) -> i32 {
    if json {
        match serde_json::to_string_pretty(&dto) {
            Ok(s) => println!("{s}"),
            Err(err) => {
                ui::error(&format!("Failed to serialize fetch result: {err}"));
                return exit_codes::EXIT_ERROR;
            }
        }
    } else {
        println!("out: {}", dto.out);
        println!("entry_id: {}", dto.entry_id);
        println!("plaintext_hash: {}", dto.plaintext_hash);
        println!("digest: {}", dto.digest);
        println!("bytes_written: {}", dto.bytes_written);
    }
    exit_codes::EXIT_SUCCESS
}

fn format_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

#[derive(Serialize)]
struct PublishBlobDto {
    ticket: String,
    entry_id: String,
    plaintext_hash: String,
    digest: String,
    reused_existing: bool,
}

#[derive(Serialize)]
struct FetchBlobDto {
    out: String,
    entry_id: String,
    plaintext_hash: String,
    digest: String,
    bytes_written: usize,
}
