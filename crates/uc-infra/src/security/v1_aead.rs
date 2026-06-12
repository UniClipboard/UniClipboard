//! V1 加密原语集中点（pub(crate)）。
//!
//! 把 KEK 派生 / MasterKey 包装拆解 / blob AEAD 三组算法封装成纯函数,
//! 供 `BlobCipherAdapter` / `EncryptedBlobStore` / 后续 SpaceAccessAdapter
//! 内联使用。算法行为与历史 `EncryptionRepository` 一致——Argon2id KDF +
//! XChaCha20-Poly1305 AEAD,nonce 为 24 字节随机。
//!
//! V1 加密不变量 ironclad 保留——这里只是把同一份算法搬到一处,杜绝
//! 多个 adapter 各自实现的行为漂移风险。
//!
//! 注：本 commit (Slice 3 - C2) 只让 `BlobCipherAdapter` 消费
//! `encrypt_blob_xchacha` / `decrypt_blob_xchacha`；其余 3 个函数
//! （derive_kek_argon2id / wrap_master_key_xchacha / unwrap_master_key_xchacha）
//! 在 C8 删除 `EncryptionPort` 后由 SpaceAccessAdapter 内联消费,
//! 期间允许 dead_code。

#![allow(dead_code)]

use argon2::Argon2;
use chacha20poly1305::aead::Aead;
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use rand::RngCore;
use uc_core::crypto::model::Passphrase;

use super::crypto_model::{EncryptedBlob, KdfParams};
use super::secrets::{Kek, MasterKey};

// 字面值常量——与历史 serde enum 输出字节级一致,为磁盘/wire format ironclad
// 不变量(Slice 4 B.4.1-3 删除四个单变体 enum 后从 adapter 端硬编码)。
const KDF_ALG_ARGON2ID: &str = "Argon2id";
const AEAD_XCHACHA20_POLY1305: &str = "XChaCha20Poly1305";
const ENCRYPTION_FORMAT_V1: &str = "V1";

#[derive(Debug, thiserror::Error)]
pub(crate) enum AeadError {
    #[error("invalid key length")]
    InvalidKey,
    #[error("AEAD encryption failed")]
    EncryptFailed,
    #[error("AEAD decryption failed (key mismatch / corrupted ciphertext)")]
    DecryptFailed,
}

/// Argon2id 派生 KEK。
pub(crate) fn derive_kek_argon2id(
    passphrase: &Passphrase,
    salt: &[u8],
    kdf: &KdfParams,
) -> Result<Kek, String> {
    if kdf.alg != KDF_ALG_ARGON2ID {
        return Err(format!("unsupported KDF algorithm: {}", kdf.alg));
    }
    let argon2 = Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        argon2::Params::new(
            kdf.params.mem_kib,
            kdf.params.iters,
            kdf.params.parallelism,
            Some(32),
        )
        .map_err(|e| format!("argon2 params: {e}"))?,
    );
    let mut okm = [0u8; 32];
    argon2
        .hash_password_into(passphrase.as_bytes(), salt, &mut okm)
        .map_err(|e| format!("argon2 hash: {e}"))?;
    Kek::from_bytes(&okm).map_err(|e| format!("Kek::from_bytes: {e}"))
}

/// XChaCha20-Poly1305 包装 MasterKey。
///
/// 输出 `EncryptedBlob` 直接对接 KeySlot.wrapped_master_key（V1 格式）。
pub(crate) fn wrap_master_key_xchacha(
    kek: &Kek,
    master_key: &MasterKey,
) -> Result<EncryptedBlob, AeadError> {
    let mut nonce = vec![0u8; 24];
    rand::rng().fill_bytes(&mut nonce);

    let cipher =
        XChaCha20Poly1305::new_from_slice(kek.as_bytes()).map_err(|_| AeadError::InvalidKey)?;
    let ciphertext = cipher
        .encrypt(XNonce::from_slice(&nonce), master_key.as_bytes())
        .map_err(|_| AeadError::EncryptFailed)?;

    Ok(EncryptedBlob {
        version: ENCRYPTION_FORMAT_V1.to_string(),
        aead: AEAD_XCHACHA20_POLY1305.to_string(),
        nonce,
        ciphertext,
        aad_fingerprint: None,
    })
}

