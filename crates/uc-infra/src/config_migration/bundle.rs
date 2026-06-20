//! `.ucbundle` container codec: plaintext header + Argon2id-derived key +
//! XChaCha20-Poly1305 sealed archive.
//!
//! On-disk layout (all multi-byte integers little-endian):
//!
//! ```text
//! plaintext header (authenticated as AEAD AAD, never encrypted):
//!   magic        "UCBUNDLE"            8 bytes
//!   format_ver   u16                   2 bytes   (= FORMAT_VER)
//!   kdf_algo     u8                    1 byte    (= KDF_ALGO_ARGON2ID)
//!   argon2 m_kib u32                   4 bytes
//!   argon2 iters u32                   4 bytes
//!   argon2 par   u32                   4 bytes
//!   salt         16 bytes
//!   nonce        24 bytes
//! ciphertext:
//!   XChaCha20-Poly1305 sealed bytes (tar archive + 16-byte tag), to EOF
//! ```
//!
//! The header is fixed-size, so reading it never requires the password — that
//! is what makes a metadata-only preview (decrypt → read `manifest.json`) and
//! version negotiation possible. The whole header is fed as AAD so a tampered
//! KDF parameter or salt fails the AEAD tag rather than silently weakening the
//! derivation.
//!
//! Persistence invariants: `magic`, `FORMAT_VER`, and the header byte layout are
//! disk-compatibility contracts. A reader rejects an unknown magic or a newer
//! `format_ver` with [`BundleError::Incompatible`] (never as a generic parse
//! error) so the operator gets an actionable reason.

use argon2::Argon2;
use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use rand::rngs::OsRng;
use rand::TryRngCore;
use uc_core::crypto::domain::Passphrase;
use zeroize::Zeroize;

/// Magic prefix identifying a `.ucbundle` file.
pub const MAGIC: &[u8; 8] = b"UCBUNDLE";

/// Outer container format version. Bump on any header-layout change.
pub const FORMAT_VER: u16 = 1;

/// KDF algorithm tag: Argon2id.
const KDF_ALGO_ARGON2ID: u8 = 1;

const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 24;
const KEY_LEN: usize = 32;

/// Fixed header byte length (see module layout).
const HEADER_LEN: usize = 8 + 2 + 1 + 4 + 4 + 4 + SALT_LEN + NONCE_LEN;

/// Hard ceiling on a sealed payload we will decrypt into memory.
///
/// Config bundles carry a sqlite snapshot plus small JSON members; 2 GiB is far
/// above any realistic clipboard history yet bounds a hostile/truncated file
/// from driving an unbounded allocation. Exceeding it is reported as
/// incompatible rather than attempted.
const MAX_SEALED_LEN: u64 = 2 * 1024 * 1024 * 1024;

/// Upper bound on the Argon2 memory cost we will honour from a bundle header.
///
/// The header is authenticated as AAD, but the KDF runs *before* the AEAD tag
/// can be checked (the derived key is needed to verify the tag), and preview is
/// ungated — so a hostile header could otherwise drive an unbounded Argon2
/// allocation during an unauthenticated read. 1 GiB is 8× the production
/// baseline (128 MiB), well above any value we emit, yet bounds the blast
/// radius. Higher values are rejected as incompatible.
const MAX_KDF_MEM_KIB: u32 = 1024 * 1024;

/// Upper bound on the Argon2 time cost (iterations) honoured from a header.
const MAX_KDF_ITERS: u32 = 1024;

/// Upper bound on the Argon2 degree of parallelism honoured from a header.
const MAX_KDF_PARALLELISM: u32 = 256;

/// Argon2id cost parameters recorded in the header and used for derivation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Argon2Params {
    /// Memory cost in KiB.
    pub mem_kib: u32,
    /// Time cost (iterations).
    pub iters: u32,
    /// Degree of parallelism (lanes).
    pub parallelism: u32,
}

impl Argon2Params {
    /// Production defaults, aligned with the project's keyslot KDF baseline
    /// (128 MiB / 3 iters / 4 lanes).
    pub const fn production() -> Self {
        Self {
            mem_kib: 128 * 1024,
            iters: 3,
            parallelism: 4,
        }
    }
}

/// Parsed plaintext header of a `.ucbundle`.
#[derive(Debug, Clone)]
pub struct BundleHeader {
    pub format_ver: u16,
    pub kdf: Argon2Params,
    pub salt: [u8; SALT_LEN],
    pub nonce: [u8; NONCE_LEN],
}

