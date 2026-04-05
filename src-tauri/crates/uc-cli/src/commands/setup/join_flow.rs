//! Join CLI flow -- JoinCliPhase, JoinCliSession, and run_connect.
//!
//! Phase-driven loop structure per D-16 through D-19.

use uc_daemon_client::setup::{ParsedSetupState, SetupHint};

// ── Phase enum ──────────────────────────────────────────────────────

/// CLI-layer phase for the join pairing flow.
///
/// Per D-12: session_id lives INSIDE the NeedPeerConfirmation variant (D-13).
#[derive(Debug, Clone, PartialEq)]
pub enum JoinCliPhase {
    /// Initial state -- prompting user to select a peer to join.
    SelectingPeer,
    /// Actively scanning/discovering peers on the network.
    #[allow(dead_code)]
    WaitingPeerDiscovery,
    /// Waiting for the host to respond to our join request.
    #[allow(dead_code)]
    WaitingHostResponse,
    /// Host accepted; awaiting short-code verification confirmation from joiner.
    NeedPeerConfirmation { session_id: String },
    /// Waiting for the joiner to enter the space passphrase.
    NeedPassphrase,
    /// Backend processing the pairing completion.
    #[allow(dead_code)]
    WaitingBackendCompletion,
    /// Pairing completed successfully.
    Completed,
    /// Pairing canceled or rejected.
    Canceled,
}

impl JoinCliPhase {
    /// Returns true if this is a terminal phase.
    #[inline]
    pub fn is_terminal(&self) -> bool {
        matches!(self, JoinCliPhase::Completed | JoinCliPhase::Canceled)
    }
}

// ── Session struct ─────────────────────────────────────────────────

/// Per D-16: session state carried through the join pairing loop.
#[derive(Debug)]
pub struct JoinCliSession {
    /// Current CLI phase.
    pub phase: JoinCliPhase,
    /// Whether a peer request has been submitted to the host.
    pub submitted_peer_request: bool,
    /// Active spinner, if any.
    pub spinner: Option<indicatif::ProgressBar>,
}

impl Default for JoinCliSession {
    fn default() -> Self {
        Self {
            phase: JoinCliPhase::SelectingPeer,
            submitted_peer_request: false,
            spinner: None,
        }
    }
}

// ── Phase derivation ───────────────────────────────────────────────

/// Per D-14: pure function to derive the next JoinCliPhase from parsed state.
#[must_use]
pub fn derive_join_phase(parsed: &ParsedSetupState, current: &JoinCliPhase) -> JoinCliPhase {
    use JoinCliPhase::*;

    // Terminal states are sticky.
    if current.is_terminal() {
        return current.clone();
    }

    match &parsed.hint {
        // Idle: if we were waiting on backend, treat as Canceled.
        SetupHint::Idle => {
            if matches!(current, WaitingBackendCompletion | WaitingHostResponse) {
                Canceled
            } else {
                current.clone()
            }
        }

        // Completed: pairing finished.
        SetupHint::Completed => Completed,

        // JoinSelectPeer: actively selecting a peer to join.
        SetupHint::JoinSelectPeer => SelectingPeer,

        // JoinWaitingForHost: peer selected; waiting for the host to accept.
        SetupHint::JoinWaitingForHost => WaitingHostResponse,

        // JoinEnterPassphrase: host approved us, now we need the passphrase.
        SetupHint::JoinEnterPassphrase => NeedPassphrase,

        // HostConfirmPeer: the host is asking us to confirm the short code.
        SetupHint::HostConfirmPeer => {
            let session_id = parsed
                .session_id
                .clone()
                .unwrap_or_else(|| "unknown".to_string());
            NeedPeerConfirmation { session_id }
        }

        // Any other hint: stay in current phase.
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

    fn idle() -> SetupVariant {
        SetupVariant::Idle
    }
    fn join_input_passphrase() -> SetupVariant {
        SetupVariant::JoinSpaceInputPassphrase
    }
    fn join_confirm() -> SetupVariant {
        SetupVariant::JoinSpaceConfirmPeer
    }

    // SelectingPeer

    #[test]
    fn join_select_peer_is_selecting() {
        let parsed = make_parsed(SetupHint::JoinSelectPeer, idle(), None, false);
        let phase = derive_join_phase(&parsed, &JoinCliPhase::SelectingPeer);
        assert!(matches!(phase, JoinCliPhase::SelectingPeer));
    }

    #[test]
    fn join_select_peer_from_other_phase_resets_to_selecting() {
        let parsed = make_parsed(SetupHint::JoinSelectPeer, idle(), None, false);
        let phase = derive_join_phase(&parsed, &JoinCliPhase::WaitingHostResponse);
        assert!(matches!(phase, JoinCliPhase::SelectingPeer));
    }

    // WaitingHostResponse

    #[test]
    fn join_waiting_for_host_is_waiting_host_response() {
        let parsed = make_parsed(SetupHint::JoinWaitingForHost, idle(), None, false);
        let phase = derive_join_phase(&parsed, &JoinCliPhase::SelectingPeer);
        assert!(matches!(phase, JoinCliPhase::WaitingHostResponse));
    }

    // NeedPassphrase

    #[test]
    fn join_enter_passphrase_is_need_passphrase() {
        let parsed = make_parsed(
            SetupHint::JoinEnterPassphrase,
            join_input_passphrase(),
            Some("s1".to_string()),
            false,
        );
        let phase = derive_join_phase(&parsed, &JoinCliPhase::SelectingPeer);
        assert!(matches!(phase, JoinCliPhase::NeedPassphrase));
    }

    // NeedPeerConfirmation

    #[test]
    fn host_confirm_peer_is_need_peer_confirmation() {
        let parsed = make_parsed(
            SetupHint::HostConfirmPeer,
            join_confirm(),
            Some("s2".to_string()),
            false,
        );
        let phase = derive_join_phase(&parsed, &JoinCliPhase::WaitingHostResponse);
        assert!(
            matches!(phase, JoinCliPhase::NeedPeerConfirmation { session_id } if session_id == "s2")
        );
    }

    #[test]
    fn need_passphrase_transitions_to_need_peer_confirmation() {
        let parsed = make_parsed(
            SetupHint::HostConfirmPeer,
            join_confirm(),
            Some("s1".to_string()),
            false,
        );
        let phase = derive_join_phase(&parsed, &JoinCliPhase::NeedPassphrase);
        assert!(
            matches!(phase, JoinCliPhase::NeedPeerConfirmation { session_id } if session_id == "s1")
        );
    }

    // Completed

    #[test]
    fn completed_hint_is_completed() {
        let parsed = make_parsed(SetupHint::Completed, SetupVariant::Completed, None, true);
        let phase = derive_join_phase(&parsed, &JoinCliPhase::SelectingPeer);
        assert!(matches!(phase, JoinCliPhase::Completed));
    }

    // Canceled

    #[test]
    fn idle_after_waiting_is_canceled() {
        let parsed = make_parsed(SetupHint::Idle, idle(), None, false);
        let phase = derive_join_phase(&parsed, &JoinCliPhase::WaitingBackendCompletion);
        assert!(matches!(phase, JoinCliPhase::Canceled));
    }

    #[test]
    fn idle_while_selecting_stays_selecting() {
        let parsed = make_parsed(SetupHint::Idle, idle(), None, false);
        let phase = derive_join_phase(&parsed, &JoinCliPhase::SelectingPeer);
        assert!(matches!(phase, JoinCliPhase::SelectingPeer));
    }
}
