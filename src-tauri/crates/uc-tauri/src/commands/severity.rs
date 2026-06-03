//! Error-severity taxonomy for frontend-facing typed command errors.
//!
//! # Why this module exists
//!
//! Every `#[serde(tag = "code")]` error enum returned to the webview mixes two
//! operationally distinct classes of failure:
//!
//! - **User errors** ([`ErrorSeverity::UserError`]): expected outcomes driven by
//!   user input or local state — bad username, wrong passphrase, name already
//!   taken, target not trusted. These are normal product flow: the UI shows a
//!   message and the user retries. They MUST NOT raise Sentry alerts.
//! - **System errors** ([`ErrorSeverity::SystemError`]): unexpected failures —
//!   persistence, hashing, IO, an unwired facade. These are bugs or operational
//!   issues and SHOULD be reported to Sentry.
//!
//! Before this module existed the webview's IPC wrapper called
//! `Sentry.captureException` on *every* command rejection, so a user typing a
//! username that starts with a digit produced a
//! `TauriCommandError(USERNAME_MUST_START_WITH_LETTER)` Sentry issue — pure
//! noise drowning real alerts.
//!
//! # Single source of truth
//!
//! The classification lives here, in one auditable table per enum (see the
//! [`classify!`] invocations below). It is exported to the frontend by
//! `tests/specta_export.rs` as `USER_FACING_ERROR_CODES`, so the webview can
//! skip Sentry capture for user errors *without re-deriving the taxonomy*. The
//! Rust enums stay the source of truth; the TS set is a generated mirror gated
//! by the same schema-drift check as `ipc-bindings.generated.ts`.
//!
//! Adding a new error variant is caught two ways:
//! - the per-enum completeness tests below construct every variant via an
//!   exhaustive `match` (compile error on an unclassified variant) and assert
//!   the serialized `code` set equals the classification table; and
//! - the frontend default for an unknown `code` is "report" — so even a missed
//!   classification fails safe (extra noise, never a swallowed system error).

/// Operational severity of a frontend-facing command error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorSeverity {
    /// Expected, user/state-driven outcome. Handled in the UI; never alerted.
    UserError,
    /// Unexpected failure. Reported to Sentry.
    SystemError,
}

/// Implemented by every `#[serde(tag = "code")]` command error enum returned to
/// the webview. [`code_severities`](CommandErrorSeverity::code_severities) is
/// the authoritative `(serialized code, severity)` table consumed by both the
/// frontend code-set export and the completeness tests.
pub trait CommandErrorSeverity {
    fn code_severities() -> &'static [(&'static str, ErrorSeverity)];
}

/// Define a [`CommandErrorSeverity`] table for an enum from a single literal
/// list of `(serialized code, severity)` pairs. Keeping the list here — rather
/// than scattering `is_user_error()` arms across enum modules — makes the whole
/// taxonomy reviewable in one place.
macro_rules! classify {
    ($ty:ty { $( $code:literal => $sev:ident ),+ $(,)? }) => {
        impl CommandErrorSeverity for $ty {
            fn code_severities() -> &'static [(&'static str, ErrorSeverity)] {
                &[ $( ($code, ErrorSeverity::$sev) ),+ ]
            }
        }
    };
}

use crate::commands::error::CommandError;
use crate::commands::mobile_sync::MobileSyncError;

// Generic command taxonomy. `#[serde(tag = "code", content = "message")]` with
// no `rename_all`, so codes are the PascalCase variant names verbatim.
classify!(CommandError {
    "NotFound" => UserError,
    "InternalError" => SystemError,
    "Timeout" => SystemError,
    "Cancelled" => UserError,
    "ValidationError" => UserError,
    "Conflict" => UserError,
});

classify!(MobileSyncError {
    "FACADE_UNAVAILABLE" => SystemError,
    "LABEL_EMPTY" => UserError,
    "LABEL_TOO_LONG" => UserError,
    "LAN_LISTENER_DISABLED" => UserError,
    "USERNAME_TAKEN" => UserError,
    "USERNAME_TOO_SHORT" => UserError,
    "USERNAME_TOO_LONG" => UserError,
    "USERNAME_MUST_START_WITH_LETTER" => UserError,
    "USERNAME_CONTAINS_FORBIDDEN_CHARS" => UserError,
    "PASSWORD_TOO_SHORT" => UserError,
    "PASSWORD_TOO_LONG" => UserError,
    "PASSWORD_HASH_FAILED" => SystemError,
    "DEVICE_NOT_FOUND" => UserError,
    "INVALID_LAN_PARAMETER" => UserError,
    "SETTINGS_LOAD_FAILED" => SystemError,
    "SETTINGS_SAVE_FAILED" => SystemError,
    "ENDPOINT_INFO_FAILED" => SystemError,
    "LAN_PROBE_FAILED" => SystemError,
    "NO_LAN_INTERFACE_AVAILABLE" => SystemError,
    "PERSISTENCE_FAILED" => SystemError,
    "QR_RENDER_FAILED" => SystemError,
});

// NOTE (ADR-008 P3-1): the unlock / silent-unlock / factory-reset / resend
// command errors moved off Tauri commands onto the daemon loopback API, so their
// severity classifications were removed here. The daemon emits user-recoverable
// outcomes as 4xx (no Sentry escalation) and the FE error wrappers log them at
// info/warn — they no longer flow through `invokeWithTrace` / this taxonomy.