/// Codec-level failures. Cipher/KDF/format detail stays here; the adapter maps
/// these onto the domain `ConfigMigrationError` variants.
#[derive(Debug, thiserror::Error)]
pub enum BundleError {
    /// Wrong magic, wrong password, or tampered/corrupt ciphertext. These are
    /// deliberately one bucket to avoid a password oracle.
    #[error("invalid password or corrupt bundle")]
    InvalidOrCorrupt,
    /// Structurally recognizable but unsupported (newer format version, oversize
    /// payload, unknown KDF tag).
    #[error("incompatible bundle: {0}")]
    Incompatible(String),
    /// Key derivation or AEAD setup failed for a reason that is not attacker
    /// controlled (e.g. bad Argon2 parameters, RNG failure).
    #[error("crypto failure")]
    Crypto,
}

/// Serialize a header to its fixed-size byte form (also used as AEAD AAD).
fn encode_header(header: &BundleHeader) -> [u8; HEADER_LEN] {
    let mut out = [0u8; HEADER_LEN];
    let mut o = 0;
    out[o..o + 8].copy_from_slice(MAGIC);
    o += 8;
    out[o..o + 2].copy_from_slice(&header.format_ver.to_le_bytes());
    o += 2;
    out[o] = KDF_ALGO_ARGON2ID;
    o += 1;
    out[o..o + 4].copy_from_slice(&header.kdf.mem_kib.to_le_bytes());
    o += 4;
    out[o..o + 4].copy_from_slice(&header.kdf.iters.to_le_bytes());
    o += 4;
    out[o..o + 4].copy_from_slice(&header.kdf.parallelism.to_le_bytes());
    o += 4;
    out[o..o + SALT_LEN].copy_from_slice(&header.salt);
    o += SALT_LEN;
    out[o..o + NONCE_LEN].copy_from_slice(&header.nonce);
    out
}

/// Parse and validate a header from the front of `bytes`.
///
/// Returns the header plus the byte offset where the ciphertext begins.
pub fn parse_header(bytes: &[u8]) -> Result<(BundleHeader, usize), BundleError> {
    if bytes.len() < HEADER_LEN {
        return Err(BundleError::InvalidOrCorrupt);
    }
    if &bytes[0..8] != MAGIC {
        return Err(BundleError::InvalidOrCorrupt);
    }
    let mut o = 8;
    let format_ver = u16::from_le_bytes([bytes[o], bytes[o + 1]]);
    o += 2;
    if format_ver > FORMAT_VER {
        return Err(BundleError::Incompatible(format!(
            "bundle format version {format_ver} is newer than supported {FORMAT_VER}"
        )));
    }
    let kdf_algo = bytes[o];
    o += 1;
    if kdf_algo != KDF_ALGO_ARGON2ID {
        return Err(BundleError::Incompatible(format!(
            "unsupported key-derivation algorithm tag {kdf_algo}"
        )));
    }
    // Length was already checked against HEADER_LEN above, so these four-byte
    // reads are in bounds; copy into a fixed array to avoid a fallible
    // `try_into`.
    let read_u32 = |off: usize| {
        let mut b = [0u8; 4];
        b.copy_from_slice(&bytes[off..off + 4]);
        u32::from_le_bytes(b)
    };
    let mem_kib = read_u32(o);
    o += 4;
    let iters = read_u32(o);
    o += 4;
    let parallelism = read_u32(o);
    o += 4;
    // Bound the KDF parameters before they reach `derive_key`: derivation runs
    // ahead of the AEAD tag check, so an out-of-range memory cost in a hostile
    // header would otherwise allocate before authentication can reject it.
    if !(8..=MAX_KDF_MEM_KIB).contains(&mem_kib)
        || !(1..=MAX_KDF_ITERS).contains(&iters)
        || !(1..=MAX_KDF_PARALLELISM).contains(&parallelism)
    {
        return Err(BundleError::Incompatible(format!(
            "key-derivation parameters out of range (mem_kib={mem_kib}, iters={iters}, parallelism={parallelism})"
        )));
    }
    let mut salt = [0u8; SALT_LEN];
    salt.copy_from_slice(&bytes[o..o + SALT_LEN]);
    o += SALT_LEN;
    let mut nonce = [0u8; NONCE_LEN];
    nonce.copy_from_slice(&bytes[o..o + NONCE_LEN]);
    o += NONCE_LEN;

    Ok((
        BundleHeader {
            format_ver,
            kdf: Argon2Params {
                mem_kib,
                iters,
                parallelism,
            },
            salt,
            nonce,
        },
        o,
    ))
}

/// Derive the 32-byte AEAD key from a passphrase + header KDF parameters.
fn derive_key(
    password: &Passphrase,
    salt: &[u8],
    kdf: &Argon2Params,
) -> Result<[u8; KEY_LEN], BundleError> {
    let params = argon2::Params::new(kdf.mem_kib, kdf.iters, kdf.parallelism, Some(KEY_LEN))
        .map_err(|_| BundleError::Crypto)?;
    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut key = [0u8; KEY_LEN];
    argon2
        .hash_password_into(password.expose().as_bytes(), salt, &mut key)
        .map_err(|_| BundleError::Crypto)?;
    Ok(key)
}

