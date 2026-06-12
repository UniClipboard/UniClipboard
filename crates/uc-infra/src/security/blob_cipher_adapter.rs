//! `BlobCipherPort` 的基础设施适配器（V1 加密：XChaCha20-Poly1305）。
//!
//! 端到端会话管理：内部持有 `EncryptionSessionPort`，自己完成
//! "会话就绪检查 + 取出 MasterKey + AEAD 加解密 + EncryptedBlob 序列化"。
//! 调用方只看到 `Plaintext` / `Ciphertext` / `Aad` 的进出。
//!
//! # Wire format
//!
//! `Ciphertext` 字节 = `serde_json::to_vec(&EncryptedBlob)`。这正是历史上
//! `EncryptionPort::encrypt_blob` 输出后被 4 个 decorators 经
//! `serde_json::to_vec(&encrypted_blob)` 写入 SQL inline_data 的字节布局——
//! 保留这个格式让 SQL 中既有的密文仍可被新 adapter 解开（V1 数据兼容
//! ironclad 不变量）。
//!
//! adapter 内部不依赖旧 `EncryptionPort`——AEAD 调用通过 `super::v1_aead`
//! 私有 helper 直接落地，跟 `EncryptionRepository` / `EncryptedBlobStore`
//! 共用同一份算法实现，杜绝行为漂移。

use async_trait::async_trait;
use std::sync::Arc;

use uc_core::crypto::domain::{Aad, ActiveSpace, Ciphertext, Plaintext};
use uc_core::ports::security::blob_cipher::{BlobCipherError, BlobCipherPort};

use super::crypto_model::EncryptedBlob;
use super::session::InMemorySession;
use super::v1_aead;

pub struct BlobCipherAdapter {
    session: Arc<InMemorySession>,
}

impl BlobCipherAdapter {
    pub fn new(session: Arc<InMemorySession>) -> Self {
        Self { session }
    }
}

#[async_trait]
impl BlobCipherPort for BlobCipherAdapter {
    async fn encrypt(
        &self,
        _space: &ActiveSpace,
        plaintext: &Plaintext,
        aad: &Aad,
    ) -> Result<Ciphertext, BlobCipherError> {
        // 当前单 master_key 模型: ActiveSpace 仅作"已解锁"语义担保,
        // 不参与会话查找。多空间路由后续扩展。
        if !self.session.is_ready() {
            return Err(BlobCipherError::NotUnlocked);
        }
        let master_key = self
            .session
            .get_master_key()
            .map_err(|e| BlobCipherError::Internal(e.to_string()))?;

        let blob = v1_aead::encrypt_blob_xchacha(&master_key, plaintext.as_bytes(), aad.as_bytes())
            .map_err(|e| BlobCipherError::Internal(e.to_string()))?;

        let bytes = serde_json::to_vec(&blob)
            .map_err(|e| BlobCipherError::Internal(format!("serialize EncryptedBlob: {e}")))?;
        Ok(Ciphertext::new(bytes))
    }

    async fn decrypt(
        &self,
        _space: &ActiveSpace,
        ciphertext: &Ciphertext,
        aad: &Aad,
    ) -> Result<Plaintext, BlobCipherError> {
        if !self.session.is_ready() {
            return Err(BlobCipherError::NotUnlocked);
        }

        let blob: EncryptedBlob = serde_json::from_slice(ciphertext.as_bytes())
            .map_err(|_| BlobCipherError::InvalidCiphertext)?;

        // V1 加密协议固定 XChaCha20-Poly1305 + version V1——其它分支当作密文损坏。
        if blob.aead != "XChaCha20Poly1305" || blob.version != "V1" {
            return Err(BlobCipherError::InvalidCiphertext);
        }

        let master_key = self
            .session
            .get_master_key()
            .map_err(|e| BlobCipherError::Internal(e.to_string()))?;

        let plain = v1_aead::decrypt_blob_xchacha(
            &master_key,
            &blob.nonce,
            &blob.ciphertext,
            aad.as_bytes(),
        )
        .map_err(|e| match e {
            v1_aead::AeadError::InvalidKey => BlobCipherError::Internal(e.to_string()),
            v1_aead::AeadError::DecryptFailed => BlobCipherError::InvalidCiphertext,
            v1_aead::AeadError::EncryptFailed => BlobCipherError::Internal(e.to_string()),
        })?;
        Ok(Plaintext::new(plain))
    }
}
