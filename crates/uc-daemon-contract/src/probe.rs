//! Pure daemon health classification (ADR-008 P5-L L2).
//!
//! This module owns the *transport-free, process-free* decision logic that
//! turns a decoded [`HealthResponse`] into a [`ProbeOutcome`]: the
//! status/version/revision comparison and the semver "strictly newer" downgrade
//! guard. It is deliberately the canonical classifier shared by **every** client
//! that probes the daemon — the GUI host (`uc-desktop`) and the CLI (`uc-cli`) —
//! so both apply byte-identical compatibility rules.
//!
//! It lives in `uc-daemon-contract` (iroh/diesel-free) rather than
//! `uc-daemon-local` (which depends on `uc-application` → iroh/diesel) so that
//! `uc-cli` can depend on it *permanently* without welding the iroh/diesel edge
//! into the CLI build (which would block the P5-4 slimming goal).
//!
//! Only the PURE classification lives here. Process-control concerns — spawning
//! `uniclipd`, terminating a stale PID, bootstrap orchestration — stay in
//! `uc-daemon-local`/`uc-daemon-process`.

use semver::Version;

use crate::api::types::HealthResponse;
use crate::DAEMON_API_REVISION;

/// 一次健康探测的分类结果。
///
/// Lifted verbatim from `uc-daemon-local::contract` (ADR-008 P5-L L2). It is a
/// pure type — its only non-std field is [`HealthResponse`], which lives in this
/// crate — so it carries no process-control baggage. `uc-daemon-local` now
/// re-exports it from here for source-compat with existing
/// `uc_daemon_local::contract::ProbeOutcome` consumers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeOutcome {
    Absent,
    Compatible(HealthResponse),
    Incompatible {
        details: String,
        observed_package_version: Option<String>,
        observed_api_revision: Option<String>,
    },
}

/// 把 daemon 上报的健康响应分类成 [`ProbeOutcome`]。
///
/// `expected_package_version` 由调用方传入——典型情况是消费方 crate 自己的
/// `env!("CARGO_PKG_VERSION")`，因为 contract crate 的 cargo 版本号未必和
/// 调用方想校验的一致。
pub fn classify_health_response(
    health: HealthResponse,
    expected_package_version: &str,
) -> ProbeOutcome {
    let observed_package_version = Some(health.package_version.clone());
    let observed_api_revision = Some(health.api_revision.clone());

    if health.status != "ok" {
        return ProbeOutcome::Incompatible {
            details: format!("daemon reported unhealthy status {}", health.status),
            observed_package_version,
            observed_api_revision,
        };
    }

    if health.package_version.trim().is_empty() {
        return ProbeOutcome::Incompatible {
            details: "daemon health response missing packageVersion".to_string(),
            observed_package_version,
            observed_api_revision,
        };
    }

    if health.api_revision.trim().is_empty() {
        return ProbeOutcome::Incompatible {
            details: "daemon health response missing apiRevision".to_string(),
            observed_package_version,
            observed_api_revision,
        };
    }

    if health.package_version != expected_package_version {
        return ProbeOutcome::Incompatible {
            details: format!(
                "daemon packageVersion {} does not match shell packageVersion {}",
                health.package_version, expected_package_version
            ),
            observed_package_version,
            observed_api_revision,
        };
    }

    if health.api_revision != DAEMON_API_REVISION {
        return ProbeOutcome::Incompatible {
            details: format!(
                "daemon apiRevision {} does not match required {}",
                health.api_revision, DAEMON_API_REVISION
            ),
            observed_package_version,
            observed_api_revision,
        };
    }

    ProbeOutcome::Compatible(health)
}

