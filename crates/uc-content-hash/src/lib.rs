//! Content-identity hashing shared by every UniClipboard implementation
//! (desktop core, mobile FFI). This is the single source of truth for the
//! `blake3v1:<hex>` cross-device content identity: any implementation that
//! computes this hash over the same bytes must reach the exact same value,
//! so the algorithm lives here once instead of being ported per platform.
//!
//! No domain types, no async, no serde — just the pure hash functions, so
//! this crate stays trivially cross-compilable (e.g. into a mobile FFI
//! target) without pulling in a larger dependency graph.
//!
//! ## Algorithm
//!
//! A representation's content hash is a plain `blake3` digest of its bytes
//! ([`content_hash`] / [`content_hash_reader`]).
//!
//! A snapshot's identity aggregates one or more representation hashes:
//! sort them, then `blake3` them under a fixed domain-separation prefix
//! ([`snapshot_hash`]). A file-list representation contributes an
//! intermediate digest of its per-file content hashes instead of its raw
//! (device-local-path-bearing) bytes ([`file_content_wrapper`]).
//!
//! Domain separation prefixes are fixed string literals with no secret
//! material — the whole algorithm is a pure function of content bytes.

use std::io::{self, Read};

/// Buffer size for [`content_hash_reader`]'s chunked reads.
const STREAM_CHUNK_SIZE: usize = 64 * 1024;

/// Domain-separation prefix for [`file_content_wrapper`].
const FILE_CONTENT_PREFIX: &[u8] = b"file-content|";

/// Domain-separation prefix for a structured file-set member leaf.
const FILE_SET_MEMBER_V1_PREFIX: &[u8] = b"file-set-member-v1|";

/// Domain-separation prefix for a structured file-set component.
const FILE_SET_V1_PREFIX: &[u8] = b"file-set-v1|";

/// Domain-separation prefix for [`snapshot_hash`].
const SNAPSHOT_HASH_PREFIX: &[u8] = b"snapshot-hash-v1|";

/// `blake3` digest of `bytes` — a single representation's content hash.
pub fn content_hash(bytes: &[u8]) -> [u8; 32] {
    *blake3::hash(bytes).as_bytes()
}

/// `blake3` digest of a reader's full contents, read in bounded chunks so the
/// caller never needs to buffer the whole input in memory.
pub fn content_hash_reader(mut reader: impl Read) -> io::Result<[u8; 32]> {
    let mut hasher = blake3::Hasher::new();
    let mut buf = [0u8; STREAM_CHUNK_SIZE];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(*hasher.finalize().as_bytes())
}

/// Aggregate per-file content digests into the single hash a file-list
/// representation contributes to [`snapshot_hash`], in place of hashing its
/// raw (device-local-path-bearing) bytes directly.
///
/// `digests` need not be pre-sorted — this function sorts an internal copy,
/// so the result is independent of input order.
pub fn file_content_wrapper(digests: &[[u8; 32]]) -> [u8; 32] {
    let mut sorted = digests.to_vec();
    sorted.sort_unstable();
    let mut hasher = blake3::Hasher::new();
    hasher.update(FILE_CONTENT_PREFIX);
    for d in &sorted {
        hasher.update(d);
    }
    *hasher.finalize().as_bytes()
}

/// Hash one structured file-set member using the ADR-010 v1 formula.
pub fn file_set_member_v1(
    root_name: &str,
    relative_path: &str,
    kind_tag: &str,
    file_digest: Option<&[u8; 32]>,
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(FILE_SET_MEMBER_V1_PREFIX);
    hasher.update(root_name.as_bytes());
    hasher.update(&[0]);
    hasher.update(relative_path.as_bytes());
    hasher.update(&[0]);
    hasher.update(kind_tag.as_bytes());
    hasher.update(&[0]);
    if let Some(file_digest) = file_digest {
        hasher.update(file_digest);
    }
    *hasher.finalize().as_bytes()
}

/// Aggregate structured member leaves into an ADR-010 v1 file-set component.
pub fn file_set_v1_wrapper(leaves: &[[u8; 32]]) -> [u8; 32] {
    let mut sorted = leaves.to_vec();
    sorted.sort_unstable();
    let mut hasher = blake3::Hasher::new();
    hasher.update(FILE_SET_V1_PREFIX);
    for leaf in &sorted {
        hasher.update(leaf);
    }
    *hasher.finalize().as_bytes()
}

/// Aggregate one or more representation hashes into a snapshot's stable
/// cross-device content identity.
///
/// Order-independent: the hashes are sorted internally, so callers do not
/// need to agree on representation ordering to reach the same result.
pub fn snapshot_hash(rep_hashes: impl IntoIterator<Item = [u8; 32]>) -> [u8; 32] {
    let mut hashes: Vec<[u8; 32]> = rep_hashes.into_iter().collect();
    hashes.sort_unstable();
    let mut hasher = blake3::Hasher::new();
    hasher.update(SNAPSHOT_HASH_PREFIX);
    for h in &hashes {
        hasher.update(h);
    }
    *hasher.finalize().as_bytes()
}

/// Format a 32-byte digest as the wire identity string `blake3v1:<hex>`.
pub fn format_blake3v1(bytes: &[u8; 32]) -> String {
    format!("blake3v1:{}", hex::encode(bytes))
}

