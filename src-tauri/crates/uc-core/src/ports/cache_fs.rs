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

/// Metadata for a single filesystem path.
/// 单个文件系统路径的元数据。
#[derive(Debug, Clone)]
pub struct FileMetadata {
    /// Size in bytes (meaningful for regular files).
    pub size_bytes: u64,
    /// Whether the path is a directory.
    pub is_dir: bool,
    /// Last-modified time as milliseconds since the Unix epoch, or `None`
    /// when the platform/filesystem does not expose a modified time.
    pub modified_unix_ms: Option<i64>,
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

    /// Read a file's entire contents.
    /// 读取文件的全部内容。
    ///
    /// Returns `Ok(None)` when the file does not exist, `Ok(Some(bytes))`
    /// with its contents otherwise, and `Err` for any other read failure
    /// (e.g. permission denied, or the path is a directory). The
    /// absent-vs-unreadable split lets callers treat a missing file as a
    /// distinct, non-error state.
    async fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>>;

    /// Write `contents` to a file, creating it or truncating an existing
    /// file. Parent directories are assumed to exist.
    /// 将内容写入文件，文件不存在则创建、已存在则截断覆盖。假定父目录已存在。
    async fn write_file(&self, path: &Path, contents: &[u8]) -> Result<()>;

    /// Read metadata for a single path.
    /// 读取单个路径的元数据。
    ///
    /// Returns `Ok(None)` when the path does not exist; `Err` for any other
    /// failure (e.g. permission denied).
    async fn metadata(&self, path: &Path) -> Result<Option<FileMetadata>>;

    /// Remove a single empty directory.
    /// 删除单个空目录。
    ///
    /// Errors if the directory is not empty (callers that want a recursive
    /// delete use [`Self::remove_dir_all`]).
    async fn remove_dir(&self, path: &Path) -> Result<()>;
}
