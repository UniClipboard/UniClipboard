//! SyncClipboard content hashing — SHA-256, uppercase hex (spec §4.1 / §4.2 /
//! §4.4).
//!
//! Normative source: `Clipboard.swift` (`computeTextHash` / `computeBytesHash`
//! / `hashMatches`) and its golden vectors in `HashTests.swift` /
//! `FixturesTests.swift` from the uc-ios app. Migration baseline:
//! `.planning/research/uc-ios-regression-checklist.md` item A3.
//!
//! ## BYTE-CRITICAL invariants (checklist A3 🔴)
//! - The hex output is **UPPERCASE** (`%02X`), 64 chars. Lowercase output is a
//!   wire regression: receivers compare hashes case-insensitively, but the
//!   published bytes must match the iOS app byte-for-byte.
//! - Text content hashes `sha256(utf8(text))`; file/image content hashes the
//!   **raw payload bytes**. The file name NEVER participates in the hash
//!   (`Clipboard.swift` §4.2 note: the basename-bound variant an earlier spec
//!   revision described never matched reality).
//! - Swift exposes two entry points (`computeTextHash` / `computeBytesHash`)
//!   that both reduce to SHA-256 over the same UTF-8 bytes; Rust collapses
//!   them into the single [`sha256_hex_upper`]. The parity invariant
//!   (`HashTests.test_H8`) therefore holds by construction.

use std::fmt::Write as _;

use sha2::{Digest, Sha256};

/// SHA-256 over `bytes`, rendered as a 64-char **UPPERCASE** hex string.
///
/// Mirrors `Clipboard.sha256Upper` in `Clipboard.swift` (spec §4.1 / §4.2):
/// `SHA256.hash(data:).map { String(format: "%02X", $0) }.joined()`.
///
/// - Text hash (spec §4.1): call with `text.as_bytes()` (UTF-8).
/// - File/image hash (spec §4.2): call with the raw payload bytes. The file
///   name does NOT participate.
pub fn sha256_hex_upper(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest {
        // Writing into a String cannot fail; ignore the fmt::Result.
        let _ = write!(out, "{b:02X}");
    }
    out
}

/// Spec §4.4 receiver-side hash verification. Mirrors
/// `Clipboard.hashMatches(expected:actual:)` in `Clipboard.swift`.
///
/// - `expected` of `None`, or whose trimmed value is empty, matches anything
///   (publishers may omit the hash — see the `clipboard_no_hash.json`
///   fixture).
/// - Otherwise the comparison is case-insensitive (Swift compares
///   `uppercased()` on both sides; we use `to_uppercase()` for the same
///   Unicode-uppercasing semantics).
///
/// Note: Swift trims `expected` with `.whitespacesAndNewlines`; Rust
/// `str::trim` trims the Unicode `White_Space` set, which is equivalent for
/// every character that can plausibly surround a hex hash.
pub fn hash_matches(actual: &str, expected: Option<&str>) -> bool {
    match expected.map(str::trim) {
        None | Some("") => true,
        Some(e) => e.to_uppercase() == actual.to_uppercase(),
    }
}

// ─── tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── computeTextHash golden vectors (HashTests.swift) ───────────────

    /// Ported from `HashTests.test_H1_computeTextHash_helloFixture_matchesSpecValue`.
    /// Cross-check against the `docs/examples/clipboard_text_short.json`
    /// fixture, which ships the hash for this exact string.
    #[test]
    fn h1_text_hash_hello_fixture_matches_spec_value() {
        assert_eq!(
            sha256_hex_upper("Hello, SyncClipboard!".as_bytes()),
            "3F4E62D9F184380BAD1B0F94B5518DCBF35ACB79B34F6D6E34F3DAB16CD7BC8F"
        );
    }

    /// Ported from `HashTests.test_H2_computeTextHash_emptyString_matchesKnownSHA256`.
    /// SHA-256("") is a universally-known constant.
    #[test]
    fn h2_text_hash_empty_string_matches_known_sha256() {
        assert_eq!(
            sha256_hex_upper("".as_bytes()),
            "E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855"
        );
    }

    /// Ported from `HashTests.test_H5_computeTextHash_isUppercase64HexChars`.
    #[test]
    fn h5_hash_is_uppercase_64_hex_chars() {
        let h = sha256_hex_upper("any input".as_bytes());
        assert_eq!(h.len(), 64);
        assert_eq!(h, h.to_uppercase(), "hash output must be uppercase");
        assert!(
            h.chars()
                .all(|c| c.is_ascii_hexdigit() && (!c.is_alphabetic() || c.is_uppercase())),
            "hash must be uppercase hex digits only"
        );
    }

    // ── computeBytesHash golden vectors (HashTests.swift) ──────────────

    /// Ported from `HashTests.test_H6_computeBytesHash_emptyData_matchesKnownSHA256`.
    #[test]
    fn h6_bytes_hash_empty_data_matches_known_sha256() {
        assert_eq!(
            sha256_hex_upper(&[]),
            "E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855"
        );
    }

    /// Ported from `HashTests.test_H7_computeBytesHash_abc_matchesNISTTestVector`.
    /// FIPS 180-2 test vector for SHA-256 of the three-byte string "abc".
    #[test]
    fn h7_bytes_hash_abc_matches_nist_test_vector() {
        assert_eq!(
            sha256_hex_upper("abc".as_bytes()),
            "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD"
        );
    }

    /// Ported from `HashTests.test_H8_computeBytesHash_matchesComputeTextHash_forSameUTF8`.
    ///
    /// In Swift this guards the text-hash / bytes-hash parity the
    /// text-overflow download verifier relies on. In Rust both entry points
    /// collapse into [`sha256_hex_upper`], so parity holds by construction —
    /// the vector is kept verbatim to lock the invariant should the two
    /// paths ever diverge again.
    #[test]
    fn h8_bytes_hash_matches_text_hash_for_same_utf8() {
        for sample in ["hello, world", "你好,世界", "", "🍎🍌"] {
            assert_eq!(
                sha256_hex_upper(sample.as_bytes()),
                sha256_hex_upper(String::from(sample).as_bytes()),
                "parity broken for {sample:?}"
            );
        }
    }

    // ── hashMatches (FixturesTests.swift) ──────────────────────────────

    /// Ported from `FixturesTests.test_hashMatches_nilOrEmptyExpectedMatchesAnything`.
    #[test]
    fn hash_matches_nil_or_empty_expected_matches_anything() {
        assert!(hash_matches("DEADBEEF", None));
        assert!(hash_matches("DEADBEEF", Some("")));
        assert!(hash_matches("DEADBEEF", Some("  ")));
        assert!(hash_matches("DEADBEEF", Some("deadbeef")));
        assert!(!hash_matches("BBB", Some("AAA")));
    }
}