/// XChaCha20-Poly1305 解包 MasterKey。
pub(crate) fn unwrap_master_key_xchacha(
    kek: &Kek,
    wrapped: &EncryptedBlob,
) -> Result<MasterKey, AeadError> {
    let cipher =
        XChaCha20Poly1305::new_from_slice(kek.as_bytes()).map_err(|_| AeadError::InvalidKey)?;
    let plaintext = cipher
        .decrypt(
            XNonce::from_slice(&wrapped.nonce),
            wrapped.ciphertext.as_ref(),
        )
        .map_err(|_| AeadError::DecryptFailed)?;
    MasterKey::from_bytes(&plaintext).map_err(|_| AeadError::DecryptFailed)
}

/// XChaCha20-Poly1305 加密业务 blob,返回完整的 `EncryptedBlob`。
///
/// 调用方决定 AAD（业务上下文绑定,例如条目 id / blob id）。
pub(crate) fn encrypt_blob_xchacha(
    master_key: &MasterKey,
    plaintext: &[u8],
    aad: &[u8],
) -> Result<EncryptedBlob, AeadError> {
    let mut nonce = vec![0u8; 24];
    rand::rng().fill_bytes(&mut nonce);

    let cipher = XChaCha20Poly1305::new_from_slice(master_key.as_bytes())
        .map_err(|_| AeadError::InvalidKey)?;
    let ciphertext = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            chacha20poly1305::aead::Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| AeadError::EncryptFailed)?;

    let aad_fp = Some(blake3::hash(aad).as_bytes()[..16].to_vec());

    Ok(EncryptedBlob {
        version: ENCRYPTION_FORMAT_V1.to_string(),
        aead: AEAD_XCHACHA20_POLY1305.to_string(),
        nonce,
        ciphertext,
        aad_fingerprint: aad_fp,
    })
}

