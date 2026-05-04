//! # uc-platform
//!
//! Platform-specific implementations for UniClipboard.
//!
//! This crate contains infrastructure implementations that interact with
//! the operating system, external services, and hardware.

// Tracing support for platform layer instrumentation
pub use tracing;

/// 编译期默认 profile。
///
/// 启用 `dev-profile` feature 时返回 `Some("dev")`，否则返回 `None`。
/// 仅作为 `UC_PROFILE` 环境变量未设置时的回退；运行时变量始终优先。
///
/// 用于在 dev 构建产物中默认隔离数据目录与系统钥匙串，避免与 prod 安装互相覆盖。
#[inline]
pub const fn default_profile() -> Option<&'static str> {
    #[cfg(feature = "dev-profile")]
    {
        Some("dev")
    }
    #[cfg(not(feature = "dev-profile"))]
    {
        None
    }
}

pub mod app_dirs;
pub mod bootstrap;
pub mod capability;
pub mod clipboard;
pub mod file_secure_storage;
pub mod ports;
pub mod secure_storage;
pub mod system_secure_storage;
pub mod usecases;
