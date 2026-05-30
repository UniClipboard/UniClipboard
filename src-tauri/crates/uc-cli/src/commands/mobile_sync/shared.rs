//! `mobile_sync` 子命令共享的小工具:命令骨架、错误渲染、重启提示、JSON 包装。
//!
//! 命令骨架(boilerplate)由 [`enter_write`] / [`enter_read`] / [`finish_json`] /
//! [`finish`] 提供 —— 见 module-level 注释顶部小段。所有 mobile-sync 子命令
//! 都遵守同一个生命周期:`enter_*` 拿 [`MobileSyncCmdCtx`] → 调 facade →
//! 渲染 → `finish_*`。

use std::sync::Arc;

use serde::Serialize;

use uc_application::facade::{
    ApplyIncomingMobileClipError, GetLatestMobileSyncDocError, GetMobileSyncFileError,
    GetMobileSyncSettingsError, ListMobileDevicesError, MobileSyncFacade,
    MobileSyncListLanInterfacesError, RegisterMobileShortcutDeviceError, RevokeMobileDeviceError,
    UpdateMobileSyncSettingsError,
};

use crate::commands::app_session::{build_app_session, refuse_if_daemon_running, CliAppSession};
use crate::exit_codes;
use crate::ui;

// ── Command lifecycle helpers ────────────────────────────────────────────

/// Wired CLI session + a clone of the mobile-sync facade. Built by
/// [`enter_write`] / [`enter_read`]; consumed by [`finish_json`] / [`finish`].
pub struct MobileSyncCmdCtx {
    pub cli: CliAppSession,
    pub facade: Arc<MobileSyncFacade>,
}

/// Boilerplate for **write commands**: print header (unless json), refuse
/// if daemon is running, build the CLI app session, take the mobile-sync
/// facade. Returns an exit code if any step fails (the inner shutdown is
/// handled before returning, so callers just propagate the code).
pub async fn enter_write(header: &str, json: bool, verbose: bool) -> Result<MobileSyncCmdCtx, i32> {
    if !json {
        ui::header(header);
    }
    refuse_if_daemon_running().await?;
    enter_inner(verbose).await
}

/// Boilerplate for **read commands**: print header (unless json), build
/// the CLI app session, take the mobile-sync facade. Daemon may be running
/// — sqlite tolerates concurrent read-only opens.
pub async fn enter_read(header: &str, json: bool, verbose: bool) -> Result<MobileSyncCmdCtx, i32> {
    if !json {
        ui::header(header);
    }
    enter_inner(verbose).await
}

async fn enter_inner(verbose: bool) -> Result<MobileSyncCmdCtx, i32> {
    let cli = build_app_session(verbose).await?;
    let Some(facade) = cli.app_facade().mobile_sync.get().cloned() else {
        ui::error("Mobile-sync facade is not wired in this build.");
        cli.shutdown().await;
        return Err(exit_codes::EXIT_ERROR);
    };
    Ok(MobileSyncCmdCtx { cli, facade })
}

/// Pretty-print `dto` as JSON to stdout, then shut the ctx down. Returns
/// SUCCESS on serialize ok, ERROR otherwise (shutdown still happens).
pub async fn finish_json<T: Serialize>(ctx: MobileSyncCmdCtx, dto: &T) -> i32 {
    let exit = match serde_json::to_string_pretty(dto) {
        Ok(s) => {
            println!("{s}");
            exit_codes::EXIT_SUCCESS
        }
        Err(err) => {
            ui::error(&format!("Failed to serialize: {err}"));
            exit_codes::EXIT_ERROR
        }
    };
    ctx.cli.shutdown().await;
    exit
}

/// Shut the ctx down, return the given exit code. Use for the
/// human-readable branch where rendering happened inline.
pub async fn finish(ctx: MobileSyncCmdCtx, exit: i32) -> i32 {
    ctx.cli.shutdown().await;
    exit
}

// ── Interactive input helpers (shared by setup / devices add) ───────────

/// Read one line from stdin (no prompt printed). Used when the password
/// arrives via pipe / heredoc — keeps it out of shell history.
pub fn read_password_stdin() -> Result<String, String> {
    use std::io::{self, BufRead};
    let mut buf = String::new();
    io::stdin()
        .lock()
        .read_line(&mut buf)
        .map_err(|e| e.to_string())?;
    Ok(buf.trim_end_matches(['\n', '\r']).to_string())
}

// ── Error renderers + restart hint (kept as-is) ──────────────────────────

/// 把 `restart_required=true` 转化为面向用户的提示字符串(英文,人类可读)。
pub fn restart_hint() -> &'static str {
    "Restart the daemon to apply: `uniclip stop && uniclip start`."
}

pub fn render_get_settings_error(err: &GetMobileSyncSettingsError) -> String {
    match err {
        GetMobileSyncSettingsError::SettingsLoadFailed(msg) => {
            format!("Failed to load mobile-sync settings: {msg}")
        }
        GetMobileSyncSettingsError::EndpointInfoFailed(msg) => {
            format!("Failed to probe LAN endpoint info: {msg}")
        }
    }
}

