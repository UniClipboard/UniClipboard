//! `uniclip mobile debug ...` —— SyncClipboard 协议链路的本地回归命令。
//!
//! P5a.9 引入,便于无 iPhone 时手动验证整条 SyncClipboard 协议链路。
//! 全部 4 个子命令 **绕过 HTTP** 直接调 [`MobileSyncFacade`],模拟 iPhone
//! 客户端的 4 条 SyncClipboard 协议路由(`GET/PUT /SyncClipboard.json` 与
//! `GET/PUT /file/{name}`)。
//!
//! | 子命令 | 模拟的 iPhone 动作 |
//! |---|---|
//! | `put-text <TEXT>` | iPhone `PUT /SyncClipboard.json` (Text type) |
//! | `put-file <PATH>` | iPhone 的两步 PUT:`PUT /file/{name}` + `PUT /SyncClipboard.json` |
//! | `get-doc` | iPhone `GET /SyncClipboard.json` |
//! | `get-file <DATANAME>` | iPhone `GET /file/{dataName}` |
//!
//! ## 与 daemon 的关系
//!
//! 全部子命令均经 `shared::enter_write` 拒绝同 profile 的 daemon —— CLI 与
//! daemon 共享同一份 sqlite,不能并发持有。运行流程:`uniclip stop` →
//! 跑 debug 命令 → 重启 daemon 看效果。
//!
//! ## CLI fallback 装配
//!
//! P5a.9 把 [`build_fallback_apply_inbound`][1] 升级为 **真
//! [`CaptureClipboardUseCase`] + `NoopInboundWrite`**:put-text / put-file
//! 真能写库,后续 `get-doc` / `get-file` 直接读得到。OS 系统剪贴板写入
//! 仍是 daemon 的责任,不在 CLI 责任范围。
//!
//! [`MobileSyncFacade`]: uc_application::facade::MobileSyncFacade
//! [`CaptureClipboardUseCase`]: uc_application::clipboard_capture::CaptureClipboardUseCase
//! [1]: uc_bootstrap::build_app_facade_from_deps

use std::path::{Path, PathBuf};

use clap::Subcommand;
use serde::Serialize;

use uc_application::facade::space_setup::TryResumeSessionError;
use uc_application::facade::{
    ApplyIncomingMobileClipOutcome, GetMobileSyncFileOutput, SyncClipboardItemType,
    SyncClipboardMeta,
};
use uc_core::mobile_sync::MobileDeviceId;

use crate::commands::mobile_sync::shared::{self, MobileSyncCmdCtx};
use crate::exit_codes;
use crate::ui;

#[derive(Subcommand)]
pub enum DebugCommands {
    /// Simulate iPhone `PUT /SyncClipboard.json` with a Text payload.
    PutText {
        /// Text content to send.
        text: String,
    },
    /// Simulate the two-step iPhone PUT (file then doc) for an Image / File
    /// payload.
    PutFile {
        /// Path to the local file to send.
        path: PathBuf,
        /// Override the inferred MIME type (e.g. `image/png`).
        #[arg(long, value_name = "MIME")]
        mime: Option<String>,
    },
    /// Simulate iPhone `GET /SyncClipboard.json` (latest meta).
    GetDoc,
    /// Simulate iPhone `GET /file/{dataName}`.
    GetFile {
        /// dataName as printed by `get-doc`.
        data_name: String,
        /// Optional path to write the bytes to. Without it, only metadata
        /// (mime / size) is printed — binary payloads aren't dumped to stdout.
        #[arg(long, value_name = "PATH")]
        output: Option<PathBuf>,
    },
}

pub async fn run(command: DebugCommands, json: bool, verbose: bool) -> i32 {
    match command {
        DebugCommands::PutText { text } => put_text(text, json, verbose).await,
        DebugCommands::PutFile { path, mime } => put_file(path, mime, json, verbose).await,
        DebugCommands::GetDoc => get_doc(json, verbose).await,
        DebugCommands::GetFile { data_name, output } => {
            get_file(data_name, output, json, verbose).await
        }
    }
}

/// Pseudo source device id used by all `debug` subcommands. Becomes the
/// suffix of the apply_incoming pseudo `DeviceId("mobile_sync:debug-cli")`,
/// keeping debug-driven entries visually distinguishable from real iPhone
/// uploads in the event log.
fn debug_source_device_id() -> MobileDeviceId {
    MobileDeviceId::new("debug-cli")
}

