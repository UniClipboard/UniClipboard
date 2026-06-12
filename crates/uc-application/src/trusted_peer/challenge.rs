use uc_core::security::IdentityFingerprint;

/// Presentation-facing artefact produced when the trust flow is awaiting
/// user verification. Lives in the application layer — not in the core
/// domain — because `short_code` is a derived presentation form (DOMAIN §5.3 / T5).
///
/// `short_code` is produced by the `network::pairing` protocol layer from
/// the canonical `peer_fingerprint`; the orchestrator only forwards it.
/// Future media (QR, NFC, …) are added here as optional fields without
/// touching the domain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustVerificationChallenge {
    pub peer_fingerprint: IdentityFingerprint,
    pub short_code: String,
}
