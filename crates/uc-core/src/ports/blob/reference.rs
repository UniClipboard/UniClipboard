//! `BlobReferenceRepositoryPort` — plaintext-hash → ciphertext-digest
//! dedup cache, local to the current device.
//!
//! Business scenario: a user copies the same file repeatedly. Every
//! copy would otherwise run through "encrypt → publish to shareable
//! store" again, producing several equivalent ciphertexts and wasting
//! storage. This port records "I have seen this plaintext before" so
//! the next encrypt-and-publish round trip can short-circuit to the
//! ciphertext already held.
//!
//! The cache is **single-active-space** scoped in Phase 1: the same
//! plaintext encrypted under different spaces produces different
//! ciphertexts, so cross-space reuse is unsound. Multi-space support is
//! deferred (Phase 2 decides whether to widen the schema with a
//! `space_id` column; tracked as a candidate tech-debt item).

use async_trait::async_trait;

use super::transfer::BlobDigest;

/// A **plaintext content fingerprint**, used to answer "have I seen this
/// plaintext before".
///
/// 32 opaque bytes. Concrete hashing is the upper layer's concern —
/// typically derived from
/// [`HashPort`](crate::ports::HashPort) and fed into this type without
/// core knowing the algorithm.
///
/// Distinct from [`BlobDigest`]: one is a plaintext identity, the other
/// a ciphertext identity. The same plaintext encrypted under two
/// different spaces yields two different ciphertexts — so plaintext
/// fingerprint → ciphertext identity is a many-to-one mapping **scoped
/// to the currently active space** (see module docs on multi-space).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlaintextHash([u8; 32]);

impl PlaintextHash {
    pub const fn from_bytes(b: [u8; 32]) -> Self {
        Self(b)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Errors produced by [`BlobReferenceRepositoryPort`].
#[derive(Debug, thiserror::Error)]
pub enum BlobReferenceError {
    /// Underlying storage failure (connection / IO / constraint etc.).
    /// Callers usually only record this — a dedup miss simply falls
    /// through to "encrypt and publish as usual", so correctness is not
    /// affected.
    #[error("repository error: {0}")]
    Repository(String),
}

/// Plaintext-fingerprint ↔ ciphertext-identity dedup cache.
///
/// Write sources:
/// * After the first local publish of some new plaintext, record the
///   mapping so the next copy of the same plaintext can reuse the
///   existing ciphertext.
/// * After pulling and decrypting someone else's content, record the
///   mapping too — this sets up the "this device may later act as
///   forwarder" path tracked in `task_plan.md §T-03`.
///
/// Read sources: every time the device is about to encrypt and publish
/// some content, look up here first — a hit skips the crypto + publish
/// and reuses the existing ciphertext identity directly for credential
/// issuance.
#[async_trait]
pub trait BlobReferenceRepositoryPort: Send + Sync {
    /// Look up whether a plaintext has been published before on this
    /// device. Returns the corresponding ciphertext identity on hit,
    /// `None` on miss.
    async fn find_by_plaintext_hash(
        &self,
        hash: &PlaintextHash,
    ) -> Result<Option<BlobDigest>, BlobReferenceError>;

    /// Record a (plaintext-fingerprint → ciphertext-identity) mapping.
    /// Re-saving the same fingerprint overwrites — re-encrypting the
    /// same plaintext with a fresh nonce yields a different ciphertext,
    /// and subsequent dedup lookups should prefer the most recent one.
    async fn save(&self, hash: PlaintextHash, digest: BlobDigest)
        -> Result<(), BlobReferenceError>;

    /// Forget a mapping. Only removes the lookup record — the
    /// ciphertext itself in the shareable store is released through
    /// [`BlobTransferPort::untag`] + reclaim scanning, **not** here.
    /// Typical use: user explicitly deletes the plaintext content.
    ///
    /// [`BlobTransferPort::untag`]: super::transfer::BlobTransferPort::untag
    async fn forget(&self, hash: &PlaintextHash) -> Result<(), BlobReferenceError>;
}
