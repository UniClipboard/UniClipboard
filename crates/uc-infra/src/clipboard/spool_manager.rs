//! Disk spool manager for representation bytes.
//! 表示字节的磁盘缓存管理器。
//!
//! ## 容量管理为什么不再扫目录
//!
//! 早期实现 `write()` 末尾调用 `enforce_limits_excluding()`，每次写入都做
//! `fs::read_dir` + 逐条 `entry.metadata()` 扫整个 spool 目录来判断是否超限。
//! 这条路径与后台 worker (`BackgroundBlobWorker`) 的 `spool.delete()` 天然
//! race：worker 删完一个文件，主线程的目录迭代恰好走到那个 dirent，
//! `entry.metadata()` 返回 ENOENT，错误一路抛回 capture，整次剪切板捕获
//! 失败、entry 不入库 —— 用户看不到这条记录。
//!
//! 结构性修复：把"容量约束"从"目录扫描"解耦成"内存计数"。`SpoolManager`
//! 内部用 `Mutex<SpoolState>` 维护一份 `IndexMap<RepresentationId, usize>`
//! 作为单一权威账本：
//!
//! * `write()` 路径：O(1) 检查 `tracked_total + bytes.len()`，超限时按
//!   插入顺序 evict 最旧的 tracked entry（只动磁盘上的目标文件 + 内存计数，
//!   不再扫整个目录）。
//! * `delete()` 路径：同步更新内存计数。
//! * 启动时：`SpoolManager::new()` 扫一遍 spool 目录把现有文件加入账本，
//!   用于进程重启后恢复。这是唯一一次目录扫描，发生在没有 worker 并发的
//!   启动时序里，不存在 race。
//!
//! `list_entries_by_mtime()` 仍然存在，但现在只服务于 `list_expired()`
//! （`SpoolJanitor` 的 TTL 清理路径），且对单个 entry 的 `metadata` ENOENT
//! 容忍跳过 —— 与本文件中 `read()` / `exists()` / `delete()` 已经做的 ENOENT
//! 容忍保持一致。这条原则的来源是"目录迭代在并发文件系统里无法保证
//! dirent → metadata 原子性"。

use std::io;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result};
use indexmap::IndexMap;
use tokio::fs;
use uc_core::ids::RepresentationId;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// 磁盘缓存管理器。
pub struct SpoolManager {
    spool_dir: PathBuf,
    max_bytes: usize,
    state: Mutex<SpoolState>,
}

/// In-memory 账本：insertion-ordered 表示条目 + 累计字节数。
/// 用 `IndexMap` 而不是 `HashMap` 因为容量驱逐需要稳定的 FIFO 顺序
/// （近似 write order，等价于粗粒度 mtime asc）。
struct SpoolState {
    entries: IndexMap<RepresentationId, usize>,
    total_bytes: usize,
}

impl SpoolState {
    fn new() -> Self {
        Self {
            entries: IndexMap::new(),
            total_bytes: 0,
        }
    }

    /// 在登记新条目前先扣减同 id 旧条目的 size（处理覆写场景）。
    /// 返回 true 表示存在旧值。
    fn unregister(&mut self, rep_id: &RepresentationId) -> bool {
        if let Some(old_size) = self.entries.shift_remove(rep_id) {
            self.total_bytes = self.total_bytes.saturating_sub(old_size);
            true
        } else {
            false
        }
    }

    fn register(&mut self, rep_id: RepresentationId, size: usize) {
        // 同 id 已经存在时 shift_remove + insert 等价于"刷新到尾部"，
        // 让重写过的条目在 eviction 顺序里被视为最新写入。
        if let Some(old_size) = self.entries.shift_remove(&rep_id) {
            self.total_bytes = self.total_bytes.saturating_sub(old_size);
        }
        self.entries.insert(rep_id, size);
        self.total_bytes = self.total_bytes.saturating_add(size);
    }

