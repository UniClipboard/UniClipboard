use super::errors::TrustedPeerApplicationError;
use super::state::{TrustState, TrustStateEvent};

/// Pure state-machine transition per DOMAIN §5.4.
///
/// - Valid paths:
///     `Idle --Initiate--> EstablishingSession`
///     `EstablishingSession --SessionOpened--> AwaitingUserVerification`
///     `AwaitingUserVerification --UserConfirmed--> Trusted`
///     `{Idle, EstablishingSession, AwaitingUserVerification} --UserCancelled/TimedOut/ProtocolError--> Aborted`
/// - Terminal states (`Trusted`, `Aborted`) accept no further events.
/// - `SessionOpened.peer_device_id` must match the peer that started the flow.
pub fn transition(
    state: TrustState,
    event: TrustStateEvent,
) -> Result<TrustState, TrustedPeerApplicationError> {
    use TrustState::*;
    use TrustStateEvent::*;

    match (state, event) {
        (Idle, Initiate { peer_device_id }) => Ok(EstablishingSession { peer_device_id }),

        (
            EstablishingSession {
                peer_device_id: current,
            },
            SessionOpened {
                peer_device_id: incoming,
                challenge,
            },
        ) => {
            if current != incoming {
                return Err(illegal(
                    "SessionOpened carried a different peer_device_id than the in-flight session",
                ));
            }
            Ok(AwaitingUserVerification {
                peer_device_id: current,
                challenge,
            })
        }

        (AwaitingUserVerification { .. }, UserConfirmed { trusted_peer }) => {
            Ok(Trusted { trusted_peer })
        }

        (Idle, UserCancelled | TimedOut | ProtocolError) => {
            Err(illegal("cannot abort while in Idle"))
        }

        (EstablishingSession { .. } | AwaitingUserVerification { .. }, UserCancelled) => {
            Ok(Aborted {
                reason: uc_core::TrustAbortReason::UserCancelled,
            })
        }
        (EstablishingSession { .. } | AwaitingUserVerification { .. }, TimedOut) => Ok(Aborted {
            reason: uc_core::TrustAbortReason::Timeout,
        }),
        (EstablishingSession { .. } | AwaitingUserVerification { .. }, ProtocolError) => {
            Ok(Aborted {
                reason: uc_core::TrustAbortReason::ProtocolError,
            })
        }

        // Any event delivered to a terminal state is rejected; the flow has ended.
        (Trusted { .. }, _) => Err(illegal("flow already reached Trusted terminal state")),
        (Aborted { .. }, _) => Err(illegal("flow already reached Aborted terminal state")),

        // Remaining combinations are non-sequiturs (e.g. SessionOpened while Idle,
        // UserConfirmed while EstablishingSession). Collapse them into one arm.
        (state, event) => Err(illegal(&format!(
            "unexpected event {event:?} in state {state:?}"
        ))),
    }
}

