use keyring::Entry;
use uc_core::ports::{SecureStorageError, SecureStoragePort};

const SERVICE_NAME: &str = "UniClipboard";

fn resolve_service_name() -> String {
    let mut suffixes: Vec<String> = Vec::new();

    if matches!(
        std::env::var("UNICLIPBOARD_ENV"),
        Ok(value) if value.eq_ignore_ascii_case("development") || value.eq_ignore_ascii_case("dev")
    ) {
        suffixes.push("dev".to_string());
    }

    let runtime_profile = std::env::var("UC_PROFILE").ok().filter(|s| !s.is_empty());
    let profile = runtime_profile.or_else(|| crate::default_profile().map(str::to_string));
    if let Some(profile) = profile {
        suffixes.push(profile);
    }

    if suffixes.is_empty() {
        SERVICE_NAME.to_string()
    } else {
        format!("{SERVICE_NAME}-{}", suffixes.join("-"))
    }
}

/// System keychain-backed secure storage.
///
/// 基于系统钥匙串的安全存储实现。
#[derive(Debug, Clone)]
pub struct SystemSecureStorage {
    service_name: String,
}

impl Default for SystemSecureStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemSecureStorage {
    /// Create a system secure storage instance.
    ///
    /// 创建系统安全存储实例。
    pub fn new() -> Self {
        Self {
            service_name: resolve_service_name(),
        }
    }

    fn entry_for_key(&self, key: &str) -> Result<Entry, SecureStorageError> {
        Entry::new(&self.service_name, key)
            .map_err(|e| SecureStorageError::Other(format!("failed to create keyring entry: {e}")))
    }
}

impl SecureStoragePort for SystemSecureStorage {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
        let entry = self.entry_for_key(key)?;
        match entry.get_secret() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(keyring::Error::PlatformFailure(msg)) => {
                Err(SecureStorageError::PermissionDenied(msg.to_string()))
            }
            Err(err) => Err(SecureStorageError::Other(format!(
                "failed to read secure storage: {err}"
            ))),
        }
    }

    fn set(&self, key: &str, value: &[u8]) -> Result<(), SecureStorageError> {
        let entry = self.entry_for_key(key)?;
        entry.set_secret(value).map_err(|err| match err {
            keyring::Error::PlatformFailure(msg) => {
                SecureStorageError::PermissionDenied(msg.to_string())
            }
            _ => SecureStorageError::Other(format!("failed to write secure storage: {err}")),
        })
    }

    fn delete(&self, key: &str) -> Result<(), SecureStorageError> {
        let entry = self.entry_for_key(key)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(keyring::Error::PlatformFailure(msg)) => {
                Err(SecureStorageError::PermissionDenied(msg.to_string()))
            }
            Err(err) => Err(SecureStorageError::Other(format!(
                "failed to delete secure storage: {err}"
            ))),
        }
    }
}
