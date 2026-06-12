//! Core security domain types shared across pairing / membership / trust.
//!
//! Only algorithm-agnostic value objects live here. Concrete cryptographic
//! derivations (SHA-256, Base32 encoding of public keys, KDFs) belong in
//! `uc-infra`.

pub mod identity_fingerprint;

pub use identity_fingerprint::{FingerprintError, IdentityFingerprint};
