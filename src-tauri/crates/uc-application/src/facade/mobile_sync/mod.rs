//! `MobileSyncFacade` —— 移动端同步功能（v1：iOS Shortcut）的应用层入口。
//!
//! 按 `uc-application/AGENTS.md` §11.4，外部 crate（bootstrap / daemon /
//! tauri / cli）只能通过本目录下的 [`MobileSyncFacade`] 访问移动端同步能
//! 力；所有底层 `*UseCase`、内部 service trait、域 ports 均保持
//! `pub(crate)` / 通过 facade 间接暴露，外部不直接持有。
//!
//! 详细的方法清单与设计取舍见 [`facade`] 子模块文档。

mod facade;

pub use facade::{
    GetMobileSyncSettingsError, LanInterfaceOption, ListLanInterfacesError, ListMobileDevicesError,
    MobileDeviceSummary, MobileSyncFacade, MobileSyncFacadeDeps, MobileSyncSettingsView,
    RegisterMobileShortcutDeviceError, RegisterMobileShortcutDeviceInput,
    RegisterMobileShortcutDeviceOutput, RevokeMobileDeviceError, RevokeMobileDeviceInput,
    ShortcutInstallMethod, ShortcutInstallMethodOption, UpdateMobileSyncSettingsError,
    UpdateMobileSyncSettingsInput, UpdateMobileSyncSettingsOutput,
};
