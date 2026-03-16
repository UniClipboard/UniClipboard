use chrono::{DateTime, Duration, Utc};
use tracing::warn;

use crate::security::space_access::action::SpaceAccessAction;
use crate::security::space_access::event::SpaceAccessEvent;
use crate::security::space_access::state::{CancelReason, SpaceAccessState};

pub struct SpaceAccessStateMachine;

impl SpaceAccessStateMachine {
    pub fn transition(
        state: SpaceAccessState,
        event: SpaceAccessEvent,
    ) -> (SpaceAccessState, Vec<SpaceAccessAction>) {
        Self::transition_at(state, event, Utc::now())
    }

    pub(crate) fn transition_at(
        state: SpaceAccessState,
        event: SpaceAccessEvent,
        now: DateTime<Utc>,
    ) -> (SpaceAccessState, Vec<SpaceAccessAction>) {
        match (state, event) {
            // ===== Start =====
            (
                SpaceAccessState::Idle,
                SpaceAccessEvent::JoinRequested {
                    pairing_session_id,
                    ttl_secs,
                },
            ) => {
                let expires_at = now + Duration::seconds(ttl_secs as i64);
                (
                    SpaceAccessState::WaitingOffer {
                        pairing_session_id,
                        expires_at,
                    },
                    vec![SpaceAccessAction::StartTimer { ttl_secs }],
                )
            }
            (
                SpaceAccessState::Idle,
                SpaceAccessEvent::SponsorAuthorizationRequested {
                    pairing_session_id,
                    space_id,
                    ttl_secs,
                },
            ) => {
                let expires_at = now + Duration::seconds(ttl_secs as i64);
                let actions = vec![
                    SpaceAccessAction::RequestOfferPreparation {
                        pairing_session_id: pairing_session_id.clone().into(),
                        space_id: space_id.clone(),
                        expires_at,
                    },
                    SpaceAccessAction::SendOffer,
                    SpaceAccessAction::StartTimer { ttl_secs },
                ];
                (
                    SpaceAccessState::WaitingJoinerProof {
                        pairing_session_id,
                        space_id,
                        expires_at,
                    },
                    actions,
                )
            }

            // ===== Offer =====
            (
                SpaceAccessState::WaitingOffer { .. },
                SpaceAccessEvent::OfferAccepted {
                    pairing_session_id,
                    space_id,
                    expires_at,
                },
            ) => {
                let ttl_secs = ttl_from_expires_at(expires_at, now);
                (
                    SpaceAccessState::WaitingUserPassphrase {
                        pairing_session_id,
                        space_id,
                        expires_at,
                    },
                    vec![
                        SpaceAccessAction::StopTimer,
                        SpaceAccessAction::StartTimer { ttl_secs },
                    ],
                )
            }

            // ===== User input =====
            (
                SpaceAccessState::WaitingUserPassphrase {
                    space_id,
                    pairing_session_id,
                    ..
                },
                SpaceAccessEvent::PassphraseSubmitted,
            ) => (
                SpaceAccessState::WaitingDecision {
                    pairing_session_id,
                    space_id: space_id.clone(),
                    sent_at: now,
                },
                vec![
                    SpaceAccessAction::RequestSpaceKeyDerivation { space_id },
                    SpaceAccessAction::SendProof,
                ],
            ),

            // ===== Proof =====
            (
                SpaceAccessState::WaitingJoinerProof {
                    pairing_session_id,
                    space_id,
                    ..
                },
                SpaceAccessEvent::ProofVerified { .. },
            ) => (
                SpaceAccessState::Granted {
                    pairing_session_id,
                    space_id: space_id.clone(),
                },
                vec![
                    SpaceAccessAction::SendResult,
                    SpaceAccessAction::PersistSponsorAccess { space_id },
                    SpaceAccessAction::StopTimer,
                ],
            ),
            (
                SpaceAccessState::WaitingJoinerProof {
                    pairing_session_id,
                    space_id,
                    ..
                },
                SpaceAccessEvent::ProofRejected { reason, .. },
            ) => (
                SpaceAccessState::Denied {
                    pairing_session_id,
                    space_id,
                    reason,
                },
                vec![SpaceAccessAction::SendResult, SpaceAccessAction::StopTimer],
            ),

            // ===== Result =====
            (
                SpaceAccessState::WaitingDecision {
                    pairing_session_id,
                    space_id,
                    ..
                },
                SpaceAccessEvent::AccessGranted { .. },
            ) => (
                SpaceAccessState::Granted {
                    pairing_session_id,
                    space_id: space_id.clone(),
                },
                vec![
                    SpaceAccessAction::PersistJoinerAccess { space_id },
                    SpaceAccessAction::StopTimer,
                ],
            ),
            (
                SpaceAccessState::WaitingDecision {
                    pairing_session_id,
                    space_id,
                    ..
                },
                SpaceAccessEvent::AccessDenied { reason, .. },
            ) => (
                SpaceAccessState::Denied {
                    pairing_session_id,
                    space_id,
                    reason,
                },
                vec![SpaceAccessAction::StopTimer],
            ),

            // ===== Cancel / Timeout =====
            (
                SpaceAccessState::WaitingOffer {
                    pairing_session_id, ..
                },
                SpaceAccessEvent::CancelledByUser,
            ) => (
                SpaceAccessState::Cancelled {
                    pairing_session_id,
                    reason: CancelReason::UserCancelled,
                },
                vec![SpaceAccessAction::StopTimer],
            ),
            (
                SpaceAccessState::WaitingOffer {
                    pairing_session_id, ..
                },
                SpaceAccessEvent::Timeout,
            ) => (
                SpaceAccessState::Cancelled {
                    pairing_session_id,
                    reason: CancelReason::Timeout,
                },
                vec![SpaceAccessAction::StopTimer],
            ),
            (
                SpaceAccessState::WaitingOffer {
                    pairing_session_id, ..
                },
                SpaceAccessEvent::SessionClosed,
            ) => (
                SpaceAccessState::Cancelled {
                    pairing_session_id,
                    reason: CancelReason::SessionClosed,
                },
                vec![SpaceAccessAction::StopTimer],
            ),
            (
                SpaceAccessState::WaitingUserPassphrase {
                    pairing_session_id, ..
                },
                SpaceAccessEvent::CancelledByUser,
            ) => (
                SpaceAccessState::Cancelled {
                    pairing_session_id,
                    reason: CancelReason::UserCancelled,
                },
                vec![SpaceAccessAction::StopTimer],
            ),
            (
                SpaceAccessState::WaitingUserPassphrase {
                    pairing_session_id, ..
                },
                SpaceAccessEvent::Timeout,
            ) => (
                SpaceAccessState::Cancelled {
                    pairing_session_id,
                    reason: CancelReason::Timeout,
                },
                vec![SpaceAccessAction::StopTimer],
            ),
            (
                SpaceAccessState::WaitingUserPassphrase {
                    pairing_session_id, ..
                },
                SpaceAccessEvent::SessionClosed,
            ) => (
                SpaceAccessState::Cancelled {
                    pairing_session_id,
                    reason: CancelReason::SessionClosed,
                },
                vec![SpaceAccessAction::StopTimer],
            ),
            (
                SpaceAccessState::WaitingDecision {
                    pairing_session_id, ..
                },
                SpaceAccessEvent::CancelledByUser,
            ) => (
                SpaceAccessState::Cancelled {
                    pairing_session_id,
                    reason: CancelReason::UserCancelled,
                },
                vec![SpaceAccessAction::StopTimer],
            ),
            (
                SpaceAccessState::WaitingDecision {
                    pairing_session_id, ..
                },
                SpaceAccessEvent::Timeout,
            ) => (
                SpaceAccessState::Cancelled {
                    pairing_session_id,
                    reason: CancelReason::Timeout,
                },
                vec![SpaceAccessAction::StopTimer],
            ),
            (
                SpaceAccessState::WaitingDecision {
                    pairing_session_id, ..
                },
                SpaceAccessEvent::SessionClosed,
            ) => (
                SpaceAccessState::Cancelled {
                    pairing_session_id,
                    reason: CancelReason::SessionClosed,
                },
                vec![SpaceAccessAction::StopTimer],
            ),
            (
                SpaceAccessState::WaitingJoinerProof {
                    pairing_session_id, ..
                },
                SpaceAccessEvent::CancelledByUser,
            ) => (
                SpaceAccessState::Cancelled {
                    pairing_session_id,
                    reason: CancelReason::UserCancelled,
                },
                vec![SpaceAccessAction::StopTimer],
            ),
            (
                SpaceAccessState::WaitingJoinerProof {
                    pairing_session_id, ..
                },
                SpaceAccessEvent::Timeout,
            ) => (
                SpaceAccessState::Cancelled {
                    pairing_session_id,
                    reason: CancelReason::Timeout,
                },
                vec![SpaceAccessAction::StopTimer],
            ),
            (
                SpaceAccessState::WaitingJoinerProof {
                    pairing_session_id, ..
                },
                SpaceAccessEvent::SessionClosed,
            ) => (
                SpaceAccessState::Cancelled {
                    pairing_session_id,
                    reason: CancelReason::SessionClosed,
                },
                vec![SpaceAccessAction::StopTimer],
            ),

            // ===== Sponsor re-authorization from terminal states =====
            // After completing authorization for one joiner, the sponsor must be
            // able to start a fresh authorization for the next joiner.
            (
                SpaceAccessState::Granted { .. }
                | SpaceAccessState::Denied { .. }
                | SpaceAccessState::Cancelled { .. },
                SpaceAccessEvent::SponsorAuthorizationRequested {
                    pairing_session_id,
                    space_id,
                    ttl_secs,
                },
            ) => {
                let expires_at = now + Duration::seconds(ttl_secs as i64);
                let actions = vec![
                    SpaceAccessAction::RequestOfferPreparation {
                        pairing_session_id: pairing_session_id.clone().into(),
                        space_id: space_id.clone(),
                        expires_at,
                    },
                    SpaceAccessAction::SendOffer,
                    SpaceAccessAction::StartTimer { ttl_secs },
                ];
                (
                    SpaceAccessState::WaitingJoinerProof {
                        pairing_session_id,
                        space_id,
                        expires_at,
                    },
                    actions,
                )
            }

            // ===== Terminal =====
            (state @ SpaceAccessState::Granted { .. }, _) => (state, vec![]),
            (state @ SpaceAccessState::Denied { .. }, _) => (state, vec![]),
            (state @ SpaceAccessState::Cancelled { .. }, _) => (state, vec![]),

            // ===== Invalid =====
            (state, event) => {
                warn!(?state, ?event, "invalid space access transition");
                (state, vec![])
            }
        }
    }
}