/// Is the running daemon a *proven* strictly-newer version than this client?
///
/// ADR-008 P4-7 (OQ-downgrade-rollback): a lower-version client must never
/// terminate a higher-version incumbent daemon — that would silently downgrade
/// a running daemon to an older build. The GUI guards the one place that kills
/// an incompatible daemon with this; the CLI mirrors it to phrase its
/// incompatible-daemon error correctly (refusing a newer daemon vs. flagging an
/// older one).
///
/// Conservative by design: returns `true` **only** when both versions parse as
/// semver and `observed > expected`. A missing or unparseable observed version
/// (corruption, a foreign process on our port, a daemon that never reported a
/// version) is *not* proven-newer.
pub fn running_daemon_is_strictly_newer(observed: Option<&str>, expected: &str) -> bool {
    let (Some(observed), Ok(expected)) = (observed, Version::parse(expected.trim())) else {
        return false;
    };
    match Version::parse(observed.trim()) {
        Ok(observed) => observed > expected,
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::types::DaemonResidency;

    const TEST_PACKAGE_VERSION: &str = "0.6.0";

    fn ok_health() -> HealthResponse {
        HealthResponse {
            status: "ok".into(),
            package_version: TEST_PACKAGE_VERSION.into(),
            api_revision: DAEMON_API_REVISION.into(),
            residency: DaemonResidency::Standalone,
        }
    }

    // ------- classify_health_response: pure decision table -------

    #[test]
    fn classify_compatible_when_all_fields_match() {
        let outcome = classify_health_response(ok_health(), TEST_PACKAGE_VERSION);
        assert_eq!(outcome, ProbeOutcome::Compatible(ok_health()));
    }

    #[test]
    fn classify_incompatible_when_status_not_ok() {
        let mut health = ok_health();
        health.status = "degraded".into();
        let outcome = classify_health_response(health, TEST_PACKAGE_VERSION);
        match outcome {
            ProbeOutcome::Incompatible {
                details,
                observed_package_version,
                observed_api_revision,
            } => {
                assert!(details.contains("degraded"));
                assert_eq!(
                    observed_package_version.as_deref(),
                    Some(TEST_PACKAGE_VERSION)
                );
                assert_eq!(observed_api_revision.as_deref(), Some(DAEMON_API_REVISION));
            }
            other => panic!("expected Incompatible, got {other:?}"),
        }
    }

    #[test]
    fn classify_incompatible_when_package_version_empty() {
        let mut health = ok_health();
        health.package_version = "   ".into();
        let outcome = classify_health_response(health, TEST_PACKAGE_VERSION);
        match outcome {
            ProbeOutcome::Incompatible { details, .. } => {
                assert!(
                    details.contains("packageVersion"),
                    "details must point at the missing field, got: {details}"
                );
            }
            other => panic!("expected Incompatible for empty packageVersion, got {other:?}"),
        }
    }

    #[test]
    fn classify_incompatible_when_api_revision_empty() {
        let mut health = ok_health();
        health.api_revision = "".into();
        let outcome = classify_health_response(health, TEST_PACKAGE_VERSION);
        match outcome {
            ProbeOutcome::Incompatible { details, .. } => {
                assert!(
                    details.contains("apiRevision"),
                    "details must point at the missing field, got: {details}"
                );
            }
            other => panic!("expected Incompatible for empty apiRevision, got {other:?}"),
        }
    }

    #[test]
    fn classify_incompatible_when_package_version_mismatches_shell() {
        let mut health = ok_health();
        health.package_version = "0.5.99".into();
        let outcome = classify_health_response(health, TEST_PACKAGE_VERSION);
        match outcome {
            ProbeOutcome::Incompatible {
                details,
                observed_package_version,
                ..
            } => {
                assert_eq!(observed_package_version.as_deref(), Some("0.5.99"));
                assert!(
                    details.contains("0.5.99") && details.contains(TEST_PACKAGE_VERSION),
                    "details must surface both observed and expected versions: {details}"
                );
            }
            other => panic!("expected Incompatible for version mismatch, got {other:?}"),
        }
    }

    #[test]
    fn classify_incompatible_when_api_revision_mismatches_constant() {
        let mut health = ok_health();
        health.api_revision = "rev-from-the-future".into();
        let outcome = classify_health_response(health, TEST_PACKAGE_VERSION);
        match outcome {
            ProbeOutcome::Incompatible {
                details,
                observed_api_revision,
                ..
            } => {
                assert_eq!(
                    observed_api_revision.as_deref(),
                    Some("rev-from-the-future")
                );
                assert!(details.contains("rev-from-the-future"));
                assert!(details.contains(DAEMON_API_REVISION));
            }
            other => panic!("expected Incompatible for revision mismatch, got {other:?}"),
        }
    }

    // ------- running_daemon_is_strictly_newer: downgrade-rollback guard -------

    #[test]
    fn newer_daemon_is_protected_from_downgrade() {
        // The whole point: a lower client must recognise a higher daemon.
        assert!(running_daemon_is_strictly_newer(Some("0.15.0"), "0.14.0"));
        assert!(running_daemon_is_strictly_newer(Some("1.0.0"), "0.14.0"));
        // Pre-release ordering: a later alpha / a stable release both count as
        // newer than an earlier alpha.
        assert!(running_daemon_is_strictly_newer(
            Some("0.14.0-alpha.5"),
            "0.14.0-alpha.4"
        ));
        assert!(running_daemon_is_strictly_newer(
            Some("0.14.0"),
            "0.14.0-alpha.4"
        ));
    }

    #[test]
    fn older_or_equal_daemon_is_not_protected() {
        // Equal → sanctioned takeover path (not a downgrade).
        assert!(!running_daemon_is_strictly_newer(Some("0.14.0"), "0.14.0"));
        // Strictly older → the existing kill-and-replace behavior must stand.
        assert!(!running_daemon_is_strictly_newer(Some("0.13.0"), "0.14.0"));
        assert!(!running_daemon_is_strictly_newer(
            Some("0.14.0-alpha.3"),
            "0.14.0-alpha.4"
        ));
    }

    #[test]
    fn unprovable_versions_are_not_protected() {
        // Missing, blank, or unparseable observed versions are NOT proven-newer,
        // so they fall through to terminate-and-replace (foreign process on our
        // port, corrupted health payload, legacy daemon without a version).
        assert!(!running_daemon_is_strictly_newer(None, "0.14.0"));
        assert!(!running_daemon_is_strictly_newer(Some("   "), "0.14.0"));
        assert!(!running_daemon_is_strictly_newer(
            Some("not-a-version"),
            "0.14.0"
        ));
        // An unparseable *expected* version (should never happen for our own
        // CARGO_PKG_VERSION) also stays conservative: don't protect.
        assert!(!running_daemon_is_strictly_newer(Some("0.15.0"), "garbage"));
    }
}
