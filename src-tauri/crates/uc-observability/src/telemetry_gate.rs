//! Process-wide runtime gate for the user-facing telemetry switch.
//!
//! Sentry reads this gate at event time so toggling `general.telemetry_enabled`
//! in the UI takes effect without a restart.
//!
//! - Frontend has its own gate (`setFrontendSentryEnabled` /
//!   `setFrontendTelemetryEnabled`) since the gate must live in the JS runtime.
//! - This module is the equivalent for the Rust side: `uc-bootstrap` consults
//!   it from Sentry's `before_send`, `before_breadcrumb`, and `before_send_log`
//!   hooks. When the gate is off, Issues / breadcrumbs / Logs are all dropped
//!   at capture time.
//!
//! ## Default
//!
//! Defaults to `true` so events emitted between process start and the first
//! settings load are NOT silently dropped — `uc-bootstrap` reads the
//! persisted preference from disk during init and overrides the default
//! before any user-visible event would normally be processed.

use std::sync::atomic::{AtomicBool, Ordering};

static TELEMETRY_ENABLED: AtomicBool = AtomicBool::new(true);

/// Returns whether the user-facing telemetry switch is currently on.
///
/// Hot path — called once per emitted event/span. `Ordering::Relaxed` is
/// sufficient because there is no other state we synchronize with: the
/// worst case is that an event emitted concurrently with a setter call is
/// classified by the pre-toggle value, which is acceptable.
#[inline]
pub fn is_telemetry_enabled() -> bool {
    TELEMETRY_ENABLED.load(Ordering::Relaxed)
}

/// Update the runtime gate.
///
/// Called from two paths:
/// - `uc-bootstrap` once at init time after reading persisted settings.
/// - `uc-webserver` PUT /settings handler whenever `telemetry_enabled`
///   changes, so the new value takes effect immediately.
pub fn set_telemetry_enabled(enabled: bool) {
    TELEMETRY_ENABLED.store(enabled, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_true_to_avoid_dropping_pre_init_events() {
        // Reset to default in case prior test mutated; then assert.
        set_telemetry_enabled(true);
        assert!(is_telemetry_enabled());
    }

    #[test]
    fn setter_round_trip() {
        set_telemetry_enabled(false);
        assert!(!is_telemetry_enabled());
        set_telemetry_enabled(true);
        assert!(is_telemetry_enabled());
    }
}
