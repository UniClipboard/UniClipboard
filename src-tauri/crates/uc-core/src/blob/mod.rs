use crate::ids::BlobId;
use crate::security::EncryptionAlgo;
use crate::ContentHash;
use std::path::PathBuf;

/// 描述：
/// Blob 在「当前设备」上的存储定位方式
///
/// 重要约束：
/// - 不能跨设备使用
/// - 不能作为网络地址
/// - 不能推导 blob identity
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlobStorageLocator {
    LocalFs {
        /// 绝对路径
        path: PathBuf,
    },
    /// 本地文件系统 + 加密包裹
    ///
    /// 注意：
    /// - encryption 只描述“存储形态”
    /// - 不等价于传输加密
    EncryptedFs { path: PathBuf, algo: EncryptionAlgo },
}

impl BlobStorageLocator {
    pub fn new_local_fs(path: PathBuf) -> Self {
        BlobStorageLocator::LocalFs { path }
    }

    pub fn new_encrypted_fs(path: PathBuf, algo: EncryptionAlgo) -> Self {
        BlobStorageLocator::EncryptedFs { path, algo }
    }
}

#[derive(Debug, Clone)]
pub struct Blob {
    pub blob_id: BlobId,
    pub locator: BlobStorageLocator,
    pub size_bytes: i64,
    pub content_hash: ContentHash,
    pub created_at_ms: i64,
    /// On-disk byte count after compression+encryption, if applicable.
    /// `None` for uncompressed or inline data.
    pub compressed_size: Option<i64>,
}

impl Blob {
    pub fn new(
        blob_id: BlobId,
        locator: BlobStorageLocator,
        size_bytes: i64,
        content_hash: ContentHash,
        created_at_ms: i64,
        compressed_size: Option<i64>,
    ) -> Self {
        Self {
            blob_id,
            locator,
            size_bytes,
            content_hash,
            created_at_ms,
            compressed_size,
        }
    }
}
