//! Blob transfer + reference ports (Slice 3 Phase 1).
//!
//! Two orthogonal concerns, one per submodule:
//!
//! * [`transfer`] — publish / fetch ciphertext across devices, plus local
//!   reference-holding (tag / untag).
//! * [`reference`] — plaintext-hash → ciphertext-digest cache on the
//!   current device, for skip-re-encrypt dedup.
//!
//! Both ports deliberately avoid naming any storage backend, hash
//! algorithm, or wire format — those live in infra adapters.

pub mod reference;
pub mod transfer;

pub use reference::{BlobReferenceError, BlobReferenceRepositoryPort, PlaintextHash};
pub use transfer::{
    BlobDigest, BlobError, BlobProgressSink, BlobTicket, BlobTransferPort, TagReason,
};
