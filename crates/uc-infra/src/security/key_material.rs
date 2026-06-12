//! KeyMaterialStore——keyring (KEK) + 磁盘 (KeySlot) 的统一存取入口。
//!
//! Slice 3 - C8 起作为 uc-infra 内部具体类型存在(原 `KeyMaterialPort` trait
//! 已删除)；唯一消费者是 `DefaultSpaceAccessAdapter`,后者通过 Arc 共享。

use std::sync::Arc;
use uc_core::{
    crypto::model::EncryptionError,
    ports::{SecureStorageError, SecureStoragePort},
};

use crate::fs::key_slot_store::KeySlotStore;
use crate::security::crypto_model::{KeyScope, KeySlot, KeySlotFile};
use crate::security::scope_identifier::scope_identifier;
use crate::security::secrets::Kek;

pub struct KeyMaterialStore {
    secure_storage: Arc<dyn SecureStoragePort>,
    keyslot_store: Arc<dyn KeySlotStore>,
}

impl KeyMaterialStore {
    pub fn new(
        secure_storage: Arc<dyn SecureStoragePort>,
        keyslot_store: Arc<dyn KeySlotStore>,
    ) -> Self {
        Self {
            secure_storage,
            keyslot_store,
        }
    }
}

fn kek_key(scope: &KeyScope) -> String {
    format!("kek:v1:{}", scope_identifier(scope))
}

fn map_storage_error(err: SecureStorageError) -> EncryptionError {
    match err {
        SecureStorageError::PermissionDenied(_) => EncryptionError::PermissionDenied,
        SecureStorageError::Corrupt(_) => EncryptionError::KeyMaterialCorrupt,
        SecureStorageError::Unavailable(msg) | SecureStorageError::Other(msg) => {
            EncryptionError::KeyringError(msg)
        }
    }
}

impl KeyMaterialStore {
    pub async fn load_kek(&self, scope: &KeyScope) -> Result<Kek, EncryptionError> {
        let key = kek_key(scope);
        let secret = self
            .secure_storage
            .get(&key)
            .map_err(map_storage_error)?
            .ok_or(EncryptionError::KeyNotFound)?;
        Kek::from_bytes(&secret)
            .map_err(|e| EncryptionError::KeyringError(format!("invalid KEK material: {e}")))
    }

    pub async fn store_kek(&self, scope: &KeyScope, kek: &Kek) -> Result<(), EncryptionError> {
        let key = kek_key(scope);
        self.secure_storage
            .set(&key, kek.as_bytes())
            .map_err(map_storage_error)
    }

    pub async fn delete_kek(&self, scope: &KeyScope) -> Result<(), EncryptionError> {
        let key = kek_key(scope);
        self.secure_storage.delete(&key).map_err(map_storage_error)
    }

    pub async fn load_keyslot(&self, scope: &KeyScope) -> Result<KeySlot, EncryptionError> {
        let file = self.keyslot_store.load().await?;
        if &file.scope != scope {
            return Err(EncryptionError::KeyMaterialCorrupt);
        }
        Ok(file.into())
    }

    /// 本机磁盘上是否存在 keyslot 文件(任意 scope)。取代 Phase C 前的
    /// `EncryptionStatePort.load_state() == Initialized` 判断:从"是否写过
    /// marker 文件"改成"是否真的有 keyslot",更精确。
    pub async fn keyslot_exists(&self) -> Result<bool, EncryptionError> {
        match self.keyslot_store.load().await {
            Ok(_) => Ok(true),
            Err(EncryptionError::KeyNotFound) => Ok(false),
            Err(other) => Err(other),
        }
    }

    pub async fn store_keyslot(&self, keyslot: &KeySlot) -> Result<(), EncryptionError> {
        let file = KeySlotFile::try_from(keyslot).map_err(|_| EncryptionError::CorruptedKeySlot)?;
        self.keyslot_store.store(&file).await
    }

    pub async fn delete_keyslot(&self, scope: &KeyScope) -> Result<(), EncryptionError> {
        let file = self.keyslot_store.load().await?;
        if &file.scope != scope {
            return Err(EncryptionError::KeyMaterialCorrupt);
        }
        self.keyslot_store.delete().await
    }
}