/// Silently resume the encryption session before any debug command that
/// touches the clipboard pipeline. capture / dedup / snapshot reads all
/// require an unlocked space. Mirrors `dev seed-clipboard` (see
/// `commands::seed_clipboard`); without this, capture errors out with
/// `failed to encrypt inline_data` and snapshot reads fail with
/// `failed to decrypt inline_data`.
///
/// On error, prints the user-facing message and returns the exit code; the
/// caller propagates via `shared::finish(ctx, code).await`.
async fn ensure_session_resumed(ctx: &MobileSyncCmdCtx) -> Result<(), i32> {
    match ctx.cli.app_facade().try_resume_session().await {
        Ok(true) => Ok(()),
        Ok(false) => {
            ui::error(
                "This profile is not set up yet. Run `uniclip init` (or `uniclip join`) first.",
            );
            Err(exit_codes::EXIT_ERROR)
        }
        Err(TryResumeSessionError::CorruptedKeyMaterial) => {
            ui::error("Key material is corrupted — consider resetting this profile.");
            Err(exit_codes::EXIT_ERROR)
        }
        Err(TryResumeSessionError::KeyringMiss) => {
            ui::error(
                "Keychain cannot silently unlock this space. Re-run `uniclip init` or `uniclip join`.",
            );
            Err(exit_codes::EXIT_ERROR)
        }
        Err(TryResumeSessionError::Internal(msg)) => {
            ui::error(&format!("Resume failed: {msg}"));
            Err(exit_codes::EXIT_ERROR)
        }
    }
}

// ── put-text ────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct PutOutcomeDto {
    outcome: &'static str,
    entry_id: Option<String>,
    snapshot_hash: Option<String>,
    existing_entry_id: Option<String>,
    decode_reason: Option<String>,
}

impl From<&ApplyIncomingMobileClipOutcome> for PutOutcomeDto {
    fn from(o: &ApplyIncomingMobileClipOutcome) -> Self {
        match o {
            ApplyIncomingMobileClipOutcome::Applied { entry_id } => Self {
                outcome: "applied",
                entry_id: Some(entry_id.to_string()),
                snapshot_hash: None,
                existing_entry_id: None,
                decode_reason: None,
            },
            ApplyIncomingMobileClipOutcome::DuplicateSkipped {
                snapshot_hash,
                existing_entry_id,
            } => Self {
                outcome: "duplicate_skipped",
                entry_id: None,
                snapshot_hash: Some(snapshot_hash.clone()),
                existing_entry_id: Some(existing_entry_id.to_string()),
                decode_reason: None,
            },
            ApplyIncomingMobileClipOutcome::DecodeFailed { reason } => Self {
                outcome: "decode_failed",
                entry_id: None,
                snapshot_hash: None,
                existing_entry_id: None,
                decode_reason: Some(reason.clone()),
            },
            ApplyIncomingMobileClipOutcome::Buffered => Self {
                outcome: "buffered",
                entry_id: None,
                snapshot_hash: None,
                existing_entry_id: None,
                decode_reason: None,
            },
        }
    }
}

fn print_outcome(label: &str, outcome: &ApplyIncomingMobileClipOutcome) {
    match outcome {
        ApplyIncomingMobileClipOutcome::Applied { entry_id } => {
            ui::success(&format!("{label}: applied"));
            ui::info("entryId", &entry_id.to_string());
        }
        ApplyIncomingMobileClipOutcome::DuplicateSkipped {
            snapshot_hash,
            existing_entry_id,
        } => {
            ui::info(label, "duplicate_skipped");
            ui::info("snapshotHash", snapshot_hash);
            ui::info("existingEntryId", &existing_entry_id.to_string());
        }
        ApplyIncomingMobileClipOutcome::DecodeFailed { reason } => {
            ui::warn(&format!("{label}: decode_failed"));
            ui::info("reason", reason);
        }
        ApplyIncomingMobileClipOutcome::Buffered => {
            ui::info(label, "buffered");
        }
    }
}