/// Convenience entry point for the common single-payload case (exactly one
/// representation, no file-list aggregation): compute the content hash of
/// `bytes`, fold it into a snapshot identity, and format it as
/// `blake3v1:<hex>`.
pub fn snapshot_hash_single_payload(bytes: &[u8]) -> String {
    let inner = content_hash(bytes);
    let outer = snapshot_hash([inner]);
    format_blake3v1(&outer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_hash_matches_plain_blake3() {
        let bytes = b"hello uniclipboard";
        assert_eq!(content_hash(bytes), *blake3::hash(bytes).as_bytes());
    }

    #[test]
    fn content_hash_reader_matches_content_hash() {
        let bytes = vec![7u8; 200 * 1024]; // exercise multiple chunk reads
        assert_eq!(
            content_hash_reader(&bytes[..]).unwrap(),
            content_hash(&bytes)
        );
    }

    #[test]
    fn snapshot_hash_single_rep_matches_manual_formula() {
        let payload = b"a single mobile-sync payload";
        let inner = content_hash(payload);

        let mut expected_hasher = blake3::Hasher::new();
        expected_hasher.update(b"snapshot-hash-v1|");
        expected_hasher.update(&inner);
        let expected = *expected_hasher.finalize().as_bytes();

        assert_eq!(snapshot_hash([inner]), expected);
    }

    #[test]
    fn snapshot_hash_is_order_independent() {
        let a = content_hash(b"rep-a");
        let b = content_hash(b"rep-b");
        assert_eq!(snapshot_hash([a, b]), snapshot_hash([b, a]));
    }

    #[test]
    fn file_content_wrapper_matches_manual_formula() {
        let d1 = content_hash(b"file-one");
        let d2 = content_hash(b"file-two");
        let mut sorted = [d1, d2];
        sorted.sort_unstable();

        let mut expected_hasher = blake3::Hasher::new();
        expected_hasher.update(b"file-content|");
        for d in &sorted {
            expected_hasher.update(d);
        }
        let expected = *expected_hasher.finalize().as_bytes();

        assert_eq!(file_content_wrapper(&[d1, d2]), expected);
    }

    #[test]
    fn file_content_wrapper_is_order_independent() {
        let d1 = content_hash(b"file-one");
        let d2 = content_hash(b"file-two");
        assert_eq!(
            file_content_wrapper(&[d1, d2]),
            file_content_wrapper(&[d2, d1])
        );
    }

    #[test]
    fn file_set_member_v1_matches_manual_formula() {
        let digest = content_hash(b"member bytes");
        let mut expected_hasher = blake3::Hasher::new();
        expected_hasher.update(b"file-set-member-v1|");
        expected_hasher.update("资料".as_bytes());
        expected_hasher.update(&[0]);
        expected_hasher.update(b"nested/file.txt");
        expected_hasher.update(&[0]);
        expected_hasher.update(b"x");
        expected_hasher.update(&[0]);
        expected_hasher.update(&digest);

        assert_eq!(
            file_set_member_v1("资料", "nested/file.txt", "x", Some(&digest)),
            *expected_hasher.finalize().as_bytes()
        );
    }

    #[test]
    fn empty_directory_member_has_no_file_digest_bytes() {
        let mut expected_hasher = blake3::Hasher::new();
        expected_hasher.update(b"file-set-member-v1|");
        expected_hasher.update(b"folder");
        expected_hasher.update(&[0]);
        expected_hasher.update(b"empty");
        expected_hasher.update(&[0]);
        expected_hasher.update(b"d");
        expected_hasher.update(&[0]);

        assert_eq!(
            file_set_member_v1("folder", "empty", "d", None),
            *expected_hasher.finalize().as_bytes()
        );
    }

    #[test]
    fn file_set_v1_wrapper_sorts_leaves() {
        let first = content_hash(b"first leaf");
        let second = content_hash(b"second leaf");
        let mut sorted = [first, second];
        sorted.sort_unstable();
        let mut expected_hasher = blake3::Hasher::new();
        expected_hasher.update(b"file-set-v1|");
        for leaf in &sorted {
            expected_hasher.update(leaf);
        }

        let expected = *expected_hasher.finalize().as_bytes();
        assert_eq!(file_set_v1_wrapper(&[first, second]), expected);
        assert_eq!(
            file_set_v1_wrapper(&[first, second]),
            file_set_v1_wrapper(&[second, first])
        );
    }

    #[test]
    fn format_blake3v1_is_lowercase_hex_with_prefix() {
        let bytes = [0xABu8; 32];
        let formatted = format_blake3v1(&bytes);
        assert!(formatted.starts_with("blake3v1:"));
        assert_eq!(formatted, format!("blake3v1:{}", "ab".repeat(32)));
    }

    #[test]
    fn snapshot_hash_single_payload_round_trips_through_pieces() {
        let payload = b"a single mobile-sync payload";
        let expected = format_blake3v1(&snapshot_hash([content_hash(payload)]));
        assert_eq!(snapshot_hash_single_payload(payload), expected);
    }

    #[test]
    fn different_payloads_produce_different_identities() {
        assert_ne!(
            snapshot_hash_single_payload(b"photo-one-bytes"),
            snapshot_hash_single_payload(b"photo-two-bytes")
        );
    }
}