    /// 弹出 insertion order 最早的、且不等于 `exclude_id` 的条目。
    /// 返回 (RepresentationId, size)。无可弹出时返回 None。
    fn pop_oldest_except(
        &mut self,
        exclude_id: Option<&RepresentationId>,
    ) -> Option<(RepresentationId, usize)> {
        let victim_index = self
            .entries
            .iter()
            .position(|(id, _)| exclude_id.map_or(true, |excl| id != excl))?;
        let (id, size) = self.entries.shift_remove_index(victim_index)?;
        self.total_bytes = self.total_bytes.saturating_sub(size);
        Some((id, size))
    }
}

/// Spool 条目元数据。
pub struct SpoolEntry {
    pub representation_id: RepresentationId,
    pub file_path: PathBuf,
    pub size: usize,
}

/// 带 mtime 的 spool 条目元数据，供 TTL 清理使用。
pub struct SpoolEntryMeta {
    pub representation_id: RepresentationId,
    pub file_path: PathBuf,
    pub size: usize,
    pub modified_ms: i64,
}

impl SpoolManager {
    /// 创建磁盘缓存管理器并确保目录存在；同时扫描目录把现有文件登记到
    /// in-memory 账本，用于进程重启恢复。
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

        let initial_state = Self::rebuild_state_from_dir(&spool_dir).with_context(|| {
            format!(
                "Failed to rebuild spool state from dir: {}",
                spool_dir.display()
            )
        })?;

