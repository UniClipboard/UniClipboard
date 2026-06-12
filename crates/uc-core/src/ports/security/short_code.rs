//! Short pairing-code generation port.
//!
//! Produces a short, human-comparable confirmation code derived from a
//! pairing-session transcript. Implementations must be deterministic for
//! given inputs so both peers compute the same value.
//!
//! Concrete implementation (SHA-256 + Base32) lives in `uc-infra`.

use anyhow::Result;

pub trait ShortCodeGeneratorPort: Send + Sync {
    fn generate(
        &self,
        session_id: &str,
        nonce_initiator: &[u8],
        nonce_responder: &[u8],
        initiator_pubkey: &[u8],
        responder_pubkey: &[u8],
        protocol_version: &str,
    ) -> Result<String>;
}