fn ttl_from_expires_at(expires_at: DateTime<Utc>, now: DateTime<Utc>) -> u64 {
    let delta = expires_at.signed_duration_since(now).num_seconds();
    if delta <= 0 {
        0
    } else {
        delta as u64
    }
}

#[cfg(test)]
mod tests {
    use super::SpaceAccessStateMachine;
    use crate::ids::SessionId as CoreSessionId;
    use crate::ids::SpaceId;
    use crate::network::SessionId as NetSessionId;
    use crate::security::space_access::action::SpaceAccessAction;
    use crate::security::space_access::event::SpaceAccessEvent;
    use crate::security::space_access::state::{CancelReason, DenyReason, SpaceAccessState};
    use chrono::{DateTime, Duration, TimeZone, Utc};

    fn fixed_now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
    }

    fn cases(
        now: DateTime<Utc>,
    ) -> Vec<(
        &'static str,
        SpaceAccessState,
        SpaceAccessEvent,
        SpaceAccessState,
        Vec<SpaceAccessAction>,
    )> {
        let pairing_session_id: NetSessionId = "session-1".to_string();
        let space_id: SpaceId = "space-1".into();
        let ttl_secs = 30_u64;
        let expires_at = now + Duration::seconds(ttl_secs as i64);

        vec![
            (
                "idle -> join requested",
                SpaceAccessState::Idle,
                SpaceAccessEvent::JoinRequested {
                    pairing_session_id: pairing_session_id.clone(),
                    ttl_secs,
                },
                SpaceAccessState::WaitingOffer {
                    pairing_session_id: pairing_session_id.clone(),
                    expires_at,
                },
                vec![SpaceAccessAction::StartTimer { ttl_secs }],
            ),
            (
                "idle -> sponsor authorization requested",
                SpaceAccessState::Idle,
                SpaceAccessEvent::SponsorAuthorizationRequested {
                    pairing_session_id: pairing_session_id.clone(),
                    space_id: space_id.clone(),
                    ttl_secs,
                },
                SpaceAccessState::WaitingJoinerProof {
                    pairing_session_id: pairing_session_id.clone(),
                    space_id: space_id.clone(),
                    expires_at,
                },
                vec![
                    SpaceAccessAction::RequestOfferPreparation {
                        pairing_session_id: CoreSessionId::from("session-1"),
                        space_id: space_id.clone(),
                        expires_at,
                    },
                    SpaceAccessAction::SendOffer,
                    SpaceAccessAction::StartTimer { ttl_secs },
                ],
            ),
            (
                "waiting offer -> offer accepted",
                SpaceAccessState::WaitingOffer {
                    pairing_session_id: pairing_session_id.clone(),
                    expires_at,
                },
                SpaceAccessEvent::OfferAccepted {
                    pairing_session_id: pairing_session_id.clone(),
                    space_id: space_id.clone(),
                    expires_at,
                },
                SpaceAccessState::WaitingUserPassphrase {
                    pairing_session_id: pairing_session_id.clone(),
                    space_id: space_id.clone(),
                    expires_at,
                },
                vec![
                    SpaceAccessAction::StopTimer,
                    SpaceAccessAction::StartTimer { ttl_secs },
                ],
            ),
            (
                "waiting passphrase -> passphrase submitted",
                SpaceAccessState::WaitingUserPassphrase {
                    pairing_session_id: pairing_session_id.clone(),
                    space_id: space_id.clone(),
                    expires_at,
                },
                SpaceAccessEvent::PassphraseSubmitted,
                SpaceAccessState::WaitingDecision {
                    pairing_session_id: pairing_session_id.clone(),
                    space_id: space_id.clone(),
                    sent_at: now,
                },
                vec![
                    SpaceAccessAction::RequestSpaceKeyDerivation {
                        space_id: space_id.clone(),
                    },
                    SpaceAccessAction::SendProof,
                ],
            ),
            (
                "waiting decision -> access granted",
                SpaceAccessState::WaitingDecision {
                    pairing_session_id: pairing_session_id.clone(),
                    space_id: space_id.clone(),
                    sent_at: now,
                },
                SpaceAccessEvent::AccessGranted {
                    pairing_session_id: pairing_session_id.clone(),
                    space_id: space_id.clone(),
                },
                SpaceAccessState::Granted {
                    pairing_session_id: pairing_session_id.clone(),
                    space_id: space_id.clone(),
                },
                vec![
                    SpaceAccessAction::PersistJoinerAccess {
                        space_id: space_id.clone(),
                    },
                    SpaceAccessAction::StopTimer,
                ],
            ),
            (
                "waiting decision -> access denied",
                SpaceAccessState::WaitingDecision {
                    pairing_session_id: pairing_session_id.clone(),
                    space_id: space_id.clone(),
                    sent_at: now,
                },
                SpaceAccessEvent::AccessDenied {
                    pairing_session_id: pairing_session_id.clone(),
                    space_id: space_id.clone(),
                    reason: DenyReason::InvalidProof,
                },
                SpaceAccessState::Denied {
                    pairing_session_id: pairing_session_id.clone(),
                    space_id: space_id.clone(),
                    reason: DenyReason::InvalidProof,
                },
                vec![SpaceAccessAction::StopTimer],
            ),
            (
                "waiting proof -> proof verified",
                SpaceAccessState::WaitingJoinerProof {
                    pairing_session_id: pairing_session_id.clone(),
                    space_id: space_id.clone(),
                    expires_at,
                },
                SpaceAccessEvent::ProofVerified {
                    pairing_session_id: pairing_session_id.clone(),
                    space_id: space_id.clone(),
                },
                SpaceAccessState::Granted {
                    pairing_session_id: pairing_session_id.clone(),
                    space_id: space_id.clone(),
                },
                vec![
                    SpaceAccessAction::SendResult,
                    SpaceAccessAction::PersistSponsorAccess {
                        space_id: space_id.clone(),
                    },
                    SpaceAccessAction::StopTimer,
                ],
            ),
            (
                "waiting proof -> proof rejected",
                SpaceAccessState::WaitingJoinerProof {
                    pairing_session_id: pairing_session_id.clone(),
                    space_id: space_id.clone(),
                    expires_at,
                },
                SpaceAccessEvent::ProofRejected {
                    pairing_session_id: pairing_session_id.clone(),
                    space_id: space_id.clone(),
                    reason: DenyReason::InvalidProof,
                },
                SpaceAccessState::Denied {
                    pairing_session_id: pairing_session_id.clone(),
                    space_id: space_id.clone(),
                    reason: DenyReason::InvalidProof,
                },
                vec![SpaceAccessAction::SendResult, SpaceAccessAction::StopTimer],
            ),
            (
                "waiting offer -> cancelled by user",
                SpaceAccessState::WaitingOffer {
                    pairing_session_id: pairing_session_id.clone(),
                    expires_at,
                },
                SpaceAccessEvent::CancelledByUser,
                SpaceAccessState::Cancelled {
                    pairing_session_id: pairing_session_id.clone(),
                    reason: CancelReason::UserCancelled,
                },
                vec![SpaceAccessAction::StopTimer],
            ),
            (
                "granted ignores events",
                SpaceAccessState::Granted {
                    pairing_session_id: pairing_session_id.clone(),
                    space_id: space_id.clone(),
                },
                SpaceAccessEvent::Timeout,
                SpaceAccessState::Granted {
                    pairing_session_id: pairing_session_id.clone(),
                    space_id: space_id.clone(),
                },
                vec![],
            ),
        ]
    }

    #[test]
    fn space_access_state_machine_table_driven() {
        let now = fixed_now();
        for (name, from, event, expected_state, expected_actions) in cases(now) {
            let (next, actions) = SpaceAccessStateMachine::transition_at(from, event, now);
            assert_eq!(next, expected_state, "state mismatch: {}", name);
            assert_eq!(actions, expected_actions, "actions mismatch: {}", name);
        }
    }

    #[test]
    fn sponsor_reauthorization_from_granted() {
        let now = fixed_now();
        let pairing_session_id = "session-2".to_string();
        let space_id: SpaceId = "space-1".into();
        let ttl_secs = 30_u64;
        let expires_at = now + Duration::seconds(ttl_secs as i64);

        let from = SpaceAccessState::Granted {
            pairing_session_id: "session-1".to_string(),
            space_id: "space-1".into(),
        };

        let (next, actions) = SpaceAccessStateMachine::transition_at(
            from,
            SpaceAccessEvent::SponsorAuthorizationRequested {
                pairing_session_id: pairing_session_id.clone(),
                space_id: space_id.clone(),
                ttl_secs,
            },
            now,
        );

        assert_eq!(
            next,
            SpaceAccessState::WaitingJoinerProof {
                pairing_session_id: pairing_session_id.clone(),
                space_id: space_id.clone(),
                expires_at,
            }
        );
        assert_eq!(
            actions,
            vec![
                SpaceAccessAction::RequestOfferPreparation {
                    pairing_session_id: CoreSessionId::from("session-2"),
                    space_id: space_id.clone(),
                    expires_at,
                },
                SpaceAccessAction::SendOffer,
                SpaceAccessAction::StartTimer { ttl_secs },
            ]
        );
    }

    #[test]
    fn sponsor_reauthorization_from_denied() {
        let now = fixed_now();
        let from = SpaceAccessState::Denied {
            pairing_session_id: "session-1".to_string(),
            space_id: "space-1".into(),
            reason: DenyReason::InvalidProof,
        };

        let (next, _actions) = SpaceAccessStateMachine::transition_at(
            from,
            SpaceAccessEvent::SponsorAuthorizationRequested {
                pairing_session_id: "session-2".to_string(),
                space_id: "space-1".into(),
                ttl_secs: 30,
            },
            now,
        );

        assert!(matches!(next, SpaceAccessState::WaitingJoinerProof { .. }));
    }

    #[test]
    fn sponsor_reauthorization_from_cancelled() {
        let now = fixed_now();
        let from = SpaceAccessState::Cancelled {
            pairing_session_id: "session-1".to_string(),
            reason: CancelReason::Timeout,
        };

        let (next, _actions) = SpaceAccessStateMachine::transition_at(
            from,
            SpaceAccessEvent::SponsorAuthorizationRequested {
                pairing_session_id: "session-2".to_string(),
                space_id: "space-1".into(),
                ttl_secs: 30,
            },
            now,
        );

        assert!(matches!(next, SpaceAccessState::WaitingJoinerProof { .. }));
    }

    #[test]
    fn invalid_transition_is_noop() {
        let now = fixed_now();
        let from = SpaceAccessState::Idle;
        let event = SpaceAccessEvent::PassphraseSubmitted;

        let (next, actions) = SpaceAccessStateMachine::transition_at(from.clone(), event, now);

        assert_eq!(next, from);
        assert!(actions.is_empty());
    }
}
