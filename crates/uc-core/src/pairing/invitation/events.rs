//! Pairing invitation domain events.
//!
//! Events describe **facts** that already happened to an invitation. Core
//! emits them; application layer is responsible for turning them into
//! `PairingDomainEvent` variants and publishing to subscribers.

use chrono::{DateTime, Utc};

use crate::DeviceId;

use super::code::InvitationCode;

/// Things that have happened to a pairing invitation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvitationEvent {
    /// Sponsor successfully issued a new invitation.
    Issued {
        code: InvitationCode,
        expires_at: DateTime<Utc>,
        issuer_device_id: DeviceId,
    },

    /// Joiner redeemed the invitation (sponsor matched code on inbound
    /// pairing request).
    Consumed { code: InvitationCode },

    /// Sponsor dropped its local invitation (e.g. before issuing a new one,
    /// or explicitly via UI). Server-side entry is left to expire naturally
    /// (Slice 1 decision Q-B1-3).
    Revoked { code: InvitationCode },

    /// Lazy expiry — `try_expire` observed `now >= expires_at`.
    Expired { code: InvitationCode },
}