async fn put_text(text: String, json: bool, verbose: bool) -> i32 {
    let ctx =
        match shared::enter_write("Debug: PUT /SyncClipboard.json (Text)", json, verbose).await {
            Ok(c) => c,
            Err(code) => return code,
        };
    if let Err(code) = ensure_session_resumed(&ctx).await {
        return shared::finish(ctx, code).await;
    }

    let size = text.len() as u64;
    let meta = SyncClipboardMeta {
        item_type: SyncClipboardItemType::Text,
        text,
        data_name: None,
        has_data: false,
        size,
        hash: None,
    };

    match ctx
        .facade
        .put_sync_doc(meta, debug_source_device_id())
        .await
    {
        Ok(outcome) => {
            if json {
                let dto = PutOutcomeDto::from(&outcome);
                shared::finish_json(ctx, &dto).await
            } else {
                print_outcome("PUT /SyncClipboard.json", &outcome);
                shared::finish(ctx, exit_codes::EXIT_SUCCESS).await
            }
        }
        Err(err) => {
            ui::error(&shared::render_apply_incoming_error(&err));
            shared::finish(ctx, exit_codes::EXIT_ERROR).await
        }
    }
}

// ── put-file ────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct PutFileDto {
    file: PutOutcomeDto,
    doc: PutOutcomeDto,
}

async fn put_file(path: PathBuf, mime_override: Option<String>, json: bool, verbose: bool) -> i32 {
    if !json {
        ui::header("Debug: two-step PUT (file → doc)");
    }

    // Read the file + derive shape BEFORE wiring the CLI session — keeps
    // session lifetime short on early file errors.
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(err) => {
            ui::error(&format!("Failed to read {}: {err}", path.display()));
            return exit_codes::EXIT_ERROR;
        }
    };
    let data_name = match path.file_name().and_then(|s| s.to_str()) {
        Some(name) => name.to_string(),
        None => {
            ui::error(&format!("Path {} has no usable file name", path.display()));
            return exit_codes::EXIT_ERROR;
        }
    };
    let mime = mime_override.unwrap_or_else(|| infer_mime(&path));
    let item_type = if mime.starts_with("image/") {
        SyncClipboardItemType::Image
    } else {
        SyncClipboardItemType::File
    };
    let size = bytes.len() as u64;

    // Header already printed; pass json=true to suppress duplicate.
    let ctx = match shared::enter_write("", true, verbose).await {
        Ok(c) => c,
        Err(code) => return code,
    };
    if let Err(code) = ensure_session_resumed(&ctx).await {
        return shared::finish(ctx, code).await;
    }

    let device = debug_source_device_id();

    // CLI debug 路径用自生成 transfer_id —— 不接 mobile_lan listener,
    // 走 `apply_incoming` 的 BufferFile 分支自我闭环;handler 端 lifecycle
    // 钩子在生产路径(uc-webserver)里发,本调试入口不参与。
    let transfer_id = format!("mobile-lan:cli-{}", uuid::Uuid::new_v4());
    let file_outcome = match ctx
        .facade
        .put_clipboard_file(
            data_name.clone(),
            mime.clone(),
            bytes,
            device.clone(),
            transfer_id,
        )
        .await
    {
        Ok(o) => o,
        Err(err) => {
            ui::error(&shared::render_apply_incoming_error(&err));
            return shared::finish(ctx, exit_codes::EXIT_ERROR).await;
        }
    };

    let meta = SyncClipboardMeta {
        item_type,
        text: data_name.clone(),
        data_name: Some(data_name.clone()),
        has_data: true,
        size,
        hash: None,
    };
    let doc_outcome = match ctx.facade.put_sync_doc(meta, device).await {
        Ok(o) => o,
        Err(err) => {
            ui::error(&shared::render_apply_incoming_error(&err));
            return shared::finish(ctx, exit_codes::EXIT_ERROR).await;
        }
    };

    if json {
        let dto = PutFileDto {
            file: PutOutcomeDto::from(&file_outcome),
            doc: PutOutcomeDto::from(&doc_outcome),
        };
        shared::finish_json(ctx, &dto).await
    } else {
        ui::info("dataName", &data_name);
        ui::info("mime", &mime);
        ui::info("size", &size.to_string());
        print_outcome("step1 PUT /file", &file_outcome);
        print_outcome("step2 PUT /SyncClipboard.json", &doc_outcome);
        shared::finish(ctx, exit_codes::EXIT_SUCCESS).await
    }
}

