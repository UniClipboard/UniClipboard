//! Port for cache filesystem operations.
//! 缓存文件系统操作的端口。

use std::path::{Path, PathBuf};

use anyhow::Result;
use async_trait::async_trait;

/// Entry in a directory listing.
/// 目录列表中的条目。
#[derive(Debug, Clone)]
pub struct DirEntry {
    pub path: PathBuf,
    pub is_dir: bool,
}

/// Port for filesystem operations needed by cache management use cases.
/// 缓存管理用例所需的文件系统操作端口。
#[async_trait]
pub trait CacheFsPort: Send + Sync {
    /// Check whether a path exists.
    /// 检查路径是否存在。
    async fn exists(&self, path: &Path) -> bool;

    /// List immediate children of a directory.
    /// 列出目录的直接子条目。
    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>>;

    /// Recursively remove a directory and all its contents.
    /// 递归删除目录及其所有内容。
    async fn remove_dir_all(&self, path: &Path) -> Result<()>;

    /// Remove a single file.
    /// 删除单个文件。
    async fn remove_file(&self, path: &Path) -> Result<()>;

    /// Recursively calculate the size of a path in bytes.
    /// 递归计算路径的大小（字节数）。
    ///
    /// Returns `Ok(0)` for non-existent paths. Returns an error if a path
    /// exists but cannot be read (e.g. permission denied).
    async fn dir_size(&self, path: &Path) -> Result<u64>;
}
