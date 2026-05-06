//! `mobile_sync` 子命令共享的小工具:错误渲染、重启提示、JSON 包装。

use uc_application::facade::{
    ApplyIncomingMobileClipError, GetLatestMobileSyncDocError, GetMobileSyncFileError,
    GetMobileSyncSettingsError, ListMobileDevicesError, MobileSyncListLanInterfacesError,
    RegisterMobileShortcutDeviceError, RevokeMobileDeviceError, UpdateMobileSyncSettingsError,
};

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
            "LAN listener is not enabled — run `uniclip mobile-sync lan enable --bind <IP>` first."
                .into()
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
