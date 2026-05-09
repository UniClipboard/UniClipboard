//! Disk spool manager for representation bytes.
//! 表示字节的磁盘缓存管理器。

use std::io;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result};
use tokio::fs;
use uc_core::ids::RepresentationId;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Disk spool manager with size limits.
/// 具备容量上限的磁盘缓存管理器。
pub struct SpoolManager {
    spool_dir: PathBuf,
    max_bytes: usize,
}

/// Spool entry metadata.
/// 缓存条目元数据。
pub struct SpoolEntry {
    pub representation_id: RepresentationId,
    pub file_path: PathBuf,
    pub size: usize,
}

/// Spool entry metadata with modified time.
/// 包含修改时间的缓存条目元数据。
pub struct SpoolEntryMeta {
    pub representation_id: RepresentationId,
    pub file_path: PathBuf,
    pub size: usize,
    pub modified_ms: i64,
}

impl SpoolManager {
    /// Create a new spool manager and ensure directory exists.
    /// 创建新的磁盘缓存管理器并确保目录存在。
    pub fn new(spool_dir: impl Into<PathBuf>, max_bytes: usize) -> Result<Self> {
        let spool_dir = spool_dir.into();

        std::fs::create_dir_all(&spool_dir)
            .with_context(|| format!("Failed to create spool dir: {}", spool_dir.display()))?;

        let metadata = std::fs::metadata(&spool_dir).with_context(|| {
            format!("Failed to read spool dir metadata: {}", spool_dir.display())
        })?;
        if !metadata.is_dir() {
            return Err(anyhow::anyhow!(
                "Spool path is not a directory: {}",
                spool_dir.display()
            ));
        }

        #[cfg(unix)]
        {
            let perms = std::fs::Permissions::from_mode(0o700);
            std::fs::set_permissions(&spool_dir, perms).with_context(|| {
                format!(
                    "Failed to set spool dir permissions: {}",
                    spool_dir.display()
                )
            })?;
        }

        Ok(Self {
            spool_dir,
            max_bytes,
        })
    }

    /// Write bytes to spool, returning the entry metadata.
    /// 写入缓存并返回条目元数据。
    pub async fn write(&self, rep_id: &RepresentationId, bytes: &[u8]) -> Result<SpoolEntry> {
        // Reject entries larger than max_bytes to avoid returning a SpoolEntry
        // for a file that will be immediately evicted by enforce_limits()
        if bytes.len() > self.max_bytes {
            return Err(anyhow::anyhow!(
                "Spool entry size {} bytes exceeds max_bytes {}",
                bytes.len(),
                self.max_bytes
            ));
        }

        let file_path = self.spool_dir.join(rep_id.to_string());

        fs::write(&file_path, bytes)
            .await
            .with_context(|| format!("Failed to write spool file: {}", file_path.display()))?;

        #[cfg(unix)]
        {
            let perms = std::fs::Permissions::from_mode(0o600);
            fs::set_permissions(&file_path, perms)
                .await
                .with_context(|| {
                    format!(
                        "Failed to set spool file permissions: {}",
                        file_path.display()
                    )
                })?;
        }

        // Enforce limits AFTER writing, excluding the newly written file from eviction
        // (count it in total, but don't evict it)
        self.enforce_limits_excluding(Some(rep_id)).await?;

        Ok(SpoolEntry {
            representation_id: rep_id.clone(),
            file_path,
            size: bytes.len(),
        })
    }

