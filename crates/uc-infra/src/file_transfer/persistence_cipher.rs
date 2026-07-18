use std::sync::Arc;

use serde::{Deserialize, Serialize};
use uc_core::crypto::aad;
use uc_core::file_transfer::FileTransferEvent;
use uc_core::ports::security::current_profile::CurrentProfilePort;
use uc_core::ports::space::{DeriveSpaceSubkeyPort, SpaceAccessError};

use crate::security::v1_aead::{decrypt_xchacha_raw, encrypt_xchacha_raw};

const METADATA_MAGIC: [u8; 4] = *b"UCTM";
const EVENT_MAGIC: [u8; 4] = *b"UCTE";
const FORMAT_VERSION: u8 = 1;
const NONCE_LEN: usize = 24;
const HEADER_LEN: usize = 4 + 1 + NONCE_LEN;
const METADATA_KEY_INFO: &[u8] = b"uniclipboard-file-transfer-metadata/v1";
const EVENT_KEY_INFO: &[u8] = b"uniclipboard-file-transfer-events/v1";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TransferMetadata {
    pub filename: String,
    pub cached_path: Option<String>,
    pub failure_detail: Option<String>,
}

#[derive(Clone)]
pub(crate) struct TransferPersistenceCipher {
    metadata_key: [u8; 32],
    event_key: [u8; 32],
}

impl TransferPersistenceCipher {
    pub(crate) fn new(metadata_key: [u8; 32], event_key: [u8; 32]) -> Self {
        Self {
            metadata_key,
            event_key,
        }
    }

    pub(crate) fn seal_metadata(
        &self,
        transfer_id: &str,
        metadata: &TransferMetadata,
    ) -> Result<Vec<u8>, TransferPersistenceCipherError> {
        let plaintext =
            postcard::to_stdvec(metadata).map_err(|_| TransferPersistenceCipherError::Serialize)?;
        seal(
            &self.metadata_key,
            METADATA_MAGIC,
            &plaintext,
            &aad::for_file_transfer_metadata(transfer_id),
        )
    }

    pub(crate) fn open_metadata(
        &self,
        transfer_id: &str,
        envelope: &[u8],
    ) -> Result<TransferMetadata, TransferPersistenceCipherError> {
        let plaintext = open(
            &self.metadata_key,
            METADATA_MAGIC,
            envelope,
            &aad::for_file_transfer_metadata(transfer_id),
        )?;
        postcard::from_bytes(&plaintext).map_err(|_| TransferPersistenceCipherError::Deserialize)
    }

    pub(crate) fn seal_event(
        &self,
        transfer_id: &str,
        sequence: i32,
        event_type: &str,
        event: &FileTransferEvent,
    ) -> Result<Vec<u8>, TransferPersistenceCipherError> {
        let plaintext =
            serde_json::to_vec(event).map_err(|_| TransferPersistenceCipherError::Serialize)?;
        seal(
            &self.event_key,
            EVENT_MAGIC,
            &plaintext,
            &aad::for_file_transfer_event(transfer_id, sequence, event_type),
        )
    }

    pub(crate) fn open_event(
        &self,
        transfer_id: &str,
        sequence: i32,
        event_type: &str,
        envelope: &[u8],
    ) -> Result<FileTransferEvent, TransferPersistenceCipherError> {
        let plaintext = open(
            &self.event_key,
            EVENT_MAGIC,
            envelope,
            &aad::for_file_transfer_event(transfer_id, sequence, event_type),
        )?;
        serde_json::from_slice(&plaintext).map_err(|_| TransferPersistenceCipherError::Deserialize)
    }
}

pub(crate) async fn derive_transfer_persistence_cipher(
    derive_subkey: &Arc<dyn DeriveSpaceSubkeyPort>,
    current_profile: &Arc<dyn CurrentProfilePort>,
) -> anyhow::Result<TransferPersistenceCipher> {
    let profile = current_profile
        .current_profile()
        .await
        .map_err(|error| anyhow::anyhow!("current profile unavailable: {error}"))?;
    let metadata_key = derive_subkey
        .derive_subkey(profile.as_ref().as_bytes(), METADATA_KEY_INFO)
        .await
        .map_err(map_key_error)?;
    let event_key = derive_subkey
        .derive_subkey(profile.as_ref().as_bytes(), EVENT_KEY_INFO)
        .await
        .map_err(map_key_error)?;
    Ok(TransferPersistenceCipher::new(metadata_key, event_key))
}