/// Lightweight extension → MIME inference. Covers the SyncClipboard
/// shortcut's image set; everything else falls back to
/// `application/octet-stream`, matching the route layer's default.
fn infer_mime(path: &Path) -> String {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase());
    match ext.as_deref() {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        Some("heic") => "image/heic",
        Some("tiff") | Some("tif") => "image/tiff",
        _ => "application/octet-stream",
    }
    .to_string()
}

// ── get-doc ─────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct DocDto {
    item_type: &'static str,
    text: String,
    data_name: Option<String>,
    has_data: bool,
    size: u64,
    hash: Option<String>,
}

impl From<&SyncClipboardMeta> for DocDto {
    fn from(m: &SyncClipboardMeta) -> Self {
        Self {
            item_type: match m.item_type {
                SyncClipboardItemType::Text => "Text",
                SyncClipboardItemType::Image => "Image",
                SyncClipboardItemType::File => "File",
                SyncClipboardItemType::Group => "Group",
            },
            text: m.text.clone(),
            data_name: m.data_name.clone(),
            has_data: m.has_data,
            size: m.size,
            hash: m.hash.clone(),
        }
    }
}

async fn get_doc(json: bool, verbose: bool) -> i32 {
    let ctx = match shared::enter_write("Debug: GET /SyncClipboard.json", json, verbose).await {
        Ok(c) => c,
        Err(code) => return code,
    };
    if let Err(code) = ensure_session_resumed(&ctx).await {
        return shared::finish(ctx, code).await;
    }

    match ctx.facade.get_latest_sync_doc().await {
        Ok(meta) => {
            if json {
                let dto = DocDto::from(&meta);
                shared::finish_json(ctx, &dto).await
            } else {
                ui::info("type", DocDto::from(&meta).item_type);
                ui::info("text", &meta.text);
                if let Some(name) = &meta.data_name {
                    ui::info("dataName", name);
                }
                ui::info("hasData", &meta.has_data.to_string());
                ui::info("size", &meta.size.to_string());
                if let Some(hash) = &meta.hash {
                    ui::info("hash", hash);
                }
                shared::finish(ctx, exit_codes::EXIT_SUCCESS).await
            }
        }
        Err(err) => {
            ui::error(&shared::render_get_latest_doc_error(&err));
            shared::finish(ctx, exit_codes::EXIT_ERROR).await
        }
    }
}

// ── get-file ────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct FileDto {
    data_name: String,
    mime: String,
    size: u64,
    output_path: Option<String>,
}

async fn get_file(data_name: String, output: Option<PathBuf>, json: bool, verbose: bool) -> i32 {
    let ctx = match shared::enter_write("Debug: GET /file/{dataName}", json, verbose).await {
        Ok(c) => c,
        Err(code) => return code,
    };
    if let Err(code) = ensure_session_resumed(&ctx).await {
        return shared::finish(ctx, code).await;
    }

    match ctx.facade.get_clipboard_file(&data_name).await {
        Ok(out) => {
            let GetMobileSyncFileOutput { mime, bytes } = out;
            let size = bytes.len() as u64;
            let written_to = match &output {
                Some(path) => match std::fs::write(path, &bytes) {
                    Ok(()) => Some(path.display().to_string()),
                    Err(err) => {
                        ui::error(&format!("Failed to write {}: {err}", path.display()));
                        return shared::finish(ctx, exit_codes::EXIT_ERROR).await;
                    }
                },
                None => None,
            };
            if json {
                let dto = FileDto {
                    data_name: data_name.clone(),
                    mime,
                    size,
                    output_path: written_to,
                };
                shared::finish_json(ctx, &dto).await
            } else {
                ui::info("dataName", &data_name);
                ui::info("mime", &mime);
                ui::info("size", &size.to_string());
                if let Some(path) = &written_to {
                    ui::success(&format!("Wrote {size} bytes to {path}"));
                } else {
                    ui::info(
                        "note",
                        "Pass --output <PATH> to dump bytes to a file (binary not printed to stdout).",
                    );
                }
                shared::finish(ctx, exit_codes::EXIT_SUCCESS).await
            }
        }
        Err(err) => {
            ui::error(&shared::render_get_file_error(&err));
            shared::finish(ctx, exit_codes::EXIT_ERROR).await
        }
    }
}
