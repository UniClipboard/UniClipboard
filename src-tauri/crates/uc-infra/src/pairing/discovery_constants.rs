//! Constants shared by the mDNS pairing publisher and resolver.
//!
//! Kept in one place so the on-wire contract (service name + TXT field
//! names) stays in lockstep across the two sides. A drift here would
//! manifest as "sponsor announces but joiner never matches" — silent and
//! hard to debug.
//!
//! ## Service-name isolation
//!
//! Pairing uses its **own** mDNS service name distinct from iroh's
//! `_irohv1._udp.local` peer-discovery flow. Reasons:
//!
//! * Pairing announces are window-scoped (only while a code is pending),
//!   whereas iroh peer discovery runs for the endpoint's lifetime. Mixing
//!   them would force the iroh discovery loop to filter pairing entries
//!   on every browse.
//! * Pairing TXT records carry different fields (`code_hash`, opaque
//!   ticket) that have no meaning to iroh's NodeId→IP loop.
//! * Privacy: pairing announces are deliberately scarce so a passive LAN
//!   observer cannot fingerprint a device by its pairing history.

/// mDNS service name pairing announces are published under.
///
/// `swarm_discovery::Discoverer::new` takes a bare service name; the crate
/// then appends `._udp.local` internally. So this constant is the bare
/// service part, **not** the fully-qualified DNS name.
pub const PAIR_SERVICE_NAME: &str = "uniclipboard-pair";

/// TXT key carrying `hex(blake3(code)[..8])` — a short hash prefix used by
/// the joiner to filter matching announces before fetching the heavier
/// ticket field. Hashing instead of broadcasting the raw code keeps a
/// passive observer from learning the code itself.
pub const TXT_CODE_HASH: &str = "ch";

/// TXT key carrying the sponsor's endpoint id (hex string). Adapters use
/// this to short-circuit "yes this is me" loops when the publisher and
/// resolver coexist in the same process during tests.
pub const TXT_NODE_ID: &str = "id";

/// TXT key carrying the postcard+hex-encoded sponsor ticket
/// (`iroh::EndpointAddr`). The joiner decodes this directly into a
/// dialable address. Encoding is hex of the postcard bytes to keep the
/// TXT value ASCII (mDNS TXT records are technically binary-safe but
/// hickory and routers along the path are happier with printable values).
pub const TXT_TICKET: &str = "tk";

/// TXT key carrying the code's `expires_at_ms` as a decimal string.
/// Joiner uses this to filter out stale announces that linger past the
/// publisher's window (the publisher should stop announcing on expiry,
/// but mDNS cache propagation gives a small grace window).
pub const TXT_EXPIRES_AT_MS: &str = "ex";

/// Computes the short hash prefix broadcast in the `code_hash` TXT field.
///
/// 8 bytes ≈ 64 bits of entropy. Returned as lowercase hex so it round
/// trips through `swarm_discovery::Peer::txt_attribute` without case
/// normalisation surprises.
pub fn compute_code_hash(code: &str) -> String {
    let digest = blake3::hash(code.as_bytes());
    hex::encode(&digest.as_bytes()[..8])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_hash_is_stable_and_16_hex_chars() {
        let h1 = compute_code_hash("ABCD-1234");
        let h2 = compute_code_hash("ABCD-1234");
        assert_eq!(h1, h2, "hash must be deterministic");
        assert_eq!(h1.len(), 16, "8 bytes -> 16 hex chars");
        assert!(
            h1.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "hash must be lowercase hex"
        );
    }

    #[test]
    fn code_hash_differs_for_different_codes() {
        assert_ne!(
            compute_code_hash("ABCD-1234"),
            compute_code_hash("ABCD-1235"),
        );
    }
}
