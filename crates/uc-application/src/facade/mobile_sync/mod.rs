//! `MobileSyncFacade` —— 移动端同步功能(v1: iOS SyncClipboard Clipboard EX)
//! 的应用层入口。
//!
//! 按 `uc-application/AGENTS.md` §11.4, 外部 crate(bootstrap / daemon /
//! tauri / cli)只能通过本目录下的 [`MobileSyncFacade`] 访问移动端同步能
//! 力;所有底层 `*UseCase`、内部 service trait、域 ports 均保持
//! `pub(crate)` / 通过 facade 间接暴露, 外部不直接持有。
//!
//! 详细的方法清单与设计取舍见 [`facade`] 子模块文档。

mod facade;
mod outbound_adapter;
mod restore_adapter;

pub use facade::streaming_scope_nonce as mobile_sync_streaming_scope_nonce;
pub use facade::{
    ApplyIncomingMobileClipError, ApplyIncomingMobileClipInput, ApplyIncomingMobileClipOutcome,
    AuthenticateBasicAuthError, AuthenticateBasicAuthInput, AuthenticatedDevice,
    GetLatestMobileSyncDocError, GetMobileSyncFileError, GetMobileSyncFileOutput,
    GetMobileSyncSettingsError, IncomingMobileBuffer, IncomingMobileClipEvent, LanInterfaceOption,
    ListLanInterfacesError, ListMobileDevicesError, MobileDeviceSummary, MobileSyncFacade,
    MobileSyncFacadeDeps, MobileSyncSettingsView, MobileSyncSnapshotPorts,
    RegisterMobileShortcutDeviceError, RegisterMobileShortcutDeviceInput,
    RegisterMobileShortcutDeviceOutput, RevokeMobileDeviceError, RevokeMobileDeviceInput,
    RotateMobilePasswordError, RotateMobilePasswordInput, RotateMobilePasswordOutput,
    ShortcutInstallMethod, ShortcutInstallMethodOption, SyncClipboardItemType, SyncClipboardMeta,
    UpdateMobileSyncSettingsError, UpdateMobileSyncSettingsInput, UpdateMobileSyncSettingsOutput,
    SYNC_CLIPBOARD_EX_INSTALL_URL,
};
