use keyring::Entry;

use crate::ports::{SecureStorageError, SecureStorageProvider};

const SERVICE_NAME: &str = "UniClipboard";

/// True when `msg` is the deterministic Secret Service rejection of a binary write,
/// caused by the `secret-service` crate hardcoding `content_type: "text/plain"` on
/// every write with no way to request `application/octet-stream`. A backend that
/// strictly validates UTF-8 for text-declared content rejects the write outright
/// with wording to this effect (observed: "Secret value contains invalid UTF-8
/// sequences but content_type declares text encoding; use application/octet-stream
/// for binary data"). This is distinct from KWallet's issue #838 (write succeeds,
/// read-back silently mangled) — here the write never lands at all, and it fails
/// identically on every retry because the payload (random bytes) is essentially
/// never valid UTF-8.
///
/// Matched by substrings rather than one fixed string so it still catches minor
/// wording differences across Secret Service backend implementations, while
/// staying specific enough not to misclassify a genuine outage.
pub(crate) fn is_binary_content_rejected(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    lower.contains("invalid utf-8")
        && (lower.contains("content_type") || lower.contains("content type"))
        && (lower.contains("text encoding") || lower.contains("text/plain"))
}

/// Classify a `keyring::Error::PlatformFailure` into a domain `SecureStorageError`.
///
/// Linux backends surface D-Bus / Secret Service transport faults as `PlatformFailure(msg)`
/// with the underlying error text. These should map to `Unavailable` (service crashed, no
/// owner, activation failed, connection lost) rather than `PermissionDenied`, which is
/// reserved for genuine ACL / prompt-dismissed outcomes.
///
/// The binary-content-type rejection (see `is_binary_content_rejected`) is deliberately
/// kept out of `Unavailable`/`PermissionDenied`: it is neither a transport outage nor an
/// ACL refusal, but a structural incompatibility between this backend and binary secrets.
/// It still surfaces as `Other`, but with wording that names the real cause instead of
/// the generic "platform failure" prefix, so logs and the probe-level fallback decision
/// (see `secure_storage::probed`) can recognize it without re-deriving the classification.
fn classify_platform_failure(msg: &str) -> SecureStorageError {
    if is_binary_content_rejected(msg) {
        return SecureStorageError::Other(format!(
            "secret service rejected a binary write because it declares content_type \
             text/plain and strictly validates UTF-8 (deterministic, not a transient \
             outage): {msg}"
        ));
    }
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
/// The returned name is `SERVICE_NAME` when no environment-derived suffix is present;
/// otherwise the resolved profile is appended with a hyphen (for example:
/// `UniClipboard-peer-a`).
///
/// The resolved profile is the namespace authority. `UNICLIPBOARD_ENV` only preserves
/// the legacy `UniClipboard-dev` isolation for an unprofiled development process; it
/// must not add a second suffix when a profile already exists.
fn resolve_service_name() -> String {
    let development_environment = matches!(
        std::env::var("UNICLIPBOARD_ENV"),
        Ok(value) if value.eq_ignore_ascii_case("development") || value.eq_ignore_ascii_case("dev")
    );

    service_name_for_context(crate::resolve_profile().as_deref(), development_environment)
}

fn service_name_for_context(profile: Option<&str>, development_environment: bool) -> String {
    match profile {
        Some(profile) => format!("{SERVICE_NAME}-{profile}"),
        None if development_environment => format!("{SERVICE_NAME}-dev"),
        None => SERVICE_NAME.to_string(),
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

    #[test]
    fn resolved_profile_is_the_only_secure_storage_namespace() {
        assert_eq!(
            service_name_for_context(Some("dev"), true),
            "UniClipboard-dev"
        );
        assert_eq!(
            service_name_for_context(Some("peer-a"), true),
            "UniClipboard-peer-a"
        );
    }

    #[test]
    fn development_environment_still_isolates_an_unprofiled_process() {
        assert_eq!(service_name_for_context(None, true), "UniClipboard-dev");
        assert_eq!(service_name_for_context(None, false), "UniClipboard");
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

    /// The exact wording observed in t-0155's diagnostic log (12/12 identical failures).
    const BINARY_CONTENT_REJECTION_TEXT: &str = "DBus error: Secret value contains invalid \
        UTF-8 sequences but content_type declares text encoding; use \
        application/octet-stream for binary data";

    #[test]
    fn binary_content_rejection_is_detected() {
        assert!(is_binary_content_rejected(BINARY_CONTENT_REJECTION_TEXT));
        // Must not be swept into a generic Unavailable/PermissionDenied bucket.
        assert!(matches!(
            classify_platform_failure(BINARY_CONTENT_REJECTION_TEXT),
            SecureStorageError::Other(_)
        ));
        match classify_platform_failure(BINARY_CONTENT_REJECTION_TEXT) {
            SecureStorageError::Other(msg) => {
                assert!(
                    msg.contains("deterministic") && msg.contains("content_type"),
                    "classification must name the real cause, got: {msg}"
                );
            }
            other => panic!("expected Other, got {other:?}"),
        }
    }

    #[test]
    fn genuine_unavailability_is_not_mistaken_for_binary_content_rejection() {
        let genuinely_unavailable = [
            "DBus error: Remote peer disconnected",
            "org.freedesktop.DBus.Error.ServiceUnknown: The name is not activatable",
            "org.freedesktop.DBus.Error.NameHasNoOwner",
            "system secret service did not respond within 3s; treating as unavailable",
            "secret service did not preserve binary payload (wrote 32 bytes, read 65 bytes back)",
        ];
        for msg in genuinely_unavailable {
            assert!(
                !is_binary_content_rejected(msg),
                "must not misclassify a genuine outage as binary-content rejection: {msg}"
            );
        }
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
