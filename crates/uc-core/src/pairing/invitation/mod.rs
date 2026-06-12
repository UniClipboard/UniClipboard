//! Pairing invitation aggregate.
//!
//! Sponsor-side domain objects that model "one short-lived credential a
//! joiner can redeem to dial this device". Lifecycle and TTL enforcement
//! live entirely in core; adapters only carry the opaque
//! [`InvitationCode`] across the wire.

mod code;
mod error;
mod events;
#[allow(clippy::module_inception)]
mod invitation;

pub use code::InvitationCode;
pub use error::{ConsumeError, RevokeError};
pub use events::InvitationEvent;
pub use invitation::{InvitationState, PairingInvitation};