/// XChaCha20-Poly1305 解密业务 blob。
///
/// 接收 nonce + ciphertext + AAD 三件直接调底层 AEAD,绕过 EncryptedBlob
/// 结构（让 EncryptedBlobStore 用 UCBL 二进制头时也能直接消费）。
pub(crate) fn decrypt_blob_xchacha(
    master_key: &MasterKey,
    nonce: &[u8],
    ciphertext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, AeadError> {
    if nonce.len() != 24 {
        return Err(AeadError::DecryptFailed);
    }
    let cipher = XChaCha20Poly1305::new_from_slice(master_key.as_bytes())
        .map_err(|_| AeadError::InvalidKey)?;
    cipher
        .decrypt(
            XNonce::from_slice(nonce),
            chacha20poly1305::aead::Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| AeadError::DecryptFailed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::crypto_model::KdfParamsV1;
    use uc_core::crypto::model::Passphrase;

    /// 单测专用的廉价 Argon2id 参数。生产默认是 128 MiB / 3 iters,在单测里
    /// 跑不起;`mem_kib = 8` 是 `parallelism = 1` 下 Argon2 的内存下限
    /// (`m_cost >= 8 * lanes`)。只验证 KDF 行为契约,不验证安全强度。
    fn cheap_kdf() -> KdfParams {
        KdfParams {
            alg: "Argon2id".to_string(),
            params: KdfParamsV1 {
                mem_kib: 8,
                iters: 1,
                parallelism: 1,
            },
        }
    }

    fn master_key(seed: u8) -> MasterKey {
        MasterKey::from_bytes(&[seed; 32]).unwrap()
    }

    fn kek(seed: u8) -> Kek {
        Kek::from_bytes(&[seed; 32]).unwrap()
    }

    // ── derive_kek_argon2id ──────────────────────────────────────────────

    #[test]
    fn derive_kek_is_deterministic_for_same_inputs() {
        let pass = Passphrase("correct horse".to_string());
        let salt = [0x11u8; 16];
        let a = derive_kek_argon2id(&pass, &salt, &cheap_kdf()).unwrap();
        let b = derive_kek_argon2id(&pass, &salt, &cheap_kdf()).unwrap();
        assert_eq!(a.as_bytes(), b.as_bytes(), "KDF must be deterministic");
    }

    #[test]
    fn derive_kek_differs_for_different_passphrase() {
        let salt = [0x22u8; 16];
        let a = derive_kek_argon2id(&Passphrase("alpha".into()), &salt, &cheap_kdf()).unwrap();
        let b = derive_kek_argon2id(&Passphrase("bravo".into()), &salt, &cheap_kdf()).unwrap();
        assert_ne!(a.as_bytes(), b.as_bytes());
    }

    #[test]
    fn derive_kek_differs_for_different_salt() {
        let pass = Passphrase("same".to_string());
        let a = derive_kek_argon2id(&pass, &[0x01u8; 16], &cheap_kdf()).unwrap();
        let b = derive_kek_argon2id(&pass, &[0x02u8; 16], &cheap_kdf()).unwrap();
        assert_ne!(a.as_bytes(), b.as_bytes());
    }

    #[test]
    fn derive_kek_rejects_unsupported_algorithm() {
        let mut kdf = cheap_kdf();
        kdf.alg = "scrypt".to_string();
        let err = derive_kek_argon2id(&Passphrase("x".into()), &[0u8; 16], &kdf).unwrap_err();
        assert!(err.contains("unsupported KDF"), "got: {err}");
    }

    // ── wrap / unwrap master key ─────────────────────────────────────────

    #[test]
    fn wrap_unwrap_master_key_round_trips() {
        let kek = kek(0xAB);
        let mk = master_key(0xCD);
        let wrapped = wrap_master_key_xchacha(&kek, &mk).unwrap();

        assert_eq!(wrapped.version, "V1");
        assert_eq!(wrapped.aead, "XChaCha20Poly1305");
        assert_eq!(wrapped.nonce.len(), 24);

        let unwrapped = unwrap_master_key_xchacha(&kek, &wrapped).unwrap();
        assert_eq!(unwrapped.as_bytes(), mk.as_bytes());
    }

    #[test]
    fn unwrap_master_key_with_wrong_kek_fails() {
        let wrapped = wrap_master_key_xchacha(&kek(0x01), &master_key(0x02)).unwrap();
        let err = unwrap_master_key_xchacha(&kek(0x09), &wrapped).unwrap_err();
        assert!(matches!(err, AeadError::DecryptFailed));
    }

    #[test]
    fn unwrap_master_key_with_tampered_ciphertext_fails() {
        let kek = kek(0x05);
        let mut wrapped = wrap_master_key_xchacha(&kek, &master_key(0x06)).unwrap();
        wrapped.ciphertext[0] ^= 0xFF;
        let err = unwrap_master_key_xchacha(&kek, &wrapped).unwrap_err();
        assert!(matches!(err, AeadError::DecryptFailed));
    }

    #[test]
    fn wrap_master_key_uses_fresh_random_nonce() {
        let kek = kek(0x07);
        let mk = master_key(0x08);
        let a = wrap_master_key_xchacha(&kek, &mk).unwrap();
        let b = wrap_master_key_xchacha(&kek, &mk).unwrap();
        assert_ne!(a.nonce, b.nonce, "nonce must be random per call");
        assert_ne!(
            a.ciphertext, b.ciphertext,
            "same plaintext must not yield identical ciphertext"
        );
    }

    // ── encrypt / decrypt blob ───────────────────────────────────────────

    #[test]
    fn encrypt_decrypt_blob_round_trips_with_aad() {
        let mk = master_key(0x33);
        let plaintext = b"clipboard payload";
        let aad = b"entry-42";
        let blob = encrypt_blob_xchacha(&mk, plaintext, aad).unwrap();

        assert_eq!(blob.version, "V1");
        assert_eq!(blob.aead, "XChaCha20Poly1305");
        assert_eq!(blob.nonce.len(), 24);

        let recovered = decrypt_blob_xchacha(&mk, &blob.nonce, &blob.ciphertext, aad).unwrap();
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn encrypt_blob_sets_truncated_blake3_aad_fingerprint() {
        let aad = b"entry-99";
        let blob = encrypt_blob_xchacha(&master_key(0x44), b"x", aad).unwrap();
        let expected = blake3::hash(aad).as_bytes()[..16].to_vec();
        assert_eq!(blob.aad_fingerprint.as_deref(), Some(expected.as_slice()));
    }

    #[test]
    fn decrypt_blob_with_wrong_aad_fails() {
        let mk = master_key(0x55);
        let blob = encrypt_blob_xchacha(&mk, b"secret", b"good-aad").unwrap();
        let err = decrypt_blob_xchacha(&mk, &blob.nonce, &blob.ciphertext, b"bad-aad").unwrap_err();
        assert!(matches!(err, AeadError::DecryptFailed));
    }

    #[test]
    fn decrypt_blob_with_wrong_key_fails() {
        let blob = encrypt_blob_xchacha(&master_key(0x01), b"secret", b"aad").unwrap();
        let err = decrypt_blob_xchacha(&master_key(0x02), &blob.nonce, &blob.ciphertext, b"aad")
            .unwrap_err();
        assert!(matches!(err, AeadError::DecryptFailed));
    }

    #[test]
    fn decrypt_blob_with_tampered_ciphertext_fails() {
        let mk = master_key(0x66);
        let blob = encrypt_blob_xchacha(&mk, b"secret", b"aad").unwrap();
        let mut ct = blob.ciphertext.clone();
        ct[0] ^= 0x01;
        let err = decrypt_blob_xchacha(&mk, &blob.nonce, &ct, b"aad").unwrap_err();
        assert!(matches!(err, AeadError::DecryptFailed));
    }

    #[test]
    fn decrypt_blob_rejects_nonce_of_wrong_length() {
        let mk = master_key(0x77);
        let blob = encrypt_blob_xchacha(&mk, b"secret", b"aad").unwrap();
        let short_nonce = vec![0u8; 12];
        let err = decrypt_blob_xchacha(&mk, &short_nonce, &blob.ciphertext, b"aad").unwrap_err();
        assert!(matches!(err, AeadError::DecryptFailed));
    }

    #[test]
    fn encrypt_blob_round_trips_empty_plaintext() {
        let mk = master_key(0x88);
        let blob = encrypt_blob_xchacha(&mk, b"", b"aad").unwrap();
        let recovered = decrypt_blob_xchacha(&mk, &blob.nonce, &blob.ciphertext, b"aad").unwrap();
        assert!(recovered.is_empty());
    }

    #[test]
    fn encrypt_blob_uses_fresh_random_nonce() {
        let mk = master_key(0x99);
        let a = encrypt_blob_xchacha(&mk, b"dup", b"aad").unwrap();
        let b = encrypt_blob_xchacha(&mk, b"dup", b"aad").unwrap();
        assert_ne!(a.nonce, b.nonce);
        assert_ne!(a.ciphertext, b.ciphertext);
    }

    /// 端到端贯通:Argon2id 派生 KEK → wrap MasterKey → unwrap 还原,证明
    /// `derive_kek` 与 `wrap/unwrap` 三者的 key/nonce 约定彼此一致。
    #[test]
    fn derive_then_wrap_then_unwrap_round_trips() {
        let pass = Passphrase("unlock me".to_string());
        let salt = [0x5Au8; 16];
        let kek = derive_kek_argon2id(&pass, &salt, &cheap_kdf()).unwrap();
        let mk = master_key(0x3C);

        let wrapped = wrap_master_key_xchacha(&kek, &mk).unwrap();
        let unwrapped = unwrap_master_key_xchacha(&kek, &wrapped).unwrap();
        assert_eq!(unwrapped.as_bytes(), mk.as_bytes());

        // 用从同口令+盐重新派生的 KEK 也能解(确定性 KDF 的端到端体现)。
        let kek_again = derive_kek_argon2id(&pass, &salt, &cheap_kdf()).unwrap();
        let unwrapped_again = unwrap_master_key_xchacha(&kek_again, &wrapped).unwrap();
        assert_eq!(unwrapped_again.as_bytes(), mk.as_bytes());
    }
}
