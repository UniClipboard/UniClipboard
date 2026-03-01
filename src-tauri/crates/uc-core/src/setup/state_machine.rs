use tracing::warn;

use crate::setup::{SetupAction, SetupError, SetupEvent, SetupState};

pub struct SetupStateMachine;

impl SetupStateMachine {
    pub fn transition(state: SetupState, event: SetupEvent) -> (SetupState, Vec<SetupAction>) {
        match (state, event) {
            // 1. Welcome
            (SetupState::Welcome, SetupEvent::StartNewSpace) => (
                SetupState::CreateSpaceInputPassphrase { error: None },
                vec![],
            ),
            (SetupState::Welcome, SetupEvent::StartJoinSpace) => (
                SetupState::JoinSpaceSelectDevice { error: None },
                vec![SetupAction::EnsureDiscovery],
            ),

            // 2. CreateSpaceInputPassphrase
            (
                SetupState::CreateSpaceInputPassphrase { .. },
                SetupEvent::SubmitPassphrase { .. },
            ) => (
                SetupState::ProcessingCreateSpace {
                    message: Some("Creating encrypted space…".into()),
                },
                vec![SetupAction::CreateEncryptedSpace],
            ),
            (SetupState::CreateSpaceInputPassphrase { .. }, SetupEvent::CancelSetup) => {
                (SetupState::Welcome, vec![])
            }
            (SetupState::ProcessingCreateSpace { .. }, SetupEvent::CreateSpaceFailed { error }) => {
                (
                    SetupState::CreateSpaceInputPassphrase { error: Some(error) },
                    vec![],
                )
            }

            // 3. JoinSpaceSelectDevice
            (SetupState::JoinSpaceSelectDevice { .. }, SetupEvent::ChooseJoinPeer { .. }) => (
                // NOTE: peer_id is stored into SetupContext by orchestrator
                // before or after dispatching this event.
                SetupState::ProcessingJoinSpace {
                    message: Some("Connecting to selected device…".into()),
                },
                vec![SetupAction::EnsurePairing {}],
            ),
            (state @ SetupState::JoinSpaceSelectDevice { .. }, SetupEvent::RefreshPeerList) => {
                (state, vec![SetupAction::EnsureDiscovery])
            }
            (SetupState::JoinSpaceSelectDevice { .. }, SetupEvent::CancelSetup) => {
                (SetupState::Welcome, vec![])
            }

            // 4. JoinSpaceConfirmPeer
            (SetupState::JoinSpaceConfirmPeer { .. }, SetupEvent::ConfirmPeerTrust) => (
                SetupState::JoinSpaceInputPassphrase { error: None },
                vec![SetupAction::ConfirmPeerTrust {}],
            ),
            (SetupState::JoinSpaceConfirmPeer { .. }, SetupEvent::CancelSetup) => (
                SetupState::JoinSpaceSelectDevice { error: None },
                vec![SetupAction::AbortPairing {}],
            ),

            // 5. JoinSpaceInputPassphrase
            (SetupState::JoinSpaceInputPassphrase { .. }, SetupEvent::SubmitPassphrase { .. }) => (
                SetupState::ProcessingJoinSpace {
                    message: Some("Verifying passphrase…".into()),
                },
                vec![SetupAction::StartJoinSpaceAccess {}],
            ),
            (SetupState::JoinSpaceInputPassphrase { .. }, SetupEvent::CancelSetup) => {
                (SetupState::JoinSpaceSelectDevice { error: None }, vec![])
            }

            // 6. Processing
            (SetupState::ProcessingJoinSpace { .. }, SetupEvent::JoinSpaceSucceeded) => {
                (SetupState::Completed, vec![SetupAction::MarkSetupComplete])
            }
            (SetupState::ProcessingCreateSpace { .. }, SetupEvent::CreateSpaceSucceeded) => {
                (SetupState::Completed, vec![SetupAction::MarkSetupComplete])
            }
            (SetupState::ProcessingJoinSpace { .. }, SetupEvent::JoinSpaceFailed { error }) => {
                let target = match &error {
                    // Passphrase-related failures → return to passphrase input
                    SetupError::PassphraseInvalidOrMismatch
                    | SetupError::PassphraseMismatch
                    | SetupError::PassphraseEmpty => {
                        SetupState::JoinSpaceInputPassphrase { error: Some(error) }
                    }
                    // Pairing/network failures → return to device selection
                    SetupError::PairingFailed
                    | SetupError::PairingRejected
                    | SetupError::PeerUnavailable
                    | SetupError::NetworkTimeout => {
                        SetupState::JoinSpaceSelectDevice { error: Some(error) }
                    }
                };
                (target, vec![])
            }
            (SetupState::ProcessingJoinSpace { .. }, SetupEvent::CancelSetup) => {
                (SetupState::Welcome, vec![SetupAction::AbortPairing {}])
            }
            (SetupState::ProcessingCreateSpace { .. }, SetupEvent::CancelSetup) => {
                (SetupState::Welcome, vec![SetupAction::AbortPairing {}])
            }

            // 7. Completed
            (state @ SetupState::Completed, _) => (state, vec![]),

            // 8. Invalid
            (state, event) => {
                warn!(?state, ?event, "invalid setup transition");
                (state, vec![SetupAction::AbortPairing {}])
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::setup::{SetupAction, SetupError, SetupEvent, SetupState, SetupStateMachine};

    fn cases() -> Vec<(
        &'static str,
        SetupState,
        fn() -> SetupEvent,
        SetupState,
        Vec<SetupAction>,
    )> {
        vec![
            // ===== Welcome =====
            (
                "welcome -> start new space",
                SetupState::Welcome,
                || SetupEvent::StartNewSpace,
                SetupState::CreateSpaceInputPassphrase { error: None },
                vec![],
            ),
            (
                "welcome -> start join space",
                SetupState::Welcome,
                || SetupEvent::StartJoinSpace,
                SetupState::JoinSpaceSelectDevice { error: None },
                vec![SetupAction::EnsureDiscovery],
            ),
            // ===== Create Space =====
            (
                "create space submit passphrase",
                SetupState::CreateSpaceInputPassphrase { error: None },
                || SetupEvent::SubmitPassphrase {
                    passphrase: "secret".into(),
                },
                SetupState::ProcessingCreateSpace {
                    message: Some("Creating encrypted space…".into()),
                },
                vec![SetupAction::CreateEncryptedSpace],
            ),
            (
                "create space failed",
                SetupState::ProcessingCreateSpace { message: None },
                || SetupEvent::CreateSpaceFailed {
                    error: SetupError::PassphraseInvalidOrMismatch,
                },
                SetupState::CreateSpaceInputPassphrase {
                    error: Some(SetupError::PassphraseInvalidOrMismatch),
                },
                vec![],
            ),
            (
                "create space succeeded",
                SetupState::ProcessingCreateSpace { message: None },
                || SetupEvent::CreateSpaceSucceeded,
                SetupState::Completed,
                vec![SetupAction::MarkSetupComplete],
            ),
            (
                "create space cancel",
                SetupState::CreateSpaceInputPassphrase { error: None },
                || SetupEvent::CancelSetup,
                SetupState::Welcome,
                vec![],
            ),
            (
                "create space cancel during processing",
                SetupState::ProcessingCreateSpace { message: None },
                || SetupEvent::CancelSetup,
                SetupState::Welcome,
                vec![SetupAction::AbortPairing {}],
            ),
            // ===== Join Space =====
            (
                "join select peer",
                SetupState::JoinSpaceSelectDevice { error: None },
                || SetupEvent::ChooseJoinPeer {
                    peer_id: "peer-1".into(),
                },
                SetupState::ProcessingJoinSpace {
                    message: Some("Connecting to selected device…".into()),
                },
                vec![SetupAction::EnsurePairing {}],
            ),
            (
                "join refresh peer list",
                SetupState::JoinSpaceSelectDevice { error: None },
                || SetupEvent::RefreshPeerList,
                SetupState::JoinSpaceSelectDevice { error: None },
                vec![SetupAction::EnsureDiscovery],
            ),
            (
                "join cancel from device picker",
                SetupState::JoinSpaceSelectDevice { error: None },
                || SetupEvent::CancelSetup,
                SetupState::Welcome,
                vec![],
            ),
            // ===== Confirm Peer =====
            (
                "confirm peer trust",
                SetupState::JoinSpaceConfirmPeer {
                    short_code: "123-456".into(),
                    peer_fingerprint: None,
                    error: None,
                },
                || SetupEvent::ConfirmPeerTrust,
                SetupState::JoinSpaceInputPassphrase { error: None },
                vec![SetupAction::ConfirmPeerTrust {}],
            ),
            (
                "cancel confirm peer",
                SetupState::JoinSpaceConfirmPeer {
                    short_code: "123-456".into(),
                    peer_fingerprint: None,
                    error: None,
                },
                || SetupEvent::CancelSetup,
                SetupState::JoinSpaceSelectDevice { error: None },
                vec![SetupAction::AbortPairing {}],
            ),
            // ===== Join Passphrase =====
            (
                "submit join passphrase",
                SetupState::JoinSpaceInputPassphrase { error: None },
                || SetupEvent::SubmitPassphrase {
                    passphrase: "secret".into(),
                },
                SetupState::ProcessingJoinSpace {
                    message: Some("Verifying passphrase…".into()),
                },
                vec![SetupAction::StartJoinSpaceAccess {}],
            ),
            (
                "join passphrase cancel",
                SetupState::JoinSpaceInputPassphrase { error: None },
                || SetupEvent::CancelSetup,
                SetupState::JoinSpaceSelectDevice { error: None },
                vec![],
            ),
            // ===== Processing Join =====
            (
                "join success",
                SetupState::ProcessingJoinSpace { message: None },
                || SetupEvent::JoinSpaceSucceeded,
                SetupState::Completed,
                vec![SetupAction::MarkSetupComplete],
            ),
            (
                "join failed (passphrase) -> back to passphrase input",
                SetupState::ProcessingJoinSpace { message: None },
                || SetupEvent::JoinSpaceFailed {
                    error: SetupError::PassphraseInvalidOrMismatch,
                },
                SetupState::JoinSpaceInputPassphrase {
                    error: Some(SetupError::PassphraseInvalidOrMismatch),
                },
                vec![],
            ),
            (
                "join failed (pairing) -> back to device selection",
                SetupState::ProcessingJoinSpace { message: None },
                || SetupEvent::JoinSpaceFailed {
                    error: SetupError::PairingFailed,
                },
                SetupState::JoinSpaceSelectDevice {
                    error: Some(SetupError::PairingFailed),
                },
                vec![],
            ),
            (
                "join failed (peer unavailable) -> back to device selection",
                SetupState::ProcessingJoinSpace { message: None },
                || SetupEvent::JoinSpaceFailed {
                    error: SetupError::PeerUnavailable,
                },
                SetupState::JoinSpaceSelectDevice {
                    error: Some(SetupError::PeerUnavailable),
                },
                vec![],
            ),
            (
                "join cancel during processing",
                SetupState::ProcessingJoinSpace { message: None },
                || SetupEvent::CancelSetup,
                SetupState::Welcome,
                vec![SetupAction::AbortPairing {}],
            ),
            // ===== Completed =====
            (
                "completed ignores events",
                SetupState::Completed,
                || SetupEvent::CancelSetup,
                SetupState::Completed,
                vec![],
            ),
        ]
    }

    #[test]
    fn setup_state_machine_table_driven() {
        for (name, from, event_fn, expected_state, expected_actions) in cases() {
            let event = event_fn();
            let (next, actions) = SetupStateMachine::transition(from.clone(), event);
            assert_eq!(next, expected_state, "state mismatch: {}", name);
            assert_eq!(actions, expected_actions, "actions mismatch: {}", name);
        }
    }

    #[test]
    fn invalid_transition_aborts_pairing() {
        let from = SetupState::Welcome;
        let event = SetupEvent::JoinSpaceSucceeded;

        let (next, actions) = SetupStateMachine::transition(from.clone(), event);

        assert_eq!(next, from);
        assert_eq!(actions, vec![SetupAction::AbortPairing {}]);
    }

    #[test]
    fn mark_setup_complete_is_the_ready_bridge() {
        // TODO(start-network-after-unlock): Setup state machine itself has no explicit Ready state.
        // Ready is emitted by uc-app's AppLifecycleCoordinator after MarkSetupComplete succeeds.

        let (create_next, create_actions) = SetupStateMachine::transition(
            SetupState::ProcessingCreateSpace { message: None },
            SetupEvent::CreateSpaceSucceeded,
        );
        assert_eq!(create_next, SetupState::Completed);
        assert_eq!(create_actions, vec![SetupAction::MarkSetupComplete]);

        let (join_next, join_actions) = SetupStateMachine::transition(
            SetupState::ProcessingJoinSpace { message: None },
            SetupEvent::JoinSpaceSucceeded,
        );
        assert_eq!(join_next, SetupState::Completed);
        assert_eq!(join_actions, vec![SetupAction::MarkSetupComplete]);
    }
}
