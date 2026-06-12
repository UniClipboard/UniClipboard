mod error;
mod events;
mod peer;
mod ports;

pub use error::TrustedPeerError;
pub use events::{TrustAbortReason, TrustedPeerEvent};
pub use peer::TrustedPeer;
pub use ports::TrustedPeerRepositoryPort;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::DeviceId;

    #[test]
    fn error_already_trusted_formats_device_id() {
        let err = TrustedPeerError::AlreadyTrusted(DeviceId::new("dev-001"));
        assert_eq!(err.to_string(), "peer `dev-001` is already trusted");
    }

    #[test]
    fn error_not_found_formats_device_id() {
        let err = TrustedPeerError::NotFound(DeviceId::new("dev-002"));
        assert_eq!(err.to_string(), "trusted peer `dev-002` not found");
    }

    #[test]
    fn error_repository_formats_underlying_message() {
        let err = TrustedPeerError::Repository("disk offline".into());
        assert_eq!(
            err.to_string(),
            "trusted-peer repository failure: disk offline"
        );
    }

    #[test]
    fn trust_abort_reason_variants_are_distinct() {
        assert_ne!(TrustAbortReason::UserCancelled, TrustAbortReason::Timeout);
        assert_ne!(TrustAbortReason::Timeout, TrustAbortReason::ProtocolError);
        assert_ne!(
            TrustAbortReason::UserCancelled,
            TrustAbortReason::ProtocolError
        );
    }
}