/// Seal `archive` into a full `.ucbundle` byte stream protected by `password`.
///
/// A fresh random salt and nonce are generated per call. The caller supplies the
/// KDF cost parameters (production defaults via [`Argon2Params::production`]).
///
/// This is the passphrase-symmetric counterpart of [`open`]: the key is derived
/// from `password` + the freshly-generated salt. When the AEAD key is already
/// available pre-derived (e.g. the installation's KEK), use [`seal_with_key`]
/// instead, passing the salt + KDF parameters that derive it from the
/// passphrase so the same [`open`] path can reproduce it.
pub fn seal(
    password: &Passphrase,
    kdf: Argon2Params,
    archive: &[u8],
) -> Result<Vec<u8>, BundleError> {
    let mut salt = [0u8; SALT_LEN];
    OsRng
        .try_fill_bytes(&mut salt)
        .map_err(|_| BundleError::Crypto)?;
    let mut nonce = [0u8; NONCE_LEN];
    OsRng
        .try_fill_bytes(&mut nonce)
        .map_err(|_| BundleError::Crypto)?;

    let mut key = derive_key(password, &salt, &kdf)?;
    let out = finish_seal(&key, salt, kdf, nonce, archive);
    key.zeroize();
    out
}

/// Seal `archive` into a full `.ucbundle` byte stream using a pre-derived
/// 32-byte AEAD `key` directly, recording `salt` + `kdf` in the (authenticated)
/// header so a reader reproduces the same key via [`open`] from the matching
/// passphrase.
///
/// Unlike [`seal`], no key derivation runs here: the caller supplies the key
/// (e.g. the installation's KEK) together with the salt + KDF parameters that
/// would derive it from the passphrase. `key` is borrowed and never zeroized by
/// this function — its lifetime is the caller's to manage. A fresh random nonce
/// is generated per call, so reusing the same `salt` across bundles does not
/// reuse a keystream.
pub fn seal_with_key(
    key: &[u8; KEY_LEN],
    salt: &[u8; SALT_LEN],
    kdf: Argon2Params,
    archive: &[u8],
) -> Result<Vec<u8>, BundleError> {
    let mut nonce = [0u8; NONCE_LEN];
    OsRng
        .try_fill_bytes(&mut nonce)
        .map_err(|_| BundleError::Crypto)?;
    finish_seal(key, *salt, kdf, nonce, archive)
}

/// Shared tail of both seal paths: build the header from the given parameters,
/// AEAD-encrypt `archive` under `key`, and concatenate header + ciphertext.
fn finish_seal(
    key: &[u8; KEY_LEN],
    salt: [u8; SALT_LEN],
    kdf: Argon2Params,
    nonce: [u8; NONCE_LEN],
    archive: &[u8],
) -> Result<Vec<u8>, BundleError> {
    let header = BundleHeader {
        format_ver: FORMAT_VER,
        kdf,
        salt,
        nonce,
    };
    let aad = encode_header(&header);

    let cipher = XChaCha20Poly1305::new_from_slice(key).map_err(|_| BundleError::Crypto)?;
    let ciphertext = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: archive,
                aad: &aad,
            },
        )
        .map_err(|_| BundleError::Crypto)?;

    let mut out = Vec::with_capacity(HEADER_LEN + ciphertext.len());
    out.extend_from_slice(&aad);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Open a full `.ucbundle` byte stream: parse the header, derive the key, and
