use thiserror::Error;

use crate::ids::DeviceId;

/// Boundary error for the trusted-peer domain.
///
/// Infrastructure adapters map their internal failures (DB, I/O, etc.) into
/// `Repository` when crossing the port boundary. Use cases layer
/// `AlreadyTrusted` / `NotFound` on top based on business semantics.
#[derive(Debug, Error)]
pub enum TrustedPeerError {
    #[error("peer `{0}` is already trusted")]
    AlreadyTrusted(DeviceId),

    #[error("trusted peer `{0}` not found")]
    NotFound(DeviceId),

    #[error("trusted-peer repository failure: {0}")]
    Repository(String),
}
