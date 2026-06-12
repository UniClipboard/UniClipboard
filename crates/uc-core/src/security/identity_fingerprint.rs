//! Stable, human-readable fingerprint identifying a device across transports
//! and restarts. Used to answer "is this still the same peer?".
//!
//! Carries 16 Base32 characters grouped as `ABCD-EFGH-IJKL-MNOP`. The exact
//! derivation (domain separator + SHA-256 over the Ed25519 public key + Base32
//! truncation) lives in `uc-infra::security` and goes through
//! `IdentityFingerprintFactoryPort`. This value object only validates shape
//! and exposes display/raw/verify accessors.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors produced when constructing or comparing fingerprints.
///
/// Algorithm-level failures (e.g. wrong public key length) are represented
/// in the concrete factory implementation's error type, not here.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FingerprintError {
    #[error("invalid fingerprint format: {0}")]
    InvalidFormat(String),
    #[error("fingerprint mismatch")]
    Mismatch,
}

/// Canonical fingerprint of a device's long-term identity public key.
///
/// Stored as the grouped display form `ABCD-EFGH-IJKL-MNOP`. Equality is
/// over the raw (ungrouped) characters so values constructed via
/// `from_raw_string` and `from_display_string` compare equal when the
/// underlying 16 characters match.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityFingerprint(String);

impl IdentityFingerprint {
    const GROUP_SIZE: usize = 4;
    const RAW_LEN: usize = 16;

    /// Construct from raw 16 Base32 characters (no separators).
    pub fn from_raw_string(raw: impl AsRef<str>) -> Result<Self, FingerprintError> {
        let raw = raw.as_ref();
        if raw.len() != Self::RAW_LEN {
            return Err(FingerprintError::InvalidFormat(format!(
                "expected {} characters, got {}",
                Self::RAW_LEN,
                raw.len()
            )));
        }
        if !raw.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(FingerprintError::InvalidFormat(
                "non-alphanumeric characters found".to_string(),
            ));
        }
        Ok(Self(Self::format_with_groups(raw)))
    }

    /// Construct from either raw (`ABCDEFGHIJKLMNOP`) or grouped
    /// (`ABCD-EFGH-IJKL-MNOP`) display form.
    pub fn from_display_string(s: impl AsRef<str>) -> Result<Self, FingerprintError> {
        let cleaned: String = s.as_ref().chars().filter(|c| *c != '-').collect();
        Self::from_raw_string(cleaned)
    }

    /// Raw 16 characters without grouping separators.
    pub fn as_raw(&self) -> String {
        self.0.replace('-', "")
    }

    /// Grouped display form `ABCD-EFGH-IJKL-MNOP`.
    pub fn as_display(&self) -> &str {
        &self.0
    }

    /// Compare two fingerprints by their raw characters.
    pub fn verify(&self, other: &IdentityFingerprint) -> Result<(), FingerprintError> {
        if self.as_raw() == other.as_raw() {
            Ok(())
        } else {
            Err(FingerprintError::Mismatch)
        }
    }

    fn format_with_groups(raw: &str) -> String {
        raw.as_bytes()
            .chunks(Self::GROUP_SIZE)
            .map(|chunk| std::str::from_utf8(chunk).unwrap_or(""))
            .collect::<Vec<_>>()
            .join("-")
    }
}

impl PartialEq for IdentityFingerprint {
    fn eq(&self, other: &Self) -> bool {
        self.as_raw() == other.as_raw()
    }
}

impl Eq for IdentityFingerprint {}

impl std::hash::Hash for IdentityFingerprint {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_raw().hash(state);
    }
}

impl fmt::Display for IdentityFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for IdentityFingerprint {
    type Err = FingerprintError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_display_string(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_raw_string_accepts_16_alphanumeric() {
        let fp = IdentityFingerprint::from_raw_string("ABCDEFGHIJKLMNOP").unwrap();
        assert_eq!(fp.as_display(), "ABCD-EFGH-IJKL-MNOP");
        assert_eq!(fp.as_raw(), "ABCDEFGHIJKLMNOP");
    }

    #[test]
    fn from_raw_string_rejects_wrong_length() {
        let err = IdentityFingerprint::from_raw_string("TOOSHORT").unwrap_err();
        assert!(matches!(err, FingerprintError::InvalidFormat(_)));
    }

    #[test]
    fn from_raw_string_rejects_non_alphanumeric() {
        let err = IdentityFingerprint::from_raw_string("ABCD-EFGH-IJKL-MN").unwrap_err();
        assert!(matches!(err, FingerprintError::InvalidFormat(_)));
    }

    #[test]
    fn from_display_string_accepts_grouped_form() {
        let fp = IdentityFingerprint::from_display_string("ABCD-EFGH-IJKL-MNOP").unwrap();
        assert_eq!(fp.as_raw(), "ABCDEFGHIJKLMNOP");
    }

    #[test]
    fn equality_ignores_grouping() {
        let raw = IdentityFingerprint::from_raw_string("ABCDEFGHIJKLMNOP").unwrap();
        let grouped = IdentityFingerprint::from_display_string("ABCD-EFGH-IJKL-MNOP").unwrap();
        assert_eq!(raw, grouped);
    }

    #[test]
    fn verify_matches_same_raw() {
        let a = IdentityFingerprint::from_raw_string("ABCDEFGHIJKLMNOP").unwrap();
        let b = IdentityFingerprint::from_display_string("ABCD-EFGH-IJKL-MNOP").unwrap();
        assert!(a.verify(&b).is_ok());
    }

    #[test]
    fn verify_rejects_different_raw() {
        let a = IdentityFingerprint::from_raw_string("ABCDEFGHIJKLMNOP").unwrap();
        let b = IdentityFingerprint::from_raw_string("QRSTUVWXYZ234567").unwrap();
        assert!(matches!(a.verify(&b), Err(FingerprintError::Mismatch)));
    }

    #[test]
    fn from_str_trait_delegates_to_display_form() {
        let fp: IdentityFingerprint = "ABCD-EFGH-IJKL-MNOP".parse().unwrap();
        assert_eq!(fp.as_raw(), "ABCDEFGHIJKLMNOP");
    }

    #[test]
    fn display_shows_grouped_form() {
        let fp = IdentityFingerprint::from_raw_string("ABCDEFGHIJKLMNOP").unwrap();
        assert_eq!(format!("{fp}"), "ABCD-EFGH-IJKL-MNOP");
    }
}