/// Every `(code, severity)` pair across all frontend-facing command errors.
fn all_code_severities() -> Vec<(&'static str, ErrorSeverity)> {
    let mut all = Vec::new();
    all.extend_from_slice(CommandError::code_severities());
    all.extend_from_slice(MobileSyncError::code_severities());
    all
}

/// The serialized `code` strings classified as [`ErrorSeverity::UserError`],
/// sorted and de-duplicated, for export to the frontend.
///
/// # Panics
///
/// Panics if the same `code` string is classified with conflicting severities
/// across enums. The frontend uses a single flat set keyed by `code`, so a code
/// that means "user" in one enum and "system" in another would be unsound. This
/// guard fails the export test (and CI) the instant that ambiguity is
/// introduced — forcing a rename or a richer keying scheme instead.
pub fn user_facing_error_codes() -> Vec<&'static str> {
    use std::collections::BTreeMap;

    let mut by_code: BTreeMap<&'static str, ErrorSeverity> = BTreeMap::new();
    for (code, severity) in all_code_severities() {
        if let Some(prev) = by_code.insert(code, severity) {
            assert_eq!(
                prev, severity,
                "error code {code:?} is classified with conflicting severities; \
                 a flat frontend code set cannot disambiguate it"
            );
        }
    }

    by_code
        .into_iter()
        .filter_map(|(code, severity)| (severity == ErrorSeverity::UserError).then_some(code))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;
    use std::collections::BTreeSet;

    /// Pull the serialized `code` discriminant out of a tagged error value.
    fn code_of<T: Serialize>(value: &T) -> String {
        serde_json::to_value(value)
            .expect("serialize")
            .get("code")
            .and_then(|c| c.as_str())
            .expect("tagged error must serialize a string `code`")
            .to_string()
    }

    /// Assert the classification table for an enum lists *exactly* the codes its
    /// variants serialize to — catching both typo'd literals and added/removed
    /// variants. Callers pass one constructed instance per variant.
    fn assert_table_matches<T: Serialize + CommandErrorSeverity>(samples: &[T]) {
        let actual: BTreeSet<String> = samples.iter().map(code_of).collect();
        let table: BTreeSet<String> = T::code_severities()
            .iter()
            .map(|(c, _)| (*c).to_string())
            .collect();
        assert_eq!(
            actual, table,
            "classification table is out of sync with the enum's serialized codes"
        );
    }

    #[test]
    fn command_error_table_is_complete() {
        use CommandError as E;
        // Exhaustive match: a new variant breaks compilation here, forcing an
        // update to both `samples` below and the `classify!` table above.
        fn _coverage(e: &E) {
            match e {
                E::NotFound(_)
                | E::InternalError(_)
                | E::Timeout(_)
                | E::Cancelled(_)
                | E::ValidationError(_)
                | E::Conflict(_) => {}
            }
        }
        let s = || "x".to_string();
        assert_table_matches(&[
            E::NotFound(s()),
            E::InternalError(s()),
            E::Timeout(s()),
            E::Cancelled(s()),
            E::ValidationError(s()),
            E::Conflict(s()),
        ]);
    }

    #[test]
    fn mobile_sync_error_table_is_complete() {
        use MobileSyncError as E;
        fn _coverage(e: &E) {
            match e {
                E::FacadeUnavailable
                | E::LabelEmpty
                | E::LabelTooLong { .. }
                | E::LanListenerDisabled
                | E::UsernameTaken { .. }
                | E::UsernameTooShort { .. }
                | E::UsernameTooLong { .. }
                | E::UsernameMustStartWithLetter
                | E::UsernameContainsForbiddenChars
                | E::PasswordTooShort { .. }
                | E::PasswordTooLong { .. }
                | E::PasswordHashFailed { .. }
                | E::DeviceNotFound { .. }
                | E::InvalidLanParameter { .. }
                | E::SettingsLoadFailed { .. }
                | E::SettingsSaveFailed { .. }
                | E::EndpointInfoFailed { .. }
                | E::LanProbeFailed { .. }
                | E::NoLanInterfaceAvailable
                | E::PersistenceFailed { .. }
                | E::QrRenderFailed { .. } => {}
            }
        }
        let m = || "x".to_string();
        assert_table_matches(&[
            E::FacadeUnavailable,
            E::LabelEmpty,
            E::LabelTooLong { max: 1 },
            E::LanListenerDisabled,
            E::UsernameTaken { username: m() },
            E::UsernameTooShort { min: 1, got: 0 },
            E::UsernameTooLong { max: 1, got: 2 },
            E::UsernameMustStartWithLetter,
            E::UsernameContainsForbiddenChars,
            E::PasswordTooShort { min: 1 },
            E::PasswordTooLong { max: 1 },
            E::PasswordHashFailed { message: m() },
            E::DeviceNotFound { device_id: m() },
            E::InvalidLanParameter { reason: m() },
            E::SettingsLoadFailed { message: m() },
            E::SettingsSaveFailed { message: m() },
            E::EndpointInfoFailed { message: m() },
            E::LanProbeFailed { message: m() },
            E::NoLanInterfaceAvailable,
            E::PersistenceFailed { message: m() },
            E::QrRenderFailed { message: m() },
        ]);
    }

    #[test]
    fn user_facing_codes_are_sorted_unique_and_user_only() {
        let codes = user_facing_error_codes();

        let mut sorted = codes.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(codes, sorted, "user-facing codes must be sorted and unique");

        // Spot-check the case that motivated this module, and that a system
        // error never leaks into the user set.
        assert!(codes.contains(&"USERNAME_MUST_START_WITH_LETTER"));
        assert!(!codes.contains(&"PERSISTENCE_FAILED"));
    }
}
