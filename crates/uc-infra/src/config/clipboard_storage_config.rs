use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ClipboardStorageConfig {
    /// 单条 representation inline 的最大字节数
    pub inline_threshold_bytes: i64,
    pub cache_max_entries: usize,
    pub cache_max_bytes: usize,
    pub spool_max_bytes: usize,
    pub spool_ttl_days: u64,
    pub worker_retry_max_attempts: u32,
    pub worker_retry_backoff_ms: u64,
}

impl ClipboardStorageConfig {
    /// v1 默认值（**非常重要：永远保留**）
    pub fn defaults() -> Self {
        Self {
            inline_threshold_bytes: 16 * 1024, // 16 KB
            cache_max_entries: 1000,
            cache_max_bytes: 100 * 1024 * 1024,
            spool_max_bytes: 1_000_000_000,
            spool_ttl_days: 7,
            worker_retry_max_attempts: 5,
            worker_retry_backoff_ms: 250,
        }
    }
}
