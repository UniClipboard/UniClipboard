use keyring::Entry;
use uc_core::ports::{SecureStorageError, SecureStoragePort};

const SERVICE_NAME: &str = "UniClipboard";

fn resolve_service_name() -> String {
    match std::env::var("UC_PROFILE") {
        Ok(profile) if !profile.is_empty() => format!("{SERVICE_NAME}-{profile}"),
        _ => SERVICE_NAME.to_string(),
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

#[cfg(test)]
mod tests {
    use super::resolve_service_name;
    use std::sync::Mutex;

    static UC_PROFILE_ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_uc_profile<T>(value: Option<&str>, f: impl FnOnce() -> T) -> T {
        let _guard = UC_PROFILE_ENV_LOCK
            .lock()
            .expect("lock UC_PROFILE test guard");
        let previous = std::env::var("UC_PROFILE").ok();

        match value {
            Some(profile) => std::env::set_var("UC_PROFILE", profile),
            None => std::env::remove_var("UC_PROFILE"),
        }

        let result = f();

        match previous {
            Some(profile) => std::env::set_var("UC_PROFILE", profile),
            None => std::env::remove_var("UC_PROFILE"),
        }

        result
    }

    #[test]
    fn service_name_defaults_without_profile() {
        let name = with_uc_profile(None, resolve_service_name);
        assert_eq!(name, "UniClipboard");
    }

    #[test]
    fn service_name_defaults_with_empty_profile() {
        let name = with_uc_profile(Some(""), resolve_service_name);
        assert_eq!(name, "UniClipboard");
    }

    #[test]
    fn service_name_isolated_by_profile() {
        let peer_a = with_uc_profile(Some("peerA"), resolve_service_name);
        let peer_b = with_uc_profile(Some("peerB"), resolve_service_name);

        assert_eq!(peer_a, "UniClipboard-peerA");
        assert_eq!(peer_b, "UniClipboard-peerB");
        assert_ne!(peer_a, peer_b);
    }
}
