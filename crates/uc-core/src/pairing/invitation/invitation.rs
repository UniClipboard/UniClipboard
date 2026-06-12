//! `PairingInvitation` aggregate — sponsor-side representation of a
//! single outstanding pairing credential.
//!
//! Core-only business rules centralised here (Slice 1 decision Q-1):
//!
//! * Lifecycle: `Pending → Consumed | Revoked | Expired` (terminal states).
//! * TTL comparison against a caller-supplied `now` (injected through
//!   `ClockPort` at the application layer; core stays time-agnostic).
//! * Code-match check lives on the aggregate so the only place that
//!   compares invitation codes is the one that owns them.
//!
//! Persistence: **none** (Slice 1 decision Q-2 — kept in-memory by the
//! application, dropped on process exit). The aggregate therefore has no
//! serde derives on the whole struct; callers that need to pass a
//! snapshot across boundaries should build application-level DTOs.

use chrono::{DateTime, Utc};

use crate::DeviceId;

use super::code::InvitationCode;
use super::error::{ConsumeError, RevokeError};
use super::events::InvitationEvent;

/// Lifecycle state of a single `PairingInvitation`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvitationState {
    /// Freshly issued; waiting for a joiner to redeem or for TTL to elapse.
    Pending { expires_at: DateTime<Utc> },
    /// Redeemed by a joiner (`consume` accepted).
    Consumed,
    /// Dropped by sponsor before consume (e.g. replaced by a new issue).
    Revoked,
    /// Discovered to be past `expires_at` during a lazy check.
    Expired,
}

/// Sponsor-side aggregate for one outstanding invitation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingInvitation {
    code: InvitationCode,
    issued_at: DateTime<Utc>,
    issuer_device_id: DeviceId,
    state: InvitationState,
}

impl PairingInvitation {
    /// Construct a fresh invitation in `Pending` state and pair it with the
    /// `Issued` domain event so publishers cannot forget to emit.
    pub fn issue(
        code: InvitationCode,
        issued_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
        issuer_device_id: DeviceId,
    ) -> (Self, InvitationEvent) {
        let event = InvitationEvent::Issued {
            code: code.clone(),
            expires_at,
            issuer_device_id: issuer_device_id.clone(),
        };
        let invitation = Self {
            code,
            issued_at,
            issuer_device_id,
            state: InvitationState::Pending { expires_at },
        };
        (invitation, event)
    }

    pub fn code(&self) -> &InvitationCode {
        &self.code
    }

    pub fn issued_at(&self) -> DateTime<Utc> {
        self.issued_at
    }

    pub fn issuer_device_id(&self) -> &DeviceId {
        &self.issuer_device_id
    }

    pub fn state(&self) -> &InvitationState {
        &self.state
    }

    pub fn is_pending(&self) -> bool {
        matches!(self.state, InvitationState::Pending { .. })
    }

    /// Attempt to consume this invitation with the code presented by a
    /// joiner. On success the state flips to `Consumed` and a `Consumed`
    /// event is returned.
    pub fn consume(
        &mut self,
        incoming_code: &InvitationCode,
        now: DateTime<Utc>,
    ) -> Result<InvitationEvent, ConsumeError> {
        match self.state {
            InvitationState::Pending { expires_at } => {
                if now >= expires_at {
                    return Err(ConsumeError::Expired);
                }
                if &self.code != incoming_code {
                    return Err(ConsumeError::CodeMismatch);
                }
                self.state = InvitationState::Consumed;
                Ok(InvitationEvent::Consumed {
                    code: self.code.clone(),
                })
            }
            _ => Err(ConsumeError::NotPending),
        }
    }

    /// Sponsor-side local revoke. Returns an error when called on a
    /// non-pending invitation so the caller can distinguish "I just
    /// revoked it" from "it was already done".
    pub fn revoke(&mut self) -> Result<InvitationEvent, RevokeError> {
        match self.state {
            InvitationState::Pending { .. } => {
                self.state = InvitationState::Revoked;
                Ok(InvitationEvent::Revoked {
                    code: self.code.clone(),
                })
            }
            _ => Err(RevokeError::NotPending),
        }
    }

