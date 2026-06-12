use thiserror::Error;

/// Secure storage errors.
///
/// 安全存储错误类型。
#[derive(Debug, Error)]
pub enum SecureStorageError {
    /// Secure storage is unavailable on this platform.
    ///
    /// 平台不支持或不可用。
    #[error("secure storage unavailable: {0}")]
    Unavailable(String),

    /// Access was denied by the platform (permissions/ACL).
    ///
    /// 平台权限或 ACL 拒绝访问。
    #[error("secure storage access denied: {0}")]
    PermissionDenied(String),

    /// Stored data is corrupt or invalid.
    ///
    /// 存储数据损坏或无效。
    #[error("secure storage data corrupt: {0}")]
    Corrupt(String),

    /// Other storage failures.
    ///
    /// 其它存储失败。
    #[error("secure storage failed: {0}")]
    Other(String),
}

/// Secure storage port for key-value secrets.
///
/// 安全存储端口：用于存取敏感字节数据。
pub trait SecureStoragePort: Send + Sync {
    /// Get a value by key.
    ///
    /// 按 key 读取数据。
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, SecureStorageError>;

    /// Set a value by key.
    ///
    /// 按 key 写入数据。
    fn set(&self, key: &str, value: &[u8]) -> Result<(), SecureStorageError>;

    /// Delete a value by key.
    ///
    /// 按 key 删除数据。
    fn delete(&self, key: &str) -> Result<(), SecureStorageError>;
}