fn illegal(msg: &str) -> TrustedPeerApplicationError {
    TrustedPeerApplicationError::IllegalTransition(msg.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trusted_peer::challenge::TrustVerificationChallenge;
    use chrono::Utc;
    use uc_core::security::IdentityFingerprint;
    use uc_core::{DeviceId, TrustAbortReason, TrustedPeer};

    fn peer(id: &str) -> DeviceId {
        DeviceId::new(id)
    }

    fn fp_for(seed: &str) -> IdentityFingerprint {
        let mut raw: String = seed.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        raw.make_ascii_uppercase();
        while raw.len() < 16 {
            raw.push('A');
        }
        IdentityFingerprint::from_raw_string(&raw[..16]).unwrap()
    }

    fn challenge(fp_seed: &str, code: &str) -> TrustVerificationChallenge {
        TrustVerificationChallenge {
            peer_fingerprint: fp_for(fp_seed),
            short_code: code.into(),
        }
    }

    fn fixture_trusted_peer(peer_id: &str) -> TrustedPeer {
        TrustedPeer {
            local_device_id: peer("local"),
            peer_device_id: peer(peer_id),
            peer_fingerprint: fp_for(&format!("FP{peer_id}")),
            trusted_at: Utc::now(),
        }
    }

    #[test]
    fn happy_path_idle_to_trusted() {
        let s0 = TrustState::Idle;

        let s1 = transition(
            s0,
            TrustStateEvent::Initiate {
                peer_device_id: peer("p1"),
            },
        )
        .unwrap();
        assert!(matches!(s1, TrustState::EstablishingSession { .. }));

        let s2 = transition(
            s1,
            TrustStateEvent::SessionOpened {
                peer_device_id: peer("p1"),
                challenge: challenge("fp", "123-456"),
            },
        )
        .unwrap();
        assert!(matches!(s2, TrustState::AwaitingUserVerification { .. }));

        let tp = fixture_trusted_peer("p1");
        let s3 = transition(
            s2,
            TrustStateEvent::UserConfirmed {
                trusted_peer: tp.clone(),
            },
        )
        .unwrap();
        assert_eq!(s3, TrustState::Trusted { trusted_peer: tp });
    }

    #[test]
    fn session_opened_with_mismatched_peer_is_rejected() {
        let s = TrustState::EstablishingSession {
            peer_device_id: peer("p1"),
        };
        let err = transition(
            s,
            TrustStateEvent::SessionOpened {
                peer_device_id: peer("different"),
                challenge: challenge("fp", "code"),
            },
        )
        .unwrap_err();
        assert!(matches!(
            err,
            TrustedPeerApplicationError::IllegalTransition(_)
        ));
    }

    #[test]
    fn cancel_from_establishing_goes_to_aborted_user_cancelled() {
        let s = TrustState::EstablishingSession {
            peer_device_id: peer("p1"),
        };
        let next = transition(s, TrustStateEvent::UserCancelled).unwrap();
        assert_eq!(
            next,
            TrustState::Aborted {
                reason: TrustAbortReason::UserCancelled
            }
        );
    }

    #[test]
    fn cancel_from_awaiting_verification_goes_to_aborted_user_cancelled() {
        let s = TrustState::AwaitingUserVerification {
            peer_device_id: peer("p1"),
            challenge: challenge("fp", "code"),
        };
        let next = transition(s, TrustStateEvent::UserCancelled).unwrap();
        assert_eq!(
            next,
            TrustState::Aborted {
                reason: TrustAbortReason::UserCancelled
            }
        );
    }

    #[test]
    fn timeout_from_establishing_goes_to_aborted_timeout() {
        let s = TrustState::EstablishingSession {
            peer_device_id: peer("p1"),
        };
        let next = transition(s, TrustStateEvent::TimedOut).unwrap();
        assert_eq!(
            next,
            TrustState::Aborted {
                reason: TrustAbortReason::Timeout
            }
        );
    }

    #[test]
    fn protocol_error_from_awaiting_goes_to_aborted_protocol_error() {
        let s = TrustState::AwaitingUserVerification {
            peer_device_id: peer("p1"),
            challenge: challenge("fp", "code"),
        };
        let next = transition(s, TrustStateEvent::ProtocolError).unwrap();
        assert_eq!(
            next,
            TrustState::Aborted {
                reason: TrustAbortReason::ProtocolError
            }
        );
    }

    #[test]
    fn idle_cannot_abort() {
        for ev in [
            TrustStateEvent::UserCancelled,
            TrustStateEvent::TimedOut,
            TrustStateEvent::ProtocolError,
        ] {
            let err = transition(TrustState::Idle, ev).unwrap_err();
            assert!(matches!(
                err,
                TrustedPeerApplicationError::IllegalTransition(_)
            ));
        }
    }

    #[test]
    fn terminal_trusted_rejects_further_events() {
        let tp = fixture_trusted_peer("p1");
        let s = TrustState::Trusted {
            trusted_peer: tp.clone(),
        };
        let err = transition(
            s,
            TrustStateEvent::Initiate {
                peer_device_id: peer("p2"),
            },
        )
        .unwrap_err();
        assert!(matches!(
            err,
            TrustedPeerApplicationError::IllegalTransition(_)
        ));
    }

    #[test]
    fn terminal_aborted_rejects_further_events() {
        let s = TrustState::Aborted {
            reason: TrustAbortReason::UserCancelled,
        };
        let err = transition(s, TrustStateEvent::UserCancelled).unwrap_err();
        assert!(matches!(
            err,
            TrustedPeerApplicationError::IllegalTransition(_)
        ));
    }

    #[test]
    fn out_of_order_events_are_rejected() {
        // UserConfirmed while in EstablishingSession (must go via AwaitingUserVerification)
        let s = TrustState::EstablishingSession {
            peer_device_id: peer("p1"),
        };
        let err = transition(
            s,
            TrustStateEvent::UserConfirmed {
                trusted_peer: fixture_trusted_peer("p1"),
            },
        )
        .unwrap_err();
        assert!(matches!(
            err,
            TrustedPeerApplicationError::IllegalTransition(_)
        ));

        // SessionOpened while in Idle
        let err2 = transition(
            TrustState::Idle,
            TrustStateEvent::SessionOpened {
                peer_device_id: peer("p1"),
                challenge: challenge("fp", "code"),
            },
        )
        .unwrap_err();
        assert!(matches!(
            err2,
            TrustedPeerApplicationError::IllegalTransition(_)
        ));
    }
}