/// AEAD-decrypt the archive bytes.
///
/// A wrong password or any ciphertext tampering surfaces as
/// [`BundleError::InvalidOrCorrupt`] (indistinguishable, no oracle).
pub fn open(password: &Passphrase, bytes: &[u8]) -> Result<Vec<u8>, BundleError> {
    let (header, ct_offset) = parse_header(bytes)?;
    let sealed = &bytes[ct_offset..];
    if sealed.len() as u64 > MAX_SEALED_LEN {
        return Err(BundleError::Incompatible(format!(
            "sealed payload exceeds the {MAX_SEALED_LEN}-byte ceiling"
        )));
    }
    let aad = encode_header(&header);

    let mut key = derive_key(password, &header.salt, &header.kdf)?;
    let cipher = XChaCha20Poly1305::new_from_slice(&key).map_err(|_| BundleError::Crypto)?;
    key.zeroize();

    cipher
        .decrypt(
            XNonce::from_slice(&header.nonce),
            Payload {
                msg: sealed,
                aad: &aad,
            },
        )
        .map_err(|_| BundleError::InvalidOrCorrupt)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cheap Argon2 params so unit tests do not spend 128 MiB / 3 iters.
    fn cheap() -> Argon2Params {
        Argon2Params {
            mem_kib: 8,
            iters: 1,
            parallelism: 1,
        }
    }

    #[test]
    fn seal_then_open_round_trips() {
        let pw = Passphrase::from("correct horse battery staple");
        let archive = b"this stands in for a tar archive".to_vec();
        let bundle = seal(&pw, cheap(), &archive).unwrap();
        assert_eq!(&bundle[0..8], MAGIC);

        let opened = open(&pw, &bundle).unwrap();
        assert_eq!(opened, archive);
    }

    #[test]
    fn seal_with_key_opens_with_the_matching_passphrase() {
        // Mirrors the production export path: the bundle is sealed with a
        // pre-derived key (the "KEK") plus the salt + KDF that derive it from a
        // passphrase; `open` re-derives that key from the passphrase + header.
        let pw = Passphrase::from("space passphrase");
        let salt = [7u8; SALT_LEN];
        let kdf = cheap();
        let key = derive_key(&pw, &salt, &kdf).unwrap();
        let archive = b"db snapshot + vault + secrets".to_vec();

        let bundle = seal_with_key(&key, &salt, kdf, &archive).unwrap();
        assert_eq!(&bundle[0..8], MAGIC);

        let opened = open(&pw, &bundle).unwrap();
        assert_eq!(opened, archive);
    }

    #[test]
    fn seal_with_key_rejects_a_wrong_passphrase() {
        let salt = [3u8; SALT_LEN];
        let kdf = cheap();
        let key = derive_key(&Passphrase::from("right"), &salt, &kdf).unwrap();
        let bundle = seal_with_key(&key, &salt, kdf, b"payload").unwrap();

        let err = open(&Passphrase::from("wrong"), &bundle).unwrap_err();
        assert!(matches!(err, BundleError::InvalidOrCorrupt));
    }

    #[test]
    fn wrong_password_is_invalid_or_corrupt() {
        let bundle = seal(&Passphrase::from("right"), cheap(), b"payload").unwrap();
        let err = open(&Passphrase::from("wrong"), &bundle).unwrap_err();
        assert!(matches!(err, BundleError::InvalidOrCorrupt));
    }

    #[test]
    fn tampered_ciphertext_is_invalid_or_corrupt() {
        let pw = Passphrase::from("pw");
        let mut bundle = seal(&pw, cheap(), b"payload").unwrap();
        let last = bundle.len() - 1;
        bundle[last] ^= 0xFF;
        let err = open(&pw, &bundle).unwrap_err();
        assert!(matches!(err, BundleError::InvalidOrCorrupt));
    }

    #[test]
    fn tampered_header_breaks_aad_and_fails() {
        let pw = Passphrase::from("pw");
        let mut bundle = seal(&pw, cheap(), b"payload").unwrap();
        // Flip a salt byte (inside the header / AAD region).
        bundle[8 + 2 + 1 + 12] ^= 0x01;
        let err = open(&pw, &bundle).unwrap_err();
        // Header still parses, but AAD mismatch + altered salt → decrypt fails.
        assert!(matches!(err, BundleError::InvalidOrCorrupt));
    }

    #[test]
    fn bad_magic_is_invalid_or_corrupt() {
        let err = open(&Passphrase::from("pw"), b"NOTAUCBUNDLExxxxxxxxxxxxxxxxxxxx").unwrap_err();
        assert!(matches!(err, BundleError::InvalidOrCorrupt));
    }

    #[test]
    fn newer_format_version_is_incompatible() {
        let pw = Passphrase::from("pw");
        let mut bundle = seal(&pw, cheap(), b"payload").unwrap();
        // Overwrite the format_ver field (offset 8, little-endian u16) with a
        // value above what we support.
        let bumped = (FORMAT_VER + 1).to_le_bytes();
        bundle[8] = bumped[0];
        bundle[9] = bumped[1];
        let err = open(&pw, &bundle).unwrap_err();
        match err {
            BundleError::Incompatible(reason) => assert!(reason.contains("newer")),
            other => panic!("expected Incompatible, got {other:?}"),
        }
    }

    #[test]
    fn truncated_header_is_invalid_or_corrupt() {
        let err = open(&Passphrase::from("pw"), b"UCBUNDLE").unwrap_err();
        assert!(matches!(err, BundleError::InvalidOrCorrupt));
    }

    #[test]
    fn header_parse_reports_ciphertext_offset() {
        let pw = Passphrase::from("pw");
        let bundle = seal(&pw, cheap(), b"abc").unwrap();
        let (_, offset) = parse_header(&bundle).unwrap();
        assert_eq!(offset, HEADER_LEN);
    }
}
