//! In-memory representation cache with bounded size.
//! 具备容量上限的内存表示缓存。

use std::collections::{HashMap, VecDeque};

use tokio::sync::Mutex;
use uc_core::ids::RepresentationId;
use uc_core::ports::clipboard::RepresentationCachePort;

/// Bounded cache for representation raw bytes.
/// 表示原始字节的有界缓存。
pub struct RepresentationCache {
    inner: Mutex<Inner>,
}

struct Inner {
    entries: HashMap<RepresentationId, CacheEntry>,
    queue: VecDeque<RepresentationId>,
    max_entries: usize,
    max_bytes: usize,
    current_bytes: usize,
}

struct CacheEntry {
    raw_bytes: Vec<u8>,
    status: CacheEntryStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheEntryStatus {
    Pending,
    Processing,
    Completed,
}

impl RepresentationCache {
    /// Create a new cache with entry and byte limits.
    /// 创建带有条目数与字节数上限的缓存。
    pub fn new(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            inner: Mutex::new(Inner {
                entries: HashMap::new(),
                queue: VecDeque::new(),
                max_entries,
                max_bytes,
                current_bytes: 0,
            }),
        }
    }

    /// Put bytes into cache, evicting oldest entries if needed.
    /// 放入缓存，必要时按最早写入顺序驱逐。
    pub async fn put(&self, rep_id: &RepresentationId, bytes: Vec<u8>) {
        let mut inner = self.inner.lock().await;

        inner.remove_entry(rep_id);
        inner.queue.retain(|id| id != rep_id);

        let entry_size = bytes.len();
        inner.entries.insert(
            rep_id.clone(),
            CacheEntry {
                raw_bytes: bytes,
                status: CacheEntryStatus::Pending,
            },
        );
        inner.queue.push_back(rep_id.clone());
        inner.current_bytes = inner.current_bytes.saturating_add(entry_size);

        inner.evict_if_needed();
    }

    /// Get cached bytes by representation id.
    /// 通过表示 ID 获取缓存字节。
    pub async fn get(&self, rep_id: &RepresentationId) -> Option<Vec<u8>> {
        let inner = self.inner.lock().await;
        inner
            .entries
            .get(rep_id)
            .map(|entry| entry.raw_bytes.clone())
    }

    /// Mark cache entry as completed.
    /// 将缓存条目标记为完成。
    pub async fn mark_completed(&self, rep_id: &RepresentationId) {
        let mut inner = self.inner.lock().await;
        if let Some(entry) = inner.entries.get_mut(rep_id) {
            entry.status = CacheEntryStatus::Completed;
        }
    }

    /// Mark cache entry as processing/spooling.
    /// 将缓存条目标记为处理/写盘中。
    pub async fn mark_spooling(&self, rep_id: &RepresentationId) {
        let mut inner = self.inner.lock().await;
        if let Some(entry) = inner.entries.get_mut(rep_id) {
            entry.status = CacheEntryStatus::Processing;
        }
    }

    /// Revert cache entry to pending state (e.g., after spool write failure).
    /// 将缓存条目恢复为挂起状态（例如，写盘失败后）。
    pub async fn mark_pending(&self, rep_id: &RepresentationId) {
        let mut inner = self.inner.lock().await;
        if let Some(entry) = inner.entries.get_mut(rep_id) {
            entry.status = CacheEntryStatus::Pending;
        }
    }

    /// Remove cache entry explicitly.
    /// 显式移除缓存条目。
    pub async fn remove(&self, rep_id: &RepresentationId) {
        let mut inner = self.inner.lock().await;
        inner.remove_entry(rep_id);
        inner.queue.retain(|id| id != rep_id);
    }
}

#[async_trait::async_trait]
impl RepresentationCachePort for RepresentationCache {
    async fn put(&self, rep_id: &RepresentationId, bytes: Vec<u8>) {
        self.put(rep_id, bytes).await;
    }

    async fn get(&self, rep_id: &RepresentationId) -> Option<Vec<u8>> {
        self.get(rep_id).await
    }

    async fn mark_completed(&self, rep_id: &RepresentationId) {
        self.mark_completed(rep_id).await;
    }

    async fn mark_spooling(&self, rep_id: &RepresentationId) {
        self.mark_spooling(rep_id).await;
    }

    async fn remove(&self, rep_id: &RepresentationId) {
        self.remove(rep_id).await;
    }
}

impl Inner {
    fn remove_entry(&mut self, rep_id: &RepresentationId) {
        if let Some(entry) = self.entries.remove(rep_id) {
            self.current_bytes = self.current_bytes.saturating_sub(entry.raw_bytes.len());
        }
    }

    fn evict_if_needed(&mut self) {
        while self.entries.len() > self.max_entries || self.current_bytes > self.max_bytes {
            if let Some(evicted_id) = self.pop_oldest_by_status(CacheEntryStatus::Completed) {
                self.remove_entry(&evicted_id);
                continue;
            }
            if let Some(evicted_id) = self.queue.pop_front() {
                self.remove_entry(&evicted_id);
            } else {
                break;
            }
        }
    }

    fn pop_oldest_by_status(&mut self, status: CacheEntryStatus) -> Option<RepresentationId> {
        let pos = self.queue.iter().position(|id| {
            self.entries
                .get(id)
                .map(|entry| entry.status == status)
                .unwrap_or(false)
        })?;
        self.queue.remove(pos)
    }
}
