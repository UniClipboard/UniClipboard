use keyring::Entry;

use crate::ports::{SecureStorageError, SecureStorageProvider};

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
fn resolve_service_name() -> String {
    let profile = crate::resolve_profile();
    resolve_service_name_with_profile(profile.as_deref())
}

fn resolve_service_name_for_explicit_profile(profile: &str) -> String {
    resolve_service_name_with_profile(Some(profile))
}

fn resolve_service_name_with_profile(profile: Option<&str>) -> String {
    let mut suffixes: Vec<String> = Vec::new();

    if matches!(
        std::env::var("UNICLIPBOARD_ENV"),
        Ok(value) if value.eq_ignore_ascii_case("development") || value.eq_ignore_ascii_case("dev")
    ) {
        suffixes.push("dev".to_string());
    }

    if let Some(profile) = profile {
        suffixes.push(profile.to_string());
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

    /// Create a system secure storage instance for an explicit profile.
    ///
    /// This constructor never reads `UC_PROFILE`.
    pub fn for_profile(profile: &str) -> Self {
        Self {
            service_name: resolve_service_name_for_explicit_profile(profile),
        }
    }

    fn entry_for_key(&self, key: &str) -> Result<Entry, SecureStorageError> {
        Entry::new(&self.service_name, key)
            .map_err(|e| SecureStorageError::Other(format!("failed to create keyring entry: {e}")))
    }
}

impl SecureStorageProvider for SystemSecureStorage {
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

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct ScopedEnv {
        values: Vec<(&'static str, Option<String>)>,
    }

    impl ScopedEnv {
        fn apply(changes: &[(&'static str, Option<&'static str>)]) -> Self {
            let values = changes
                .iter()
                .map(|(name, value)| {
                    let previous = std::env::var(name).ok();
                    match value {
                        Some(value) => std::env::set_var(name, value),
                        None => std::env::remove_var(name),
                    }
                    (*name, previous)
                })
                .collect();
            Self { values }
        }
    }

    impl Drop for ScopedEnv {
        fn drop(&mut self) {
            for (name, value) in self.values.drain(..).rev() {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }

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

    #[test]
    fn explicit_profile_service_name_does_not_consult_ambient_profile() {
        let _lock = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _env = ScopedEnv::apply(&[
            ("UC_PROFILE", Some("ambient-must-not-leak")),
            ("UNICLIPBOARD_ENV", None),
        ]);

        assert_eq!(
            SystemSecureStorage::for_profile("019d-profile-a").service_name,
            "UniClipboard-019d-profile-a"
        );
        assert_eq!(
            SystemSecureStorage::for_profile("019d-profile-b").service_name,
            "UniClipboard-019d-profile-b"
        );
    }

    #[test]
    fn default_service_name_preserves_ambient_and_development_resolution() {
        let _lock = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _env = ScopedEnv::apply(&[("UC_PROFILE", None), ("UNICLIPBOARD_ENV", None)]);

        let no_ambient = if cfg!(feature = "dev-profile") {
            "UniClipboard-dev"
        } else {
            "UniClipboard"
        };
        assert_eq!(SystemSecureStorage::new().service_name, no_ambient);

        std::env::set_var("UC_PROFILE", "ambient-profile");
        assert_eq!(
            SystemSecureStorage::new().service_name,
            "UniClipboard-ambient-profile"
        );

        std::env::set_var("UNICLIPBOARD_ENV", "development");
        assert_eq!(
            SystemSecureStorage::new().service_name,
            "UniClipboard-dev-ambient-profile"
        );

        std::env::remove_var("UC_PROFILE");
        let development_without_ambient = if cfg!(feature = "dev-profile") {
            "UniClipboard-dev-dev"
        } else {
            "UniClipboard-dev"
        };
        assert_eq!(
            SystemSecureStorage::new().service_name,
            development_without_ambient
        );
    }
}