    /// Read bytes from spool. Returns None if missing.
    /// 读取缓存字节，若不存在则返回 None。
    pub async fn read(&self, rep_id: &RepresentationId) -> Result<Option<Vec<u8>>> {
        let file_path = self.spool_dir.join(rep_id.to_string());
        match fs::read(&file_path).await {
            Ok(bytes) => Ok(Some(bytes)),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err)
                .with_context(|| format!("Failed to read spool file: {}", file_path.display())),
        }
    }

    /// Whether a spool file exists for `rep_id`. Lighter than `read()` when
    /// the caller only needs presence (e.g. `StagedReconciler`).
    /// 检查缓存中是否存在该表示的字节，比 `read()` 轻量。
    pub async fn exists(&self, rep_id: &RepresentationId) -> Result<bool> {
        let file_path = self.spool_dir.join(rep_id.to_string());
        match fs::metadata(&file_path).await {
            Ok(meta) => Ok(meta.is_file()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err)
                .with_context(|| format!("Failed to stat spool file: {}", file_path.display())),
        }
    }

    /// Delete spool entry. Missing file is treated as success.
    /// 删除缓存条目，若不存在则视为成功。
    pub async fn delete(&self, rep_id: &RepresentationId) -> Result<()> {
        let file_path = self.spool_dir.join(rep_id.to_string());
        match fs::remove_file(&file_path).await {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err)
                .with_context(|| format!("Failed to delete spool file: {}", file_path.display())),
        }
    }

    /// Maximum bytes configured for the spool.
    /// 配置的最大字节数。
    pub fn max_bytes(&self) -> usize {
        self.max_bytes
    }

    async fn list_entries_by_mtime(&self) -> Result<Vec<SpoolEntryMeta>> {
        let mut entries = Vec::new();
        let mut dir = fs::read_dir(&self.spool_dir).await?;
        while let Some(entry) = dir.next_entry().await? {
            let meta = entry.metadata().await?;
            if !meta.is_file() {
                continue;
            }
            let file_name = entry.file_name();
            let Some(name) = file_name.to_str() else {
                tracing::warn!("Skipping spool entry with non-utf8 filename");
                continue;
            };
            let modified = meta.modified()?;
            let modified_ms = modified
                .duration_since(UNIX_EPOCH)
                .map_err(|err| anyhow::anyhow!("invalid mtime: {err}"))?
                .as_millis() as i64;
            entries.push(SpoolEntryMeta {
                representation_id: RepresentationId::from_str(name),
                file_path: entry.path(),
                size: meta.len() as usize,
                modified_ms,
            });
        }
        // Sort by mtime, then by representation_id for deterministic ordering
        // when mtimes are equal (e.g., low-resolution filesystem timestamps)
        entries.sort_by(|a, b| {
            a.modified_ms
                .cmp(&b.modified_ms)
                .then_with(|| a.representation_id.cmp(&b.representation_id))
        });
        Ok(entries)
    }

    async fn enforce_limits_excluding(&self, exclude_id: Option<&RepresentationId>) -> Result<()> {
        let mut entries = self.list_entries_by_mtime().await?;
        let mut total_bytes = entries.iter().map(|entry| entry.size).sum::<usize>();

        while total_bytes > self.max_bytes {
            // Find the oldest entry that is NOT excluded
            let exclude_idx = entries
                .iter()
                .position(|e| exclude_id.map_or(false, |exclude| &e.representation_id == exclude));

            let evict_idx = if exclude_idx == Some(0) {
                // If the first entry is excluded, try the next one
                if entries.len() > 1 {
                    Some(1)
                } else {
                    None
                }
            } else {
                // Otherwise, evict the first (oldest) entry
                exclude_idx
                    .and_then(|i| if i == 0 { None } else { Some(0) })
                    .or(Some(0))
            };

            let Some(idx) = evict_idx else {
                break;
            };
            let oldest = &entries[idx];
            fs::remove_file(&oldest.file_path).await?;
            total_bytes = total_bytes.saturating_sub(oldest.size);
            entries.remove(idx);
        }
        Ok(())
    }

    /// List spool entries expired by TTL.
    /// 枚举超过 TTL 的缓存条目。
    pub async fn list_expired(&self, now_ms: i64, ttl_days: u64) -> Result<Vec<SpoolEntryMeta>> {
        let ttl_ms = (ttl_days as i64) * 24 * 60 * 60 * 1000;
        let mut expired = Vec::new();
        for entry in self.list_entries_by_mtime().await? {
            if now_ms - entry.modified_ms > ttl_ms {
                expired.push(entry);
            }
        }
        Ok(expired)
    }
}
