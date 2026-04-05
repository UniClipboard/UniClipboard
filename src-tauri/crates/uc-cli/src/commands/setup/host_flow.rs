//! Host CLI flow -- HostCliPhase, HostCliSession, and run_pair.
//!
//! Phase-driven loop structure per D-16 through D-19:
//!   loop { poll -> parse -> derive phase -> match -> sleep }

use std::time::Instant;

use indicatif::ProgressBar;
use uc_daemon_client::setup::{ParsedSetupState, SetupHint, SetupVariant};

// ── Phase enum ──────────────────────────────────────────────────────

/// CLI-layer phase for the host pairing flow.
///
/// Per D-11: session_id lives INSIDE the phase variant (D-13).
#[derive(Debug, Clone, PartialEq)]
pub enum HostCliPhase {
    /// Waiting for a join request from a peer.
    WaitingJoinRequest,
    /// A peer has requested to join; awaiting host accept/reject decision.
    NeedDecision { session_id: String },
    /// Peer decision accepted; awaiting short-code verification confirmation.
    NeedVerification { session_id: String },
    /// Backend processing the pairing completion.
    #[allow(dead_code)]
    WaitingBackendCompletion,
    /// Pairing completed successfully.
    Completed,
    /// Pairing canceled or rejected.
    Canceled,
}

impl HostCliPhase {
    /// Returns true if this is a terminal phase (Completed or Canceled).
    #[inline]
    pub fn is_terminal(&self) -> bool {
        matches!(self, HostCliPhase::Completed | HostCliPhase::Canceled)
    }
}

// ── Session struct ─────────────────────────────────────────────────

/// Per D-16: session state carried through the host pairing loop.
#[derive(Debug)]
pub struct HostCliSession {
    /// Current CLI phase.
    pub phase: HostCliPhase,
    /// Whether pairing presence (lease) is currently enabled.
    pub pairing_presence_enabled: bool,
    /// Last time the host pairing lease was refreshed.
    pub last_lease_refresh: Instant,
    /// Active spinner, if any.
    pub spinner: Option<ProgressBar>,
}

impl Default for HostCliSession {
    fn default() -> Self {
        Self {
            phase: HostCliPhase::WaitingJoinRequest,
            pairing_presence_enabled: false,
            last_lease_refresh: Instant::now(),
            spinner: None,
        }
    }
}

// ── Phase derivation ───────────────────────────────────────────────