        Ok(Self {
            spool_dir,
            max_bytes,
            state: Mutex::new(initial_state),
        })
    }

    /// 启动时扫一遍 spool 目录，按 mtime 升序（≈ FIFO）登记所有已存在文件。
    ///
    /// 这是唯一允许同步扫盘的入口，发生在 worker 启动之前，没有并发删除
    /// 风险。任何 metadata 失败（broken symlink 等）会被跳过并 warn，不
    /// 阻止启动。
    fn rebuild_state_from_dir(spool_dir: &PathBuf) -> io::Result<SpoolState> {
        let mut collected: Vec<(RepresentationId, usize, std::time::SystemTime)> = Vec::new();
        let read_dir = match std::fs::read_dir(spool_dir) {
            Ok(rd) => rd,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                return Ok(SpoolState::new());
            }
            Err(err) => return Err(err),
        };

        for entry in read_dir {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    tracing::warn!(error = %err, "Skipping unreadable spool dir entry at startup");
                    continue;
                }
            };
            let meta = match entry.metadata() {
                Ok(meta) => meta,
                Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
                Err(err) => {
                    tracing::warn!(error = %err, path = %entry.path().display(),
                        "Skipping spool entry with unreadable metadata at startup");
                    continue;
                }
            };
            if !meta.is_file() {
                continue;
            }
            let file_name = entry.file_name();
            let Some(name) = file_name.to_str() else {
                tracing::warn!("Skipping spool entry with non-utf8 filename at startup");
                continue;
            };
            let modified = meta.modified().unwrap_or(UNIX_EPOCH);
            collected.push((
                RepresentationId::from_str(name),
                meta.len() as usize,
                modified,
            ));
        }

        // 按 mtime asc 排序，让 IndexMap 的 insertion order 反映 FIFO。
        // mtime tie-break 用 id 字典序，保证启动恢复顺序与 list_entries_by_mtime
        // 的排序规则一致。
        collected.sort_by(|a, b| a.2.cmp(&b.2).then_with(|| a.0.cmp(&b.0)));

        let mut state = SpoolState::new();
        for (id, size, _) in collected {
            state.register(id, size);
        }
        Ok(state)
    }

    /// 写入字节到 spool 并返回条目元数据。
    pub async fn write(&self, rep_id: &RepresentationId, bytes: &[u8]) -> Result<SpoolEntry> {
        // 单条 entry 大小检查（durability sanity check，与目录状态无关）。
        if bytes.len() > self.max_bytes {
            return Err(anyhow::anyhow!(
                "Spool entry size {} bytes exceeds max_bytes {}",
                bytes.len(),
                self.max_bytes
            ));
        }

        // 容量驱逐：基于 in-memory 计数判断，必要时 evict 最旧的非自身条目。
        // 关键 invariant：本次 write 的目标 id 不应被 evict 自己，否则会出现
        // "刚 evict 完又准备写回同一个文件" 的语义混乱（重写场景由
        // SpoolState::register 内部以 "刷新到尾部" 的方式处理）。
        let victims = self.plan_eviction(rep_id, bytes.len());
        for victim_id in victims {
            // 文件可能已被后台 worker.spool.delete 删除（race 在此处是无害的：
            // 我们已经通过 in-memory 账本把它扣减了，磁盘 ENOENT 就是预期）。
            let path = self.spool_dir.join(victim_id.to_string());
            match fs::remove_file(&path).await {
                Ok(()) => {}
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => {
                    // 不是 ENOENT 的失败保留为 warn，因为内存账本已扣减；
                    // 磁盘上残留的旧文件最终会被 SpoolJanitor 的 TTL 清理收掉。
                    tracing::warn!(
                        representation_id = %victim_id,
                        error = %err,
                        path = %path.display(),
                        "Failed to evict oldest spool file; in-memory counter already decremented",
                    );
                }
            }
        }

        // 写入字节并设置权限。这两步任何一步失败都直接返回错误，调用方
        // （DurableSpoolQueue / CaptureClipboardUseCase）会把它视为 durability
        // 失败。本次 write 不会污染 in-memory 账本——register 只在两步都成
        // 功后才发生。
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

        // 文件已在磁盘上，登记到内存账本。
        self.state
            .lock()
            .expect("spool state mutex poisoned")
            .register(rep_id.clone(), bytes.len());

        Ok(SpoolEntry {
            representation_id: rep_id.clone(),
            file_path,
            size: bytes.len(),
        })
    }

    /// 计算本次写入需要驱逐的旧条目 id 列表。
    ///
    /// 拆成独立函数是因为 eviction 的"决策"是纯内存操作（持锁），
    /// 但"执行"（`fs::remove_file`）必须在 .await 之前释放锁。
    /// `MutexGuard` 不允许跨 .await 持有。
    fn plan_eviction(
        &self,
        incoming_id: &RepresentationId,
        incoming_size: usize,
    ) -> Vec<RepresentationId> {
        let mut state = self.state.lock().expect("spool state mutex poisoned");

        // 覆写场景：先扣掉同 id 的旧 size，让"是否超限"的判断基于真实剩余空间。
        // 真正的 register 在 write 末尾发生，这里只是预扣。
        let existing_size = state.entries.get(incoming_id).copied().unwrap_or(0);
        let projected_total = state
            .total_bytes
            .saturating_sub(existing_size)
            .saturating_add(incoming_size);

        if projected_total <= self.max_bytes {
            return Vec::new();
        }

        let mut victims = Vec::new();
        let mut running_total = projected_total;
        while running_total > self.max_bytes {
            // 不允许 evict 自己（覆写时 incoming_id 仍在 entries 里）。
            let Some((victim_id, victim_size)) = state.pop_oldest_except(Some(incoming_id)) else {
                // 没有其他条目可 evict 了 —— max_bytes 太小，单靠本次写入就突破上限。
                // 此时仍然让 write 继续（已经过了 single-entry 大小检查），保留
                // existing oversize 现象但至少不会卡死。
                break;
            };
            running_total = running_total.saturating_sub(victim_size);
            victims.push(victim_id);
        }
        victims
    }

    /// 读取 spool 字节，不存在返回 None。
    pub async fn read(&self, rep_id: &RepresentationId) -> Result<Option<Vec<u8>>> {
        let file_path = self.spool_dir.join(rep_id.to_string());
        match fs::read(&file_path).await {
            Ok(bytes) => Ok(Some(bytes)),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err)
                .with_context(|| format!("Failed to read spool file: {}", file_path.display())),
        }
    }

    /// 检查 spool 中是否存在该表示的字节，比 `read()` 轻量。
    pub async fn exists(&self, rep_id: &RepresentationId) -> Result<bool> {
        let file_path = self.spool_dir.join(rep_id.to_string());
        match fs::metadata(&file_path).await {
            Ok(meta) => Ok(meta.is_file()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err)
                .with_context(|| format!("Failed to stat spool file: {}", file_path.display())),
        }
    }

    /// 删除 spool 条目，文件不存在视为成功。
    pub async fn delete(&self, rep_id: &RepresentationId) -> Result<()> {
        let file_path = self.spool_dir.join(rep_id.to_string());
        let remove_result = fs::remove_file(&file_path).await;

        // 内存账本无条件扣减：即使磁盘文件已被并发清理（ENOENT），账本侧
        // 也应当移除，避免下次容量检查多算。
        self.state
            .lock()
            .expect("spool state mutex poisoned")
            .unregister(rep_id);

        match remove_result {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err)
                .with_context(|| format!("Failed to delete spool file: {}", file_path.display())),
        }
    }

    /// spool 的最大字节配置。
    pub fn max_bytes(&self) -> usize {
        self.max_bytes
    }

    /// 当前内存账本累计字节数。仅用于测试与可观测性，不参与外部 API。
    #[doc(hidden)]
    pub fn tracked_total_bytes(&self) -> usize {
        self.state
            .lock()
            .expect("spool state mutex poisoned")
            .total_bytes
    }

    /// 当前内存账本中条目数。仅用于测试。
    #[doc(hidden)]
    pub fn tracked_entry_count(&self) -> usize {
        self.state
            .lock()
            .expect("spool state mutex poisoned")
            .entries
            .len()
    }

    /// 按 mtime 升序列出 spool 中现存文件。
    ///
    /// 仅 `list_expired()` (TTL 清理) 使用。对单个 entry 的 metadata ENOENT
    /// 容忍跳过 —— 目录迭代在并发文件系统里无法保证 dirent → metadata
    /// 原子性，与本文件 `read()` / `exists()` / `delete()` 的 ENOENT 容忍
    /// 保持一致。
    async fn list_entries_by_mtime(&self) -> Result<Vec<SpoolEntryMeta>> {
        let mut entries = Vec::new();
        let mut dir = fs::read_dir(&self.spool_dir).await?;
        loop {
            let entry = match dir.next_entry().await {
                Ok(Some(entry)) => entry,
                Ok(None) => break,
                Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
                Err(err) => return Err(err.into()),
            };
            let meta = match entry.metadata().await {
                Ok(meta) => meta,
                Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
                Err(err) => return Err(err.into()),
            };
            if !meta.is_file() {
                continue;
            }
            let file_name = entry.file_name();
            let Some(name) = file_name.to_str() else {
                tracing::warn!("Skipping spool entry with non-utf8 filename");
                continue;
            };
            let modified = match meta.modified() {
                Ok(t) => t,
                Err(err) => {
                    tracing::warn!(error = %err, "Skipping spool entry with unreadable mtime");
                    continue;
                }
            };
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
        entries.sort_by(|a, b| {
            a.modified_ms
                .cmp(&b.modified_ms)
                .then_with(|| a.representation_id.cmp(&b.representation_id))
        });
        Ok(entries)
    }

    /// 枚举超过 TTL 的缓存条目，供 `SpoolJanitor` 使用。
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn rep_id(s: &str) -> RepresentationId {
        RepresentationId::from(s)
    }

    fn make_spool() -> (SpoolManager, TempDir) {
        let dir = TempDir::new().expect("tempdir");
        let spool = SpoolManager::new(dir.path(), 1024).expect("spool");
        (spool, dir)
    }

    #[tokio::test]
    async fn exists_returns_true_after_write() {
        let (spool, _dir) = make_spool();
        let id = rep_id("rep-exists-1");
        spool.write(&id, b"hello").await.expect("write");

        assert!(spool.exists(&id).await.expect("exists"));
    }

    #[tokio::test]
    async fn exists_returns_false_when_missing() {
        let (spool, _dir) = make_spool();
        let id = rep_id("rep-missing");

        assert!(!spool.exists(&id).await.expect("exists"));
    }

    #[tokio::test]
    async fn exists_returns_false_after_delete() {
        let (spool, _dir) = make_spool();
        let id = rep_id("rep-delete");
        spool.write(&id, b"data").await.expect("write");
        spool.delete(&id).await.expect("delete");

        assert!(!spool.exists(&id).await.expect("exists"));
    }

    #[tokio::test]
    async fn delete_missing_is_ok() {
        let (spool, _dir) = make_spool();
        let id = rep_id("rep-never-existed");

        spool
            .delete(&id)
            .await
            .expect("delete missing should be ok");
    }

    #[tokio::test]
    async fn write_rejects_when_size_exceeds_max_bytes() {
        let dir = TempDir::new().expect("tempdir");
        let spool = SpoolManager::new(dir.path(), 4).expect("spool");
        let id = rep_id("rep-too-big");

        let err = match spool.write(&id, b"oversized").await {
            Err(e) => e,
            Ok(_) => panic!("expected reject"),
        };
        assert!(err.to_string().contains("exceeds max_bytes"));
        assert!(!spool.exists(&id).await.expect("exists"));
    }

    #[tokio::test]
    async fn list_expired_with_zero_ttl_returns_all_written_entries() {
        let (spool, _dir) = make_spool();
        spool.write(&rep_id("a"), b"1").await.expect("write a");
        spool.write(&rep_id("b"), b"2").await.expect("write b");

        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        let expired = spool.list_expired(now_ms, 0).await.expect("list expired");
        assert_eq!(expired.len(), 2);
    }

    #[tokio::test]
    async fn list_expired_with_large_ttl_returns_nothing() {
        let (spool, _dir) = make_spool();
        spool.write(&rep_id("a"), b"1").await.expect("write a");

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        let expired = spool
            .list_expired(now_ms, 1000)
            .await
            .expect("list expired");
        assert!(expired.is_empty());
    }

    // --- 结构性修复新增测试 ---

    #[tokio::test]
    async fn tracked_total_reflects_writes_and_deletes() {
        let (spool, _dir) = make_spool();
        assert_eq!(spool.tracked_total_bytes(), 0);
        assert_eq!(spool.tracked_entry_count(), 0);

        spool.write(&rep_id("a"), b"hello").await.expect("write a");
        spool
            .write(&rep_id("b"), b"world!!")
            .await
            .expect("write b");
        assert_eq!(spool.tracked_total_bytes(), 5 + 7);
        assert_eq!(spool.tracked_entry_count(), 2);

        spool.delete(&rep_id("a")).await.expect("delete a");
        assert_eq!(spool.tracked_total_bytes(), 7);
        assert_eq!(spool.tracked_entry_count(), 1);
    }

    #[tokio::test]
    async fn rewriting_same_id_does_not_double_count() {
        let (spool, _dir) = make_spool();
        spool.write(&rep_id("a"), b"hello").await.expect("write 1");
        spool
            .write(&rep_id("a"), b"world!!")
            .await
            .expect("write 2");
        assert_eq!(spool.tracked_total_bytes(), 7);
        assert_eq!(spool.tracked_entry_count(), 1);
    }

    #[tokio::test]
    async fn delete_missing_id_is_idempotent_in_state() {
        let (spool, _dir) = make_spool();
        spool.delete(&rep_id("ghost")).await.expect("delete ghost");
        assert_eq!(spool.tracked_total_bytes(), 0);
        assert_eq!(spool.tracked_entry_count(), 0);
    }

    #[tokio::test]
    async fn capacity_overflow_evicts_oldest_first() {
        // max_bytes=12, 三个 5 字节条目 => 第三个写入应 evict "a" 才能腾出空间。
        let dir = TempDir::new().expect("tempdir");
        let spool = SpoolManager::new(dir.path(), 12).expect("spool");

        spool.write(&rep_id("a"), b"AAAAA").await.expect("write a");
        spool.write(&rep_id("b"), b"BBBBB").await.expect("write b");
        // total=10
        spool.write(&rep_id("c"), b"CCCCC").await.expect("write c");
        // 写完 c (15) 后 evict a (剩 10)；evict b 会让总量再降到 5 — 不需要。
        // 期望最终：b, c 仍在；a 被驱逐。

        assert!(!spool.exists(&rep_id("a")).await.unwrap());
        assert!(spool.exists(&rep_id("b")).await.unwrap());
        assert!(spool.exists(&rep_id("c")).await.unwrap());
        assert_eq!(spool.tracked_entry_count(), 2);
        assert_eq!(spool.tracked_total_bytes(), 10);
    }

    #[tokio::test]
    async fn capacity_eviction_does_not_evict_incoming_id_on_rewrite() {
        // 覆写场景：max_bytes=8，先写 b=4，再写 a=4（总 8），再覆写 a=6
        // 应当驱逐 b（最旧的非自身条目），保留 a 的新内容。
        let dir = TempDir::new().expect("tempdir");
        let spool = SpoolManager::new(dir.path(), 8).expect("spool");

        spool.write(&rep_id("b"), b"BBBB").await.expect("write b");
        spool.write(&rep_id("a"), b"AAAA").await.expect("write a");
        spool
            .write(&rep_id("a"), b"AAAAAA")
            .await
            .expect("rewrite a");

        assert!(!spool.exists(&rep_id("b")).await.unwrap());
        assert!(spool.exists(&rep_id("a")).await.unwrap());
        let bytes = spool.read(&rep_id("a")).await.unwrap().unwrap();
        assert_eq!(&bytes, b"AAAAAA");
    }

    #[tokio::test]
    async fn rebuild_state_from_existing_dir_at_startup() {
        // 手动在目录里放两个文件，再创建 SpoolManager —— 启动扫描应当
        // 把它们登记到内存账本里。模拟"进程重启后 spool 文件残留"场景。
        let dir = TempDir::new().expect("tempdir");
        std::fs::write(dir.path().join("rep-1"), b"hello").unwrap();
        std::fs::write(dir.path().join("rep-2"), b"world!!").unwrap();

        let spool = SpoolManager::new(dir.path(), 1024).expect("spool");
        assert_eq!(spool.tracked_entry_count(), 2);
        assert_eq!(spool.tracked_total_bytes(), 5 + 7);
        assert!(spool.exists(&rep_id("rep-1")).await.unwrap());
        assert!(spool.exists(&rep_id("rep-2")).await.unwrap());
    }

    #[tokio::test]
    async fn concurrent_write_and_delete_does_not_race_to_enoent() {
        // 这条 race 是结构性 bug 的复现：旧实现里 write 末尾扫目录调
        // metadata，会被并发 delete 删掉的文件触发 ENOENT。新实现完全
        // 走 in-memory 账本，应当不再失败。
        let dir = TempDir::new().expect("tempdir");
        let spool = Arc::new(SpoolManager::new(dir.path(), 16 * 1024 * 1024).expect("spool"));

        // 预先写入一批 "victim" 文件给并发 delete 删。
        for i in 0..32 {
            spool
                .write(&rep_id(&format!("victim-{i}")), &vec![0u8; 4096])
                .await
                .expect("seed victim");
        }

        let spool_writer = spool.clone();
        let writer = tokio::spawn(async move {
            for i in 0..64 {
                spool_writer
                    .write(&rep_id(&format!("new-{i}")), &vec![1u8; 4096])
                    .await
                    .expect("concurrent write should never fail with ENOENT");
            }
        });

        let spool_deleter = spool.clone();
        let deleter = tokio::spawn(async move {
            for i in 0..32 {
                spool_deleter
                    .delete(&rep_id(&format!("victim-{i}")))
                    .await
                    .expect("concurrent delete should be idempotent");
            }
        });

        writer.await.expect("writer task panicked");
        deleter.await.expect("deleter task panicked");

        // 内存账本与磁盘最终一致：account total = entries.len() * 4096。
        assert_eq!(
            spool.tracked_total_bytes(),
            spool.tracked_entry_count() * 4096
        );
    }
}
