//! Spawn contract between CLI and daemon binary.
//!
//! The CLI `start [--server]` sets these environment variables before
//! detached-spawning the daemon binary. The daemon reads them at startup
//! to resolve its run mode.

/// Environment variable carrying the daemon run mode from the CLI spawner
/// to the daemon binary.
pub const RUN_MODE_ENV: &str = "UC_DAEMON_RUN_MODE";

/// Value of [`RUN_MODE_ENV`] that selects headless server mode.
pub const RUN_MODE_SERVER: &str = "server";

// ── D9 unlock contract (ADR-008) ──────────────────────────────────────

/// Environment variable a **strict-unattended** launcher sets on the daemon.
///
/// Set by the autostart / service-manager unit (ADR-008 D10, P4-4) and any
/// other launcher where *no GUI will ever come* to unlock the session. Its
/// presence makes the daemon enforce the D9 contract via
/// [`validate_unattended_unlock`]: keyring auto-unlock must be available.
///
/// Absent for GUI spawn (attended) and interactive `uniclip start` (lenient
/// unattended — the user is at a terminal and can `uniclip unlock`); those keep
/// their current force-unlock behavior.
pub const UNATTENDED_ENV: &str = "UC_DAEMON_UNATTENDED";

/// Whether the current process was launched with strict-unattended intent
/// (see [`UNATTENDED_ENV`]).
pub fn unattended_from_env() -> bool {
    std::env::var(UNATTENDED_ENV).as_deref() == Ok("1")
}

/// Violation of the D9 unlock contract: a strict-unattended daemon configured
/// with `auto_unlock_enabled = false`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnlockContractViolation;

impl std::fmt::Display for UnlockContractViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "unattended autostart requires keyring auto-unlock, but \
             auto_unlock_enabled is false — a daemon with no GUI fallback \
             cannot self-unlock. Enable auto-unlock or disable unattended \
             autostart (ADR-008 D9)."
        )
    }
}

impl std::error::Error for UnlockContractViolation {}

/// D9 mutual exclusion — the single source of truth.
///
/// A strict-unattended daemon has no GUI to fall back on, so it cannot honor
/// `auto_unlock_enabled = false`: nobody would ever unlock it. This rejects
/// that contradictory configuration. The daemon startup self-check fail-fasts
/// on `Err`; GUI settings / CLI pre-checks (P4-4) surface it as a friendly
/// error before the bad config is ever written.
pub fn validate_unattended_unlock(
    unattended: bool,
    auto_unlock_enabled: bool,
) -> Result<(), UnlockContractViolation> {
    if unattended && !auto_unlock_enabled {
        Err(UnlockContractViolation)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validator_rejects_only_unattended_without_auto_unlock() {
        assert!(validate_unattended_unlock(true, false).is_err());
        // The other three combinations are all permitted.
        assert!(validate_unattended_unlock(true, true).is_ok());
        assert!(validate_unattended_unlock(false, false).is_ok());
        assert!(validate_unattended_unlock(false, true).is_ok());
    }

    #[test]
    fn unattended_env_requires_exact_one() {
        static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _env = ENV_LOCK.lock().unwrap();

        std::env::set_var(UNATTENDED_ENV, "1");
        assert!(unattended_from_env());
        std::env::set_var(UNATTENDED_ENV, "true");
        assert!(!unattended_from_env(), "only \"1\" is truthy");
        std::env::remove_var(UNATTENDED_ENV);
        assert!(!unattended_from_env());
    }
}
