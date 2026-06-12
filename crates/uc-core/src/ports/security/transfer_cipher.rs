//! 剪切板网络传输加解密 port。
//!
//! 语义：在"当前进程已解锁的会话"上对剪切板网络字节做 AEAD 加解密。
//! adapter 内部完成"会话就绪检查 + 取出密钥 + 按内部 wire format
//! (chunked + zstd + AEAD) 处理"整条链路——调用方只看得到字节进出。
//!
//! 合并了原先 `TransferPayloadEncryptorPort` / `TransferPayloadDecryptorPort`
//! 两个 port 的职责；签名里不再出现 `MasterKey`——会话的"已解锁"语义
//! 由 adapter 内部持有的会话端口担保。
//!
//! 与 `BlobCipherPort` 的区别：
//! - 本 port 的 AAD 由 adapter 按 wire format 内部构造（per-chunk
//!   `transfer_id ‖ chunk_index`），调用方既无需也不可提供；
//! - 本 port 的 wire format（V3 `UC3\0` header + per-chunk 帧 + 内置 zstd）
//!   是传输专用，不与 `BlobCipherPort` 的落盘 AEAD 共享实现。

use async_trait::async_trait;

/// 业务语义级的传输加解密失败。
///
/// 故意保持粗粒度——调用方一般只需要区分"会话还能不能用"、
/// "数据是否损坏"以及"其它内部故障"。
#[derive(Debug, thiserror::Error)]
pub enum TransferCipherError {
    /// 当前没有已解锁的会话——调用方应先完成 unlock 再重试。
    #[error("encryption session is not unlocked")]
    NotUnlocked,

    /// 密文的 wire format 无法识别或已被篡改（含 AEAD tag 校验失败）。
    #[error("invalid transfer payload format")]
    InvalidFormat,

    /// 加密路径失败——通常来自底层算法库内部错误。
    #[error("transfer payload encryption failed")]
    EncryptionFailed,

    /// 解密路径失败——密钥不匹配 / chunk 失效等。
    #[error("transfer payload decryption failed")]
    DecryptionFailed,

    /// 其它不可恢复的内部故障。
    #[error("transfer cipher internal error: {0}")]
    Internal(String),
}

/// 剪切板传输加解密 port。
///
/// 方法契约：
/// - `encrypt` 将明文字节封装成传输专用 wire format 字节流；adapter 负责
///   分片 + 压缩 + AEAD，调用方不需要理解格式细节。
/// - `decrypt` 从 wire format 字节流反向还原明文。
/// - 两方法都要求 adapter 内部会话已解锁；未解锁返回
///   [`TransferCipherError::NotUnlocked`]，调用方应当作"跳过本次同步"处理。
#[async_trait]
pub trait TransferCipherPort: Send + Sync {
    async fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, TransferCipherError>;

    async fn decrypt(&self, encrypted: &[u8]) -> Result<Vec<u8>, TransferCipherError>;
}
