use thiserror::Error;

use crate::ids::DeviceId;

/// Boundary error for the membership domain.
///
/// Infrastructure adapters map their internal failures (DB, I/O, etc.)
/// into `Repository` when crossing the port boundary. Use cases surface
/// `AlreadyAdmitted` and `NotFound` based on the business semantics they
/// enforce on top of the (thin) repository port.
#[derive(Debug, Error)]
pub enum MembershipError {
    #[error("member `{0}` has already been admitted")]
    AlreadyAdmitted(DeviceId),

    #[error("member `{0}` not found")]
    NotFound(DeviceId),

    #[error("membership repository failure: {0}")]
    Repository(String),
}
