//! Slice 1 iroh-native pairing ports.
//!
//! * [`session`] — joiner `dial_by_invitation` + session-level send/recv/close
//! * [`events`]  — sponsor-side inbound session event stream
//!
//! Replaces [`PairingTransportPort`] + the pairing-specific variants of
//! [`NetworkEventPort`]; both legacy ports are deprecated for Slice 5
//! removal.
//!
//! [`PairingTransportPort`]: crate::ports::pairing_transport::PairingTransportPort
//! [`NetworkEventPort`]: crate::ports::network_events::NetworkEventPort

pub mod events;
pub mod session;

pub use events::{PairingEventPort, PairingSessionEvent};
pub use session::{
    DialError, DialOutcome, DiscoveryChannel, PairingSessionId, PairingSessionPort, SessionError,
};
