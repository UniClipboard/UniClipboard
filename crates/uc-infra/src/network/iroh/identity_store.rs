//! iroh-backed implementation of [`LocalIdentityPort`].
//!
//! Persists the device's long-term Ed25519 secret key (raw 32 bytes) through a
//! [`SecureStoragePort`] under [`IDENTITY_STORE_KEY`] and derives the public
//! [`IdentityFingerprint`] via an injected [`IdentityFingerprintFactoryPort`].
//! Raw secret material never leaves this adapter.

use std::sync::Arc;

use async_trait::async_trait;
use iroh::SecretKey;
use tracing::{debug, instrument};

use uc_core::{
    ports::{
        security::IdentityFingerprintFactoryPort, LocalIdentityError, LocalIdentityPort,
        SecureStorageError, SecureStoragePort,
    },
    security::IdentityFingerprint,
};

/// Secure-storage key under which the 32-byte Ed25519 secret is persisted.
///
/// The `v1` suffix reserves room for a future key-rotation / re-encoding
/// migration without colliding with existing installs.
pub const IDENTITY_STORE_KEY: &str = "iroh-identity:v1";

const SECRET_KEY_LEN: usize = 32;

/// iroh adapter for [`LocalIdentityPort`].
pub struct IrohIdentityStore {
    secure_storage: Arc<dyn SecureStoragePort>,
    fingerprint_factory: Arc<dyn IdentityFingerprintFactoryPort + Send + Sync>,
}

impl IrohIdentityStore {
    pub fn new(
        secure_storage: Arc<dyn SecureStoragePort>,
        fingerprint_factory: Arc<dyn IdentityFingerprintFactoryPort + Send + Sync>,
    ) -> Self {
        Self {
            secure_storage,
            fingerprint_factory,
        }
    }

    fn load_secret(&self) -> Result<Option<SecretKey>, LocalIdentityError> {
        let raw = self
            .secure_storage
            .get(IDENTITY_STORE_KEY)
            .map_err(map_storage_err)?;
        let Some(bytes) = raw else {
            return Ok(None);
        };
        if bytes.len() != SECRET_KEY_LEN {
            return Err(LocalIdentityError::Storage(format!(
                "corrupt iroh identity: expected {SECRET_KEY_LEN} bytes, got {}",
                bytes.len()
            )));
        }
        let mut arr = [0u8; SECRET_KEY_LEN];
        arr.copy_from_slice(&bytes);
        Ok(Some(SecretKey::from_bytes(&arr)))
    }

    fn persist_secret(&self, sk: &SecretKey) -> Result<(), LocalIdentityError> {
        let bytes: [u8; SECRET_KEY_LEN] = sk.to_bytes();
        self.secure_storage
            .set(IDENTITY_STORE_KEY, &bytes)
            .map_err(map_storage_err)
    }

    fn derive_fingerprint(
        &self,
        sk: &SecretKey,
    ) -> Result<IdentityFingerprint, LocalIdentityError> {
        let pubkey_bytes: [u8; SECRET_KEY_LEN] = *sk.public().as_bytes();
        self.fingerprint_factory
            .from_public_key(&pubkey_bytes)
            .map_err(|err| {
                // Ed25519 public keys are always 32 bytes, so a factory failure
                // here indicates an algorithm-level bug rather than bad input.
                // Surface it through `Storage` — the only non-AlreadyExists
                // variant — with enough context for ops.
                LocalIdentityError::Storage(format!("fingerprint derivation failed: {err}"))
            })
    }

    fn generate_new() -> SecretKey {
        SecretKey::generate()
    }

    /// Return the persisted iroh `SecretKey`, generating + persisting a fresh
    /// one if the slot is empty. Used by [`super::node::IrohNodeBuilder`] so
    /// the endpoint's network identity matches the fingerprint exposed via
    /// [`LocalIdentityPort`]. Crate-private so the `iroh::SecretKey` type
    /// stays confined to this module.
    pub(crate) fn ensure_secret_key(&self) -> Result<SecretKey, LocalIdentityError> {
        if let Some(existing) = self.load_secret()? {
            return Ok(existing);
        }
        let sk = Self::generate_new();
        self.persist_secret(&sk)?;
        Ok(sk)
    }
}

#[async_trait]
impl LocalIdentityPort for IrohIdentityStore {
    #[instrument(skip_all)]
    async fn create(&self) -> Result<IdentityFingerprint, LocalIdentityError> {
        if self.load_secret()?.is_some() {
            return Err(LocalIdentityError::AlreadyExists);
        }
        let sk = Self::generate_new();
        self.persist_secret(&sk)?;
        let fp = self.derive_fingerprint(&sk)?;
        debug!(fingerprint = %fp, "iroh identity created");
        Ok(fp)
    }