/// Per D-14: pure function to derive the next HostCliPhase from parsed state.
///
/// Takes the parsed daemon state and the current CLI phase to produce the next phase.
/// The `current` phase is used for transitional state (e.g., preserving session_id
/// across poll cycles when the backend hasn't changed).
///
/// Per D-15: `last_submitted_*` fields are NOT used here -- deduplication is handled
/// by the caller using the submitted session IDs.
#[must_use]
pub fn derive_host_phase(parsed: &ParsedSetupState, current: &HostCliPhase) -> HostCliPhase {
    use HostCliPhase::*;

    // Terminal states are sticky -- once Completed or Canceled, stay there.
    if current.is_terminal() {
        return current.clone();
    }

    match &parsed.hint {
        // Hint: idle -- host is not in an active pairing flow.
        SetupHint::Idle => {
            if matches!(current, WaitingBackendCompletion) {
                Canceled
            } else {
                current.clone()
            }
        }

        // Hint: completed -- pairing finished successfully.
        SetupHint::Completed => Completed,

        // Hint: host-confirm-peer -- this is the decision/verification branch.
        SetupHint::HostConfirmPeer => {
            if matches!(parsed.variant, SetupVariant::JoinSpaceConfirmPeer) {
                // NeedVerification state.
                let session_id = parsed
                    .session_id
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string());
                NeedVerification { session_id }
            } else {
                // NeedDecision state.
                let session_id = parsed
                    .session_id
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string());
                NeedDecision { session_id }
            }
        }

        // Any other hint (JoinSelectPeer, JoinEnterPassphrase, etc.)
        // means the host is not the active participant.
        _ => current.clone(),
    }
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use uc_daemon_client::setup::SetupHint;
    use uc_daemon_client::setup::SetupVariant;

    fn make_parsed(
        hint: SetupHint,
        variant: SetupVariant,
        session_id: Option<String>,
        completed: bool,
    ) -> ParsedSetupState {
        ParsedSetupState {
            hint,
            variant,
            session_id,
            has_completed: completed,
            short_code: None,
            selected_peer_label: None,
            error_code: None,
        }
    }

    fn idle_variant() -> SetupVariant {
        SetupVariant::Idle
    }
    fn join_confirm() -> SetupVariant {
        SetupVariant::JoinSpaceConfirmPeer
    }

    // WaitingJoinRequest

    #[test]
    fn from_waiting_idle_stays_waiting() {
        let parsed = make_parsed(SetupHint::Idle, idle_variant(), None, false);
        let phase = derive_host_phase(&parsed, &HostCliPhase::WaitingJoinRequest);
        assert!(matches!(phase, HostCliPhase::WaitingJoinRequest));
    }

    #[test]
    fn idle_after_waiting_backend_is_canceled() {
        let parsed = make_parsed(SetupHint::Idle, idle_variant(), None, false);
        let phase = derive_host_phase(&parsed, &HostCliPhase::WaitingBackendCompletion);
        assert!(matches!(phase, HostCliPhase::Canceled));
    }

    // NeedDecision

    #[test]
    fn host_confirm_without_variant_is_need_decision() {
        let parsed = make_parsed(
            SetupHint::HostConfirmPeer,
            SetupVariant::Idle,
            Some("s1".to_string()),
            false,
        );
        let phase = derive_host_phase(&parsed, &HostCliPhase::WaitingJoinRequest);
        assert!(matches!(phase, HostCliPhase::NeedDecision { session_id } if session_id == "s1"));
    }

    #[test]
    fn host_confirm_different_session_updates_session_id() {
        let parsed = make_parsed(
            SetupHint::HostConfirmPeer,
            SetupVariant::Idle,
            Some("s2".to_string()),
            false,
        );
        let phase = derive_host_phase(
            &parsed,
            &HostCliPhase::NeedDecision {
                session_id: "s1".to_string(),
            },
        );
        assert!(matches!(phase, HostCliPhase::NeedDecision { session_id } if session_id == "s2"));
    }

    // NeedVerification

    #[test]
    fn host_confirm_with_variant_is_need_verification() {
        let parsed = make_parsed(
            SetupHint::HostConfirmPeer,
            join_confirm(),
            Some("s3".to_string()),
            false,
        );
        let phase = derive_host_phase(&parsed, &HostCliPhase::WaitingJoinRequest);
        assert!(
            matches!(phase, HostCliPhase::NeedVerification { session_id } if session_id == "s3")
        );
    }

    #[test]
    fn need_decision_transitions_to_need_verification() {
        let parsed = make_parsed(
            SetupHint::HostConfirmPeer,
            join_confirm(),
            Some("s1".to_string()),
            false,
        );
        let phase = derive_host_phase(
            &parsed,
            &HostCliPhase::NeedDecision {
                session_id: "s1".to_string(),
            },
        );
        assert!(
            matches!(phase, HostCliPhase::NeedVerification { session_id } if session_id == "s1")
        );
    }

    // Completed

    #[test]
    fn completed_hint_is_completed_phase() {
        let parsed = make_parsed(SetupHint::Completed, SetupVariant::Completed, None, true);
        let phase = derive_host_phase(&parsed, &HostCliPhase::WaitingJoinRequest);
        assert!(matches!(phase, HostCliPhase::Completed));
    }

    #[test]
    fn completed_stays_completed() {
        let parsed = make_parsed(SetupHint::Completed, SetupVariant::Completed, None, true);
        let phase = derive_host_phase(&parsed, &HostCliPhase::Completed);
        assert!(matches!(phase, HostCliPhase::Completed));
    }

    // Join hints on host

    #[test]
    fn join_select_peer_while_waiting_stays_waiting() {
        let parsed = make_parsed(SetupHint::JoinSelectPeer, idle_variant(), None, false);
        let phase = derive_host_phase(&parsed, &HostCliPhase::WaitingJoinRequest);
        assert!(matches!(phase, HostCliPhase::WaitingJoinRequest));
    }
}
