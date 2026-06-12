//! 业务 blob 加解密 port。
//!
//! 在"已解锁的空间"上做数据面加解密——调用方出示 `ActiveSpace` 作为
//! 解锁凭据，adapter 内部按空间查找会话中的密钥物料。签名里不出现
//! MasterKey / 算法标签 / 版本号等基础设施概念。
//!
//! 合并了原先 `EncryptionPort::{encrypt_blob, decrypt_blob}` 的职责。
//! 传输分片场景（chunked + 内置压缩 + per-chunk wire-format AAD）已经
//! 由独立的 `TransferCipherPort` 承担——AAD 模型与 wire format 无法与
//! 本 port 共享，因此保持两套 port 并行。

use async_trait::async_trait;

use crate::crypto::domain::{Aad, ActiveSpace, Ciphertext, Plaintext};

/// 业务语义级的数据加解密失败。
///
/// 故意保持粗粒度——调用方一般只需要区分"还能不能继续用这个空间"和
/// "数据本身坏了"。算法细节 / AEAD tag 失败 / nonce 结构问题全部
/// 归到 `InvalidCiphertext`，由 adapter 在日志里补更细的信息。
#[derive(Debug, thiserror::Error)]
pub enum BlobCipherError {
    /// `ActiveSpace` 所对应的会话已经不再持有密钥（例如被 lock 过）。
    ///
    /// 正常情况下拿到 `ActiveSpace` 意味着已解锁；出现此错误通常代表
    /// 调用方把句柄抱过了 lock 边界——调用方应重新走 `SpaceAccessPort::unlock`。
    #[error("space session is no longer unlocked")]
    NotUnlocked,

    /// 密文本身损坏 / AAD 不匹配 / 解包失败——数据层故障。
    #[error("invalid ciphertext or aad mismatch")]
    InvalidCiphertext,

    /// 其它内部失败（底层算法库、IO 等）。
    #[error("blob cipher internal error: {0}")]
    Internal(String),
}

/// 业务 blob 加解密 port。
///
/// 方法契约：
/// - 加密成功返回 `Ciphertext`——不透明字节，包含 adapter 自描述的 nonce / tag 布局。
/// - 解密成功返回 `Plaintext`——drop 时自动清零。
/// - AAD 由调用方按业务规则构造（条目 id / 空间 id 等），adapter 原样写入 AEAD。
#[async_trait]
pub trait BlobCipherPort: Send + Sync {
    async fn encrypt(
        &self,
        space: &ActiveSpace,
        plaintext: &Plaintext,
        aad: &Aad,
    ) -> Result<Ciphertext, BlobCipherError>;

    async fn decrypt(
        &self,
        space: &ActiveSpace,
        ciphertext: &Ciphertext,
        aad: &Aad,
    ) -> Result<Plaintext, BlobCipherError>;
}
