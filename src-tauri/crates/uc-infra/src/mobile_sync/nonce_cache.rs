//! `InMemoryNonceCache` —— [`NoncePort`] 的进程内实现。
//!
//! 用于 LAN HTTP 鉴权链路防重放：每来一个请求把 `X-UC-Nonce` 登记到滑
//! 动窗口；窗口内重复出现即认定重放，应翻 401 nonce_replay。
//!
//! 设计要点：
//!
//! - **进程内 + 进程重启即丢**：和 `InMemoryShortcutDownloadTokenStore`
//!   同样选择，理由也一致——重启 daemon 即作废未消费 nonce 是安全增益
//!   而非负担。集群部署当前不在 v1 范围。
//! - **lazy GC**：每次 `record_if_new` 顺手把过期项清掉，避免后台 task
//!   依赖（adapter 不持有 runtime handle）。窗口典型 60s，期望规模个位数
//!   到百位数。
//! - **硬上限 + CacheFull**：防止攻击者构造大量随机 nonce 把 HashMap
//!   吃爆。超限时拒绝新条目，由 middleware 翻 503 nonce_cache_full。
//! - **`tokio::sync::Mutex`** 而非 `std::sync::Mutex`：异步 critical
//!   section 极短，但保持与项目其他 mobile_sync adapter 同步语义一致。

use std::collections::HashMap;

use async_trait::async_trait;
use tokio::sync::Mutex;

use uc_core::ports::{NonceError, NoncePort};

/// 默认窗口：60 秒。与 SPEC §4.3 的时间戳漂移容忍同值。
pub const DEFAULT_NONCE_TTL_MS: i64 = 60_000;

/// 默认硬上限：单进程 10000 条。窗口 60s 下平均速率不到 167 RPS 才会撑满，
/// 远高于 v1 mobile sync 的预期负载。超限即拒绝。
pub const DEFAULT_NONCE_MAX_ENTRIES: usize = 10_000;

pub struct InMemoryNonceCache {
    entries: Mutex<HashMap<String, i64>>,
    ttl_ms: i64,
    max_entries: usize,
}

impl InMemoryNonceCache {
    pub fn new(ttl_ms: i64, max_entries: usize) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            ttl_ms,
            max_entries,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(DEFAULT_NONCE_TTL_MS, DEFAULT_NONCE_MAX_ENTRIES)
    }

    #[cfg(test)]
    async fn len(&self) -> usize {
        self.entries.lock().await.len()
    }
}

#[async_trait]
impl NoncePort for InMemoryNonceCache {
    async fn record_if_new(&self, nonce: &str, observed_at_ms: i64) -> Result<bool, NonceError> {
        let mut guard = self.entries.lock().await;

        // Lazy GC：扫一遍清掉过期项，给本次插入腾位置。
        let cutoff = observed_at_ms.saturating_sub(self.ttl_ms);
        guard.retain(|_, t| *t > cutoff);

        if guard.contains_key(nonce) {
            return Ok(false);
        }

        if guard.len() >= self.max_entries {
            return Err(NonceError::CacheFull);
        }

        guard.insert(nonce.to_string(), observed_at_ms);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn first_seen_returns_true_and_inserts() {
        let cache = InMemoryNonceCache::with_defaults();
        let ok = cache.record_if_new("n1", 1_000).await.unwrap();
        assert!(ok, "首次见 nonce 应返回 true");
        assert_eq!(cache.len().await, 1);
    }

    #[tokio::test]
    async fn replay_within_window_returns_false() {
        let cache = InMemoryNonceCache::with_defaults();
        assert!(cache.record_if_new("n1", 1_000).await.unwrap());
        let again = cache.record_if_new("n1", 1_500).await.unwrap();
        assert!(!again, "窗口内重复 nonce 必须返回 false");
        assert_eq!(cache.len().await, 1);
    }

    #[tokio::test]
    async fn record_after_ttl_treats_as_fresh() {
        let cache = InMemoryNonceCache::new(60_000, 10);
        assert!(cache.record_if_new("n1", 1_000).await.unwrap());
        // 跨过 TTL（60_000 ms 后 + 1）—— 旧记录应被 GC 掉。
        let ok = cache.record_if_new("n1", 1_000 + 60_000 + 1).await.unwrap();
        assert!(ok, "跨 TTL 后旧 nonce 应不再算重放");
        // 同时旧条目已被清掉，cache 内只剩当前这条。
        assert_eq!(cache.len().await, 1);
    }

    #[tokio::test]
    async fn distinct_nonces_each_get_recorded() {
        let cache = InMemoryNonceCache::with_defaults();
        assert!(cache.record_if_new("a", 1_000).await.unwrap());
        assert!(cache.record_if_new("b", 1_001).await.unwrap());
        assert!(cache.record_if_new("c", 1_002).await.unwrap());
        assert_eq!(cache.len().await, 3);
    }

    #[tokio::test]
    async fn cache_full_returns_error_without_inserting() {
        let cache = InMemoryNonceCache::new(60_000, 2);
        assert!(cache.record_if_new("a", 1_000).await.unwrap());
        assert!(cache.record_if_new("b", 1_001).await.unwrap());
        // 已满，第三条应被拒绝且不写入。
        let err = cache.record_if_new("c", 1_002).await.unwrap_err();
        assert!(matches!(err, NonceError::CacheFull));
        assert_eq!(cache.len().await, 2);
    }

    #[tokio::test]
    async fn cache_full_recovers_after_ttl_evicts() {
        let cache = InMemoryNonceCache::new(60_000, 2);
        assert!(cache.record_if_new("a", 1_000).await.unwrap());
        assert!(cache.record_if_new("b", 1_001).await.unwrap());
        // 跨 TTL 后旧条目清空，新条目可以进。
        let ok = cache.record_if_new("c", 1_000 + 60_000 + 1).await.unwrap();
        assert!(ok);
        // 此时 a / b 都被 GC 掉，仅 c 留下。
        assert_eq!(cache.len().await, 1);
    }
}
