use keyring::Entry;
use uc_core::ports::{SecureStorageError, SecureStoragePort};

const SERVICE_NAME: &str = "UniClipboard";

/// Classify a `keyring::Error::PlatformFailure` into a domain `SecureStorageError`.
///
/// Linux backends surface D-Bus / Secret Service transport faults as `PlatformFailure(msg)`
/// with the underlying error text. These should map to `Unavailable` (service crashed, no
/// owner, activation failed, connection lost) rather than `PermissionDenied`, which is
/// reserved for genuine ACL / prompt-dismissed outcomes.
fn classify_platform_failure(msg: &str) -> SecureStorageError {
    let lower = msg.to_ascii_lowercase();
    let unavailable_markers = [
        "remote peer disconnected",
        "connection reset",
        "broken pipe",
        "no such file or directory",
        "no such interface",
        "no such object",
        "serviceunknown",
        "service_unknown",
        "namehasnoowner",
        "name_has_no_owner",
        "activationfailed",
        "activation_failed",
        "nameowner",
        "disconnected",
        "no reply",
        "noreply",
        "timed out",
        "timeout",
    ];
    let denied_markers = [
        "prompt dismissed",
        "promptdismissed",
        "access denied",
        "accessdenied",
        "access_denied",
        "permission denied",
        "permissiondenied",
        "not authorized",
        "notauthorized",
    ];
    if unavailable_markers.iter().any(|m| lower.contains(m)) {
        SecureStorageError::Unavailable(msg.to_string())
    } else if denied_markers.iter().any(|m| lower.contains(m)) {
        SecureStorageError::PermissionDenied(msg.to_string())
    } else {
        SecureStorageError::Other(format!("platform failure: {msg}"))
    }
}

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
                Err(classify_platform_failure(&msg.to_string()))
            }
            Err(err) => Err(SecureStorageError::Other(format!(
                "failed to read secure storage: {err}"
            ))),
        }
    }

    fn set(&self, key: &str, value: &[u8]) -> Result<(), SecureStorageError> {
        let entry = self.entry_for_key(key)?;
        entry.set_secret(value).map_err(|err| match err {
            keyring::Error::PlatformFailure(msg) => classify_platform_failure(&msg.to_string()),
            _ => SecureStorageError::Other(format!("failed to write secure storage: {err}")),
        })
    }

    fn delete(&self, key: &str) -> Result<(), SecureStorageError> {
        let entry = self.entry_for_key(key)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(keyring::Error::PlatformFailure(msg)) => {
                Err(classify_platform_failure(&msg.to_string()))
            }
            Err(err) => Err(SecureStorageError::Other(format!(
                "failed to delete secure storage: {err}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_classification() {
        assert!(matches!(
            classify_platform_failure("DBus error: Remote peer disconnected"),
            SecureStorageError::Unavailable(_)
        ));
        assert!(matches!(
            classify_platform_failure("org.freedesktop.DBus.Error.ServiceUnknown: ..."),
            SecureStorageError::Unavailable(_)
        ));
        assert!(matches!(
            classify_platform_failure("org.freedesktop.DBus.Error.NameHasNoOwner"),
            SecureStorageError::Unavailable(_)
        ));
    }

    #[test]
    fn denied_classification() {
        assert!(matches!(
            classify_platform_failure("Prompt dismissed by user"),
            SecureStorageError::PermissionDenied(_)
        ));
        assert!(matches!(
            classify_platform_failure("AccessDenied"),
            SecureStorageError::PermissionDenied(_)
        ));
    }

    #[test]
    fn unknown_classification_falls_through_to_other() {
        match classify_platform_failure("something totally weird") {
            SecureStorageError::Other(msg) => assert!(msg.contains("platform failure")),
            _ => panic!("expected Other"),
        }
    }
}
