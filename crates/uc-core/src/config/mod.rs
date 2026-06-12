//! # Pure Data Module / 纯数据模块 - Data Transfer Objects Only
//!
//! ## Responsibilities / 职责
//!
//! - ✅ Define configuration data structures / 定义配置数据结构
//! - ✅ Provide TOML → DTO mapping / 提供 TOML → DTO 的映射
//!
//! ## Prohibited / 禁止事项
//!
//! ❌ **No business logic or policies / 禁止任何业务逻辑或策略**
//! ❌ **No validation logic / 禁止验证逻辑**
//! ❌ **No default value calculation / 禁止默认值计算**
//!
//! ## Iron Rule / 铁律
//!
//! > **This module contains data only, no policy, no validation.**
//! > **此模块只包含数据结构定义，禁止：任何业务逻辑或策略、验证逻辑、默认值计算。**

use std::path::PathBuf;

/// Maximum allowed plaintext size for clipboard transfer payloads (128 MiB).
pub const RECEIVE_PLAINTEXT_CAP: usize = 128 * 1024 * 1024;

/// Application configuration DTO (pure data, no logic)
/// 应用配置 DTO（纯数据，无逻辑）
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// Device name (may be empty - this is a fact, not an error)
    /// 设备名称（可能为空 - 这就是事实，不是错误）
    pub device_name: String,

    /// Vault key file path (path info only, no existence check)
    /// Vault 密钥文件路径（仅路径信息，不检查文件是否存在）
    pub vault_key_path: PathBuf,

    /// Vault snapshot file path (path info only, no existence check)
    /// Vault snapshot 文件路径（仅路径信息，不检查文件是否存在）
    pub vault_snapshot_path: PathBuf,

    /// Web server port
    pub webserver_port: u16,

    /// Database path
    pub database_path: PathBuf,

    /// Silent start flag
    pub silent_start: bool,
}

impl AppConfig {
    /// Create AppConfig from TOML value
    /// 从 TOML 值创建 AppConfig
    ///
    /// **Prohibited / 禁止**: This method must NOT contain any validation
    /// or default value logic. Empty strings are valid "facts".
    /// 此方法必须不包含任何验证或默认值逻辑。空字符串是合法的"事实"。
    pub fn from_toml(toml_value: &toml::Value) -> anyhow::Result<Self> {
        Ok(Self {
            device_name: toml_value
                .get("general")
                .and_then(|g| g.get("device_name"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            vault_key_path: PathBuf::from(
                toml_value
                    .get("security")
                    .and_then(|s| s.get("vault_key_path"))
                    .and_then(|v| v.as_str())
                    .unwrap_or(""),
            ),
            vault_snapshot_path: PathBuf::from(
                toml_value
                    .get("security")
                    .and_then(|s| s.get("vault_snapshot_path"))
                    .and_then(|v| v.as_str())
                    .unwrap_or(""),
            ),
            webserver_port: toml_value
                .get("network")
                .and_then(|n| n.get("webserver_port"))
                .and_then(|v| v.as_integer())
                .unwrap_or(0) as u16,
            database_path: PathBuf::from(
                toml_value
                    .get("storage")
                    .and_then(|s| s.get("database_path"))
                    .and_then(|v| v.as_str())
                    .unwrap_or(""),
            ),
            silent_start: toml_value
                .get("general")
                .and_then(|g| g.get("silent_start"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        })
    }

    /// Create empty AppConfig (all empty/default values)
    /// 创建空的 AppConfig（所有字段为空/默认值）
    ///
    /// **Note**: This is a pure data constructor with "empty" as valid facts.
    /// 注意：这是一个纯数据构造函数，"空"是合法的事实。
    pub fn empty() -> Self {
        Self {
            device_name: String::new(),
            vault_key_path: PathBuf::new(),
            vault_snapshot_path: PathBuf::new(),
            webserver_port: 0,
            database_path: PathBuf::new(),
            silent_start: false,
        }
    }

    /// Create AppConfig with system-default paths for production use
    /// 生产环境使用：创建具有系统默认路径的 AppConfig
    ///
    /// **Note**: This is a pure data constructor that builds paths from a provided base directory.
    /// The base directory should be computed by the caller using platform-specific logic (e.g., `dirs` crate).
    /// 注意：这是一个纯数据构造函数，从提供的基础目录构建路径。
    /// 基础目录应由调用方使用平台特定逻辑（如 `dirs` crate）计算。
    ///
    /// # Arguments / 参数
    ///
    /// * `data_dir` - Base directory for app data (e.g., `~/Library/Application Support/uniclipboard`)
    ///                应用数据的基础目录（例如 `~/Library/Application Support/uniclipboard`）
    pub fn with_system_defaults(data_dir: PathBuf) -> Self {
        Self {
            device_name: String::new(),
            vault_key_path: data_dir.join("vault/key"),
            vault_snapshot_path: data_dir.join("vault/snapshot"),
            webserver_port: 0,
            database_path: data_dir.join("uniclipboard.db"),
            silent_start: false,
        }
    }
}