pub fn render_update_settings_error(err: &UpdateMobileSyncSettingsError) -> String {
    match err {
        UpdateMobileSyncSettingsError::SettingsLoadFailed(msg) => {
            format!("Failed to load settings: {msg}")
        }
        UpdateMobileSyncSettingsError::SettingsSaveFailed(msg) => {
            format!("Failed to save settings: {msg}")
        }
        UpdateMobileSyncSettingsError::InvalidLanParameter(msg) => {
            format!("Invalid LAN parameter: {msg}")
        }
    }
}

pub fn render_list_devices_error(err: &ListMobileDevicesError) -> String {
    match err {
        ListMobileDevicesError::PersistenceFailed(msg) => {
            format!("Failed to list mobile devices: {msg}")
        }
    }
}

pub fn render_revoke_error(err: &RevokeMobileDeviceError) -> String {
    match err {
        RevokeMobileDeviceError::NotFound(id) => {
            format!("Device not found (already revoked?): {id}")
        }
        RevokeMobileDeviceError::PersistenceFailed(msg) => {
            format!("Failed to revoke device: {msg}")
        }
    }
}

pub fn render_register_error(err: &RegisterMobileShortcutDeviceError) -> String {
    match err {
        RegisterMobileShortcutDeviceError::LabelEmpty => "Device label must not be empty.".into(),
        RegisterMobileShortcutDeviceError::LabelTooLong => {
            "Device label is too long (max 64 chars).".into()
        }
        RegisterMobileShortcutDeviceError::LanListenerDisabled => {
            "LAN listener is not enabled — run `uniclip mobile-sync setup` or `network set --ip <IP>` first."
                .into()
        }
        RegisterMobileShortcutDeviceError::UsernameTaken(name) => {
            format!("Username `{name}` is already taken — pick another.")
        }
        RegisterMobileShortcutDeviceError::UsernameTooShort { min, got } => {
            format!("Username is too short: must be at least {min} characters (got {got}).")
        }
        RegisterMobileShortcutDeviceError::UsernameTooLong { max, got } => {
            format!("Username is too long: must be at most {max} characters (got {got}).")
        }
        RegisterMobileShortcutDeviceError::UsernameMustStartWithLetter => {
            "Username must start with an ASCII letter.".into()
        }
        RegisterMobileShortcutDeviceError::UsernameContainsForbiddenChars => {
            "Username contains forbidden characters — only letters, digits, and underscore are allowed.".into()
        }
        RegisterMobileShortcutDeviceError::PasswordTooShort { min } => {
            format!("Password is too short (minimum {min} characters).")
        }
        RegisterMobileShortcutDeviceError::PasswordTooLong { max } => {
            format!("Password is too long (maximum {max} characters).")
        }
        RegisterMobileShortcutDeviceError::PasswordHashFailed(msg) => {
            format!("Password hashing failed: {msg}")
        }
        RegisterMobileShortcutDeviceError::PersistenceFailed(msg) => {
            format!("Persistence failed: {msg}")
        }
        RegisterMobileShortcutDeviceError::QrRenderFailed(msg) => {
            format!("QR rendering failed: {msg}")
        }
        RegisterMobileShortcutDeviceError::SettingsLoadFailed(msg) => {
            format!("Settings load failed: {msg}")
        }
        RegisterMobileShortcutDeviceError::NoLanInterfaceAvailable => {
            "No usable LAN interface found for auto-pick — connect to a LAN or set `lan_advertise_ip` explicitly via `mobile-sync network set --ip <IP>`."
                .into()
        }
        RegisterMobileShortcutDeviceError::LanInterfaceProbeFailed(msg) => {
            format!("LAN interface probe failed: {msg}")
        }
    }
}

pub fn render_list_lan_interfaces_error(err: &MobileSyncListLanInterfacesError) -> String {
    match err {
        MobileSyncListLanInterfacesError::ProbeFailed(msg) => {
            format!("Failed to probe LAN interfaces: {msg}")
        }
    }
}

// ── P5a.9 debug subcommand error renderers ──────────────────────────────

pub fn render_apply_incoming_error(err: &ApplyIncomingMobileClipError) -> String {
    match err {
        ApplyIncomingMobileClipError::Inbound(inner) => {
            format!("Inbound clipboard apply failed: {inner}")
        }
        ApplyIncomingMobileClipError::EncodeFailed(msg) => {
            format!("V3 envelope encode failed: {msg}")
        }
        ApplyIncomingMobileClipError::Internal(msg) => {
            format!("Internal apply error: {msg}")
        }
    }
}

pub fn render_get_latest_doc_error(err: &GetLatestMobileSyncDocError) -> String {
    match err {
        GetLatestMobileSyncDocError::NotFound => {
            "No clipboard entry yet (404 — same response iPhone would see).".into()
        }
        GetLatestMobileSyncDocError::Port(inner) => {
            format!("Snapshot port failure: {inner}")
        }
    }
}

pub fn render_get_file_error(err: &GetMobileSyncFileError) -> String {
    match err {
        GetMobileSyncFileError::NotFound => "No matching file for this dataName (404).".into(),
        GetMobileSyncFileError::Port(inner) => {
            format!("Snapshot port failure: {inner}")
        }
        GetMobileSyncFileError::Staging(msg) => {
            // P5a.3.5: File 出站读 staging 文件 IO 失败(权限 / 中途盘错)。
            format!("File staging IO failure: {msg}")
        }
    }
}
