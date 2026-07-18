use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uc_core::crypto::aad;
use uc_core::ids::EntryId;
use uc_core::ports::{ReceiveArtifact, ReceiveArtifactOwnership};

use crate::security::v1_aead::{decrypt_xchacha_raw, encrypt_xchacha_raw};

const MAGIC: [u8; 4] = *b"UCAR";
const FORMAT_VERSION: u8 = 1;
const NONCE_LEN: usize = 24;
const HEADER_LEN: usize = MAGIC.len() + 1 + NONCE_LEN;

#[derive(Debug, thiserror::Error)]
pub(super) enum ReceiveArtifactCipherError {
    #[error("receive artifact envelope is truncated")]
    Truncated,
    #[error("receive artifact envelope magic mismatch")]
    BadMagic,
    #[error("unsupported receive artifact envelope version")]
    UnsupportedVersion,
    #[error("receive artifact encryption failed")]
    Encrypt,
    #[error("receive artifact verification failed")]
    Decrypt,
    #[error("receive artifact serialization failed")]
    Serialize,
    #[error("receive artifact deserialization failed")]
    Deserialize,
    #[cfg(windows)]
    #[error("receive artifact path encoding is invalid")]
    InvalidPath,
}

#[derive(Serialize, Deserialize)]
struct EncodedArtifact {
    item_id: String,
    staged_path: Vec<u8>,
    final_path: Vec<u8>,
    ownership: u8,
}

pub(super) struct ReceiveArtifactCipher {
    key: [u8; 32],
}

impl ReceiveArtifactCipher {
    pub(super) fn new(key: [u8; 32]) -> Self {
        Self { key }
    }

    pub(super) fn seal(
        &self,
        entry_id: &str,
        attempt_id: &str,
        artifacts: &[ReceiveArtifact],
    ) -> Result<Vec<u8>, ReceiveArtifactCipherError> {
        let encoded = artifacts
            .iter()
            .map(|artifact| {
                Ok(EncodedArtifact {
                    item_id: artifact.item_id.clone(),
                    staged_path: encode_path(&artifact.staged_path),
                    final_path: encode_path(&artifact.final_path),
                    ownership: match artifact.ownership {
                        ReceiveArtifactOwnership::ManagedStaging => 0,
                        ReceiveArtifactOwnership::UserDestination => 1,
                    },
                })
            })
            .collect::<Result<Vec<_>, ReceiveArtifactCipherError>>()?;
        let plaintext =
            postcard::to_stdvec(&encoded).map_err(|_| ReceiveArtifactCipherError::Serialize)?;
        let ad = aad::for_receive_artifact_log(&EntryId::from(entry_id), attempt_id);
        let (nonce, ciphertext) = encrypt_xchacha_raw(&self.key, &plaintext, &ad)
            .map_err(|_| ReceiveArtifactCipherError::Encrypt)?;
        let mut envelope = Vec::with_capacity(HEADER_LEN + ciphertext.len());
        envelope.extend_from_slice(&MAGIC);
        envelope.push(FORMAT_VERSION);
        envelope.extend_from_slice(&nonce);
        envelope.extend_from_slice(&ciphertext);
        Ok(envelope)
    }

    pub(super) fn open(
        &self,
        entry_id: &str,
        attempt_id: &str,
        envelope: &[u8],
    ) -> Result<Vec<ReceiveArtifact>, ReceiveArtifactCipherError> {
        if envelope.len() < HEADER_LEN {
            return Err(ReceiveArtifactCipherError::Truncated);
        }
        if envelope[..MAGIC.len()] != MAGIC {
            return Err(ReceiveArtifactCipherError::BadMagic);
        }
        if envelope[MAGIC.len()] != FORMAT_VERSION {
            return Err(ReceiveArtifactCipherError::UnsupportedVersion);
        }
        let nonce_start = MAGIC.len() + 1;
        let nonce_end = nonce_start + NONCE_LEN;
        let ad = aad::for_receive_artifact_log(&EntryId::from(entry_id), attempt_id);
        let plaintext = decrypt_xchacha_raw(
            &self.key,
            &envelope[nonce_start..nonce_end],
            &envelope[nonce_end..],
            &ad,
        )
        .map_err(|_| ReceiveArtifactCipherError::Decrypt)?;
        let encoded: Vec<EncodedArtifact> = postcard::from_bytes(&plaintext)
            .map_err(|_| ReceiveArtifactCipherError::Deserialize)?;
        encoded
            .into_iter()
            .map(|artifact| {
                Ok(ReceiveArtifact {
                    item_id: artifact.item_id,
                    staged_path: decode_path(artifact.staged_path)?,
                    final_path: decode_path(artifact.final_path)?,
                    ownership: match artifact.ownership {
                        0 => ReceiveArtifactOwnership::ManagedStaging,
                        1 => ReceiveArtifactOwnership::UserDestination,
                        _ => return Err(ReceiveArtifactCipherError::Deserialize),
                    },
                })
            })
            .collect()
    }
}

#[cfg(unix)]
fn encode_path(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(unix)]
fn decode_path(path: Vec<u8>) -> Result<PathBuf, ReceiveArtifactCipherError> {
    use std::os::unix::ffi::OsStringExt;
    Ok(PathBuf::from(OsString::from_vec(path)))
}

#[cfg(windows)]
fn encode_path(path: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[cfg(windows)]
fn decode_path(path: Vec<u8>) -> Result<PathBuf, ReceiveArtifactCipherError> {
    use std::os::windows::ffi::OsStringExt;
    if path.len() % 2 != 0 {
        return Err(ReceiveArtifactCipherError::InvalidPath);
    }
    let wide = path
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    Ok(PathBuf::from(OsString::from_wide(&wide)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifacts_round_trip_without_plaintext_paths() {
        let cipher = ReceiveArtifactCipher::new([7; 32]);
        let artifacts = vec![ReceiveArtifact {
            item_id: "item-1".to_owned(),
            staged_path: PathBuf::from("/tmp/private-stage"),
            final_path: PathBuf::from("/tmp/private-final"),
            ownership: ReceiveArtifactOwnership::ManagedStaging,
        }];
        let envelope = cipher.seal("entry", "a1", &artifacts).unwrap();
        assert_eq!(cipher.open("entry", "a1", &envelope).unwrap(), artifacts);
        assert!(cipher.open("entry", "a2", &envelope).is_err());
        assert!(!envelope
            .windows(b"private-final".len())
            .any(|window| window == b"private-final"));
    }
}