    /// Observe TTL elapse lazily. Returns `Some(Expired)` exactly once (the
    /// transition `Pending → Expired`); subsequent calls return `None`.
    pub fn try_expire(&mut self, now: DateTime<Utc>) -> Option<InvitationEvent> {
        if let InvitationState::Pending { expires_at } = self.state {
            if now >= expires_at {
                self.state = InvitationState::Expired;
                return Some(InvitationEvent::Expired {
                    code: self.code.clone(),
                });
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn fixed_issuer() -> DeviceId {
        DeviceId::new("11111111-1111-4111-8111-111111111111")
    }

    fn fixed_now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-04-19T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn pending_invitation() -> (PairingInvitation, InvitationEvent) {
        let issued = fixed_now();
        let expires = issued + Duration::minutes(5);
        PairingInvitation::issue(
            InvitationCode::new("ABCD-1234"),
            issued,
            expires,
            fixed_issuer(),
        )
    }

    #[test]
    fn issue_emits_issued_event_and_starts_pending() {
        let (inv, event) = pending_invitation();
        assert!(inv.is_pending());
        match event {
            InvitationEvent::Issued {
                code,
                expires_at,
                issuer_device_id,
            } => {
                assert_eq!(code.as_str(), "ABCD-1234");
                assert_eq!(expires_at, fixed_now() + Duration::minutes(5));
                assert_eq!(issuer_device_id, fixed_issuer());
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn consume_matching_code_transitions_to_consumed() {
        let (mut inv, _) = pending_invitation();
        let event = inv
            .consume(&InvitationCode::new("ABCD-1234"), fixed_now())
            .expect("consume should succeed");
        assert_eq!(inv.state(), &InvitationState::Consumed);
        assert_eq!(
            event,
            InvitationEvent::Consumed {
                code: InvitationCode::new("ABCD-1234"),
            }
        );
    }

    #[test]
    fn consume_wrong_code_is_mismatch_and_keeps_pending() {
        let (mut inv, _) = pending_invitation();
        let err = inv
            .consume(&InvitationCode::new("WRONG"), fixed_now())
            .unwrap_err();
        assert_eq!(err, ConsumeError::CodeMismatch);
        assert!(inv.is_pending());
    }

    #[test]
    fn consume_after_expiry_is_expired_error() {
        let (mut inv, _) = pending_invitation();
        let later = fixed_now() + Duration::minutes(10);
        let err = inv
            .consume(&InvitationCode::new("ABCD-1234"), later)
            .unwrap_err();
        assert_eq!(err, ConsumeError::Expired);
        assert!(inv.is_pending(), "expiry is lazy and not self-triggered");
    }

    #[test]
    fn consume_on_already_consumed_is_not_pending() {
        let (mut inv, _) = pending_invitation();
        inv.consume(&InvitationCode::new("ABCD-1234"), fixed_now())
            .unwrap();
        let err = inv
            .consume(&InvitationCode::new("ABCD-1234"), fixed_now())
            .unwrap_err();
        assert_eq!(err, ConsumeError::NotPending);
    }

    #[test]
    fn revoke_from_pending_returns_revoked_event() {
        let (mut inv, _) = pending_invitation();
        let event = inv.revoke().expect("revoke should succeed");
        assert_eq!(inv.state(), &InvitationState::Revoked);
        assert_eq!(
            event,
            InvitationEvent::Revoked {
                code: InvitationCode::new("ABCD-1234"),
            }
        );
    }

    #[test]
    fn revoke_on_consumed_fails_not_pending() {
        let (mut inv, _) = pending_invitation();
        inv.consume(&InvitationCode::new("ABCD-1234"), fixed_now())
            .unwrap();
        assert_eq!(inv.revoke().unwrap_err(), RevokeError::NotPending);
    }

    #[test]
    fn try_expire_before_ttl_returns_none() {
        let (mut inv, _) = pending_invitation();
        let early = fixed_now() + Duration::minutes(1);
        assert!(inv.try_expire(early).is_none());
        assert!(inv.is_pending());
    }

    #[test]
    fn try_expire_after_ttl_transitions_once() {
        let (mut inv, _) = pending_invitation();
        let late = fixed_now() + Duration::minutes(10);
        let first = inv.try_expire(late);
        assert_eq!(
            first,
            Some(InvitationEvent::Expired {
                code: InvitationCode::new("ABCD-1234"),
            })
        );
        assert_eq!(inv.state(), &InvitationState::Expired);
        // Second call is idempotent no-op.
        assert!(inv.try_expire(late).is_none());
    }

    #[test]
    fn consume_boundary_exactly_at_expiry_is_expired() {
        let (mut inv, _) = pending_invitation();
        let boundary = fixed_now() + Duration::minutes(5);
        assert_eq!(
            inv.consume(&InvitationCode::new("ABCD-1234"), boundary)
                .unwrap_err(),
            ConsumeError::Expired
        );
    }
}
