use keyring::Entry;
use uc_core::ports::{SecureStorageError, SecureStoragePort};

const SERVICE_NAME: &str = "UniClipboard";

/// Builds the keychain service name used to namespace secure storage entries.
///
/// The returned name is `SERVICE_NAME` when no environment-derived suffixes are present;
/// otherwise the suffixes are appended with hyphens (for example: `UniClipboard-dev-profile`).
///
/// The function appends the `"dev"` suffix when `UNICLIPBOARD_ENV` is set to `"development"` or `"dev"` (case-insensitive).
/// It also appends a profile suffix taken from `UC_PROFILE` if non-empty, or from `crate::default_profile()` if `UC_PROFILE` is unset or empty.
///
/// # Examples
///
/// ```
/// // ensure a deterministic environment for the example
/// std::env::set_var("UNICLIPBOARD_ENV", "development");
/// std::env::set_var("UC_PROFILE", "staging");
/// assert_eq!(resolve_service_name(), format!("{}-dev-staging", SERVICE_NAME));
/// std::env::remove_var("UNICLIPBOARD_ENV");
/// std::env::remove_var("UC_PROFILE");
/// ```
fn resolve_service_name() -> String {
    let mut suffixes: Vec<String> = Vec::new();

    if matches!(
        std::env::var("UNICLIPBOARD_ENV"),
        Ok(value) if value.eq_ignore_ascii_case("development") || value.eq_ignore_ascii_case("dev")
    ) {
        suffixes.push("dev".to_string());
    }

    if let Some(profile) = crate::resolve_profile() {
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