fn map_key_error(error: SpaceAccessError) -> anyhow::Error {
    match error {
        SpaceAccessError::NotUnlocked => {
            anyhow::anyhow!("session locked: transfer persistence key unavailable")
        }
        other => anyhow::anyhow!("derive transfer persistence key: {other}"),
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum TransferPersistenceCipherError {
    #[error("transfer persistence envelope is truncated")]
    Truncated,
    #[error("transfer persistence envelope type mismatch")]
    BadMagic,
    #[error("unsupported transfer persistence envelope version")]
    UnsupportedVersion,
    #[error("transfer persistence encryption failed")]
    Encrypt,
    #[error("transfer persistence verification failed")]
    Decrypt,
    #[error("transfer persistence serialization failed")]
    Serialize,
    #[error("transfer persistence deserialization failed")]
    Deserialize,
}

fn seal(
    key: &[u8; 32],
    magic: [u8; 4],
    plaintext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, TransferPersistenceCipherError> {
    let (nonce, ciphertext) = encrypt_xchacha_raw(key, plaintext, aad)
        .map_err(|_| TransferPersistenceCipherError::Encrypt)?;
    let mut envelope = Vec::with_capacity(HEADER_LEN + ciphertext.len());
    envelope.extend_from_slice(&magic);
    envelope.push(FORMAT_VERSION);
    envelope.extend_from_slice(&nonce);
    envelope.extend_from_slice(&ciphertext);
    Ok(envelope)
}

fn open(
    key: &[u8; 32],
    magic: [u8; 4],
    envelope: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, TransferPersistenceCipherError> {
    if envelope.len() < HEADER_LEN {
        return Err(TransferPersistenceCipherError::Truncated);
    }
    if envelope[..4] != magic {
        return Err(TransferPersistenceCipherError::BadMagic);
    }
    if envelope[4] != FORMAT_VERSION {
        return Err(TransferPersistenceCipherError::UnsupportedVersion);
    }
    decrypt_xchacha_raw(key, &envelope[5..HEADER_LEN], &envelope[HEADER_LEN..], aad)
        .map_err(|_| TransferPersistenceCipherError::Decrypt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_round_trip_is_bound_to_transfer() {
        let cipher = TransferPersistenceCipher::new([7; 32], [8; 32]);
        let metadata = TransferMetadata {
            filename: "private.txt".to_owned(),
            cached_path: Some("/private/path".to_owned()),
            failure_detail: None,
        };
        let envelope = cipher.seal_metadata("transfer-1", &metadata).unwrap();
        assert_eq!(
            cipher.open_metadata("transfer-1", &envelope).unwrap(),
            metadata
        );
        assert!(cipher.open_metadata("transfer-2", &envelope).is_err());
        assert!(!envelope
            .windows(b"private.txt".len())
            .any(|window| window == b"private.txt"));
    }

    #[test]
    fn event_round_trip_is_bound_to_sequence_and_type() {
        let cipher = TransferPersistenceCipher::new([7; 32], [8; 32]);
        let event = FileTransferEvent::started("transfer-1", "peer", "private.txt", Some(5));
        let envelope = cipher
            .seal_event("transfer-1", 1, "started", &event)
            .unwrap();
        assert_eq!(
            cipher
                .open_event("transfer-1", 1, "started", &envelope)
                .unwrap(),
            event
        );
        assert!(cipher
            .open_event("transfer-1", 2, "started", &envelope)
            .is_err());
        assert!(!envelope
            .windows(b"private.txt".len())
            .any(|window| window == b"private.txt"));
    }
}

#[cfg(test)]
pub(crate) mod test_keys {
    use super::*;
    use async_trait::async_trait;
    use uc_core::ids::ProfileId;
    use uc_core::ports::security::current_profile::CurrentProfileError;

    struct FixedSubkey;

    #[async_trait]
    impl DeriveSpaceSubkeyPort for FixedSubkey {
        async fn derive_subkey(
            &self,
            _salt: &[u8],
            info: &[u8],
        ) -> Result<[u8; 32], SpaceAccessError> {
            Ok(if info == METADATA_KEY_INFO {
                [0x5A; 32]
            } else {
                [0xA5; 32]
            })
        }
    }

    struct FixedProfile;

    #[async_trait]
    impl CurrentProfilePort for FixedProfile {
        async fn current_profile(&self) -> Result<ProfileId, CurrentProfileError> {
            Ok(ProfileId::from("test"))
        }
    }

    pub(crate) fn ports() -> (Arc<dyn DeriveSpaceSubkeyPort>, Arc<dyn CurrentProfilePort>) {
        (Arc::new(FixedSubkey), Arc::new(FixedProfile))
    }
}
