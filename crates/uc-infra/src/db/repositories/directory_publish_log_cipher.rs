use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uc_core::crypto::aad;
use uc_core::ids::EntryId;

use crate::security::v1_aead::{decrypt_xchacha_raw, encrypt_xchacha_raw};

const MAGIC: [u8; 4] = *b"UCDP";
const FORMAT_VERSION: u8 = 1;
const NONCE_LEN: usize = 24;
const HEADER_LEN: usize = MAGIC.len() + 1 + NONCE_LEN;

#[derive(Debug, thiserror::Error)]
pub(super) enum PublishLogCipherError {
    #[error("directory publish log envelope is truncated")]
    Truncated,
    #[error("directory publish log envelope magic mismatch")]
    BadMagic,
    #[error("unsupported directory publish log envelope version")]
    UnsupportedVersion,
    #[error("directory publish log encryption failed")]
    EncryptFailed,
    #[error("directory publish log verification failed")]
    DecryptFailed,
    #[error("directory publish log root map serialization failed")]
    Serialize,
    #[error("directory publish log root map deserialization failed")]
    Deserialize,
    #[cfg(windows)]
    #[error("directory publish log path encoding is invalid for this platform")]
    InvalidPathEncoding,
}

#[derive(Serialize, Deserialize)]
struct EncodedRootMap {
    roots: Vec<(EncodedPath, EncodedPath)>,
}

#[derive(Serialize, Deserialize)]
struct EncodedPath {
    bytes: Vec<u8>,
}

#[derive(Clone)]
pub(super) struct DirectoryPublishLogCipher {
    key: [u8; 32],
}

impl DirectoryPublishLogCipher {
    pub(super) fn new(key: [u8; 32]) -> Self {
        Self { key }
    }

    pub(super) fn seal(
        &self,
        entry_id: &EntryId,
        attempt_id: &str,
        root_map: &[(PathBuf, PathBuf)],
    ) -> Result<Vec<u8>, PublishLogCipherError> {
        let payload = EncodedRootMap {
            roots: root_map
                .iter()
                .map(|(staged, final_path)| Ok((encode_path(staged)?, encode_path(final_path)?)))
                .collect::<Result<Vec<_>, PublishLogCipherError>>()?,
        };
        let plaintext =
            postcard::to_stdvec(&payload).map_err(|_| PublishLogCipherError::Serialize)?;
        let ad = aad::for_directory_publish_log(entry_id, attempt_id);
        let (nonce, ciphertext) = encrypt_xchacha_raw(&self.key, &plaintext, &ad)
            .map_err(|_| PublishLogCipherError::EncryptFailed)?;

        let mut envelope = Vec::with_capacity(HEADER_LEN + ciphertext.len());
        envelope.extend_from_slice(&MAGIC);
        envelope.push(FORMAT_VERSION);
        envelope.extend_from_slice(&nonce);
        envelope.extend_from_slice(&ciphertext);
        Ok(envelope)
    }

    pub(super) fn open(
        &self,
        entry_id: &EntryId,
        attempt_id: &str,
        envelope: &[u8],
    ) -> Result<Vec<(PathBuf, PathBuf)>, PublishLogCipherError> {
        if envelope.len() < HEADER_LEN {
            return Err(PublishLogCipherError::Truncated);
        }
        if envelope[..MAGIC.len()] != MAGIC {
            return Err(PublishLogCipherError::BadMagic);
        }
        if envelope[MAGIC.len()] != FORMAT_VERSION {
            return Err(PublishLogCipherError::UnsupportedVersion);
        }
        let nonce_start = MAGIC.len() + 1;
        let nonce_end = nonce_start + NONCE_LEN;
        let ad = aad::for_directory_publish_log(entry_id, attempt_id);
        let plaintext = decrypt_xchacha_raw(
            &self.key,
            &envelope[nonce_start..nonce_end],
            &envelope[nonce_end..],
            &ad,
        )
        .map_err(|_| PublishLogCipherError::DecryptFailed)?;
        let payload: EncodedRootMap =
            postcard::from_bytes(&plaintext).map_err(|_| PublishLogCipherError::Deserialize)?;
        payload
            .roots
            .into_iter()
            .map(|(staged, final_path)| Ok((decode_path(staged)?, decode_path(final_path)?)))
            .collect()
    }
}

#[cfg(unix)]
fn encode_path(path: &Path) -> Result<EncodedPath, PublishLogCipherError> {
    use std::os::unix::ffi::OsStrExt;

    Ok(EncodedPath {
        bytes: path.as_os_str().as_bytes().to_vec(),
    })
}

#[cfg(unix)]
fn decode_path(path: EncodedPath) -> Result<PathBuf, PublishLogCipherError> {
    use std::os::unix::ffi::OsStringExt;

    Ok(PathBuf::from(OsString::from_vec(path.bytes)))
}

#[cfg(windows)]
fn encode_path(path: &Path) -> Result<EncodedPath, PublishLogCipherError> {
    use std::os::windows::ffi::OsStrExt;

    let mut bytes = Vec::new();
    for unit in path.as_os_str().encode_wide() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    Ok(EncodedPath { bytes })
}

#[cfg(windows)]
fn decode_path(path: EncodedPath) -> Result<PathBuf, PublishLogCipherError> {
    use std::os::windows::ffi::OsStringExt;

    if path.bytes.len() % 2 != 0 {
        return Err(PublishLogCipherError::InvalidPathEncoding);
    }
    let wide = path
        .bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect::<Vec<_>>();
    Ok(PathBuf::from(OsString::from_wide(&wide)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_map_round_trips_and_is_bound_to_attempt() {
        let cipher = DirectoryPublishLogCipher::new([7; 32]);
        let entry_id = EntryId::from("entry");
        let roots = vec![(
            PathBuf::from("/tmp/.uniclip-incoming/root"),
            PathBuf::from("/tmp/private-name"),
        )];
        let envelope = cipher.seal(&entry_id, "a1", &roots).unwrap();
        assert_eq!(cipher.open(&entry_id, "a1", &envelope).unwrap(), roots);
        assert!(cipher.open(&entry_id, "a2", &envelope).is_err());
        assert!(!envelope
            .windows(b"private-name".len())
            .any(|window| window == b"private-name"));
    }
}