    #[instrument(skip_all)]
    async fn ensure(&self) -> Result<IdentityFingerprint, LocalIdentityError> {
        if let Some(existing) = self.load_secret()? {
            return self.derive_fingerprint(&existing);
        }
        let sk = Self::generate_new();
        self.persist_secret(&sk)?;
        let fp = self.derive_fingerprint(&sk)?;
        debug!(fingerprint = %fp, "iroh identity generated via ensure()");
        Ok(fp)
    }

    #[instrument(skip_all)]
    async fn get_current_fingerprint(
        &self,
    ) -> Result<Option<IdentityFingerprint>, LocalIdentityError> {
        match self.load_secret()? {
            None => Ok(None),
            Some(sk) => Ok(Some(self.derive_fingerprint(&sk)?)),
        }
    }
}

fn map_storage_err(err: SecureStorageError) -> LocalIdentityError {
    LocalIdentityError::Storage(err.to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;
    use crate::security::Sha256IdentityFingerprintFactory;

    #[derive(Default)]
    struct InMemorySecureStorage {
        map: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl SecureStoragePort for InMemorySecureStorage {
        fn get(&self, key: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
            Ok(self.map.lock().unwrap().get(key).cloned())
        }

        fn set(&self, key: &str, value: &[u8]) -> Result<(), SecureStorageError> {
            self.map
                .lock()
                .unwrap()
                .insert(key.to_string(), value.to_vec());
            Ok(())
        }

        fn delete(&self, key: &str) -> Result<(), SecureStorageError> {
            self.map.lock().unwrap().remove(key);
            Ok(())
        }
    }

    fn make_store() -> (IrohIdentityStore, Arc<InMemorySecureStorage>) {
        let storage = Arc::new(InMemorySecureStorage::default());
        let factory = Arc::new(Sha256IdentityFingerprintFactory);
        let store = IrohIdentityStore::new(storage.clone(), factory);
        (store, storage)
    }

    #[tokio::test]
    async fn create_generates_identity_when_store_empty() {
        let (store, storage) = make_store();

        let fp = store.create().await.expect("create should succeed");

        assert_eq!(fp.as_raw().len(), 16);
        let persisted = storage
            .get(IDENTITY_STORE_KEY)
            .unwrap()
            .expect("secret persisted");
        assert_eq!(persisted.len(), SECRET_KEY_LEN);
    }

    #[tokio::test]
    async fn create_rejects_second_call() {
        let (store, _) = make_store();
        store.create().await.unwrap();

        let err = store.create().await.unwrap_err();
        assert!(matches!(err, LocalIdentityError::AlreadyExists));
    }

    #[tokio::test]
    async fn ensure_generates_when_empty() {
        let (store, storage) = make_store();

        let fp = store.ensure().await.expect("ensure generates");

        assert_eq!(fp.as_raw().len(), 16);
        assert!(storage.get(IDENTITY_STORE_KEY).unwrap().is_some());
    }

    #[tokio::test]
    async fn ensure_returns_existing_fingerprint_on_retry() {
        let (store, _) = make_store();
        let first = store.ensure().await.unwrap();

        let second = store.ensure().await.unwrap();

        assert_eq!(first, second);
    }

    #[tokio::test]
    async fn ensure_matches_create_for_same_store() {
        let (store, _) = make_store();
        let created = store.create().await.unwrap();

        let ensured = store.ensure().await.unwrap();

        assert_eq!(created, ensured);
    }

    #[tokio::test]
    async fn get_current_fingerprint_none_when_empty() {
        let (store, _) = make_store();

        let got = store.get_current_fingerprint().await.unwrap();

        assert!(got.is_none());
    }

    #[tokio::test]
    async fn get_current_fingerprint_matches_created() {
        let (store, _) = make_store();
        let fp = store.create().await.unwrap();

        let got = store
            .get_current_fingerprint()
            .await
            .unwrap()
            .expect("fingerprint present after create");

        assert_eq!(fp, got);
    }

    #[tokio::test]
    async fn corrupt_secret_length_maps_to_storage_error() {
        let (store, storage) = make_store();
        storage
            .set(IDENTITY_STORE_KEY, &vec![0u8; SECRET_KEY_LEN - 1])
            .unwrap();

        let err = store.get_current_fingerprint().await.unwrap_err();

        match err {
            LocalIdentityError::Storage(msg) => {
                assert!(msg.contains("corrupt iroh identity"), "msg was {msg}");
            }
            other => panic!("expected Storage variant, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn fingerprint_is_stable_across_loads() {
        let storage = Arc::new(InMemorySecureStorage::default());
        let factory = Arc::new(Sha256IdentityFingerprintFactory);

        let first_store = IrohIdentityStore::new(storage.clone(), factory.clone());
        let fp_a = first_store.create().await.unwrap();

        let second_store = IrohIdentityStore::new(storage.clone(), factory);
        let fp_b = second_store
            .get_current_fingerprint()
            .await
            .unwrap()
            .expect("persisted identity reloaded");

        assert_eq!(fp_a, fp_b);
    }
}
