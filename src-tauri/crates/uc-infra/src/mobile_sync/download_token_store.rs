//! `InMemoryShortcutDownloadTokenStore` —— [`ShortcutDownloadTokenStorePort`]
//! 的进程内实现。
//!
//! `.shortcut` 下载是"刚登记好 → iPhone Safari 立即领取"的一次性短旁路；
//! TTL 5 分钟（SPEC §5.1.1），过期作废。把它做成 in-memory 而非落盘是有
//! 意为之：
//!
//! 1. 重启 daemon 即作废所有未消费的 token —— 攻击面窗口最短。
//! 2. token 是"装着 .shortcut 字节流 + device 绑定"的临时凭据，落盘反而
//!    会让二维码截屏 + 重启对方的攻击成为可能。
//! 3. 数量上限有限（每个用户每次 register 才生成一份），HashMap O(1) 足够。
//!
//! 实现要点：
//! - `tokio::sync::Mutex<HashMap<…>>`：所有访问一律走异步锁，避免 std
//!   `Mutex::lock()` 在 async 上下文长时间持锁的副作用。
//! - lazy expiration：每次 `consume` 都顺手清理已过期项，避免后台任务
//!   依赖（让 adapter 不需要持有 runtime handle）。
//! - 一次性消费：`consume` 命中即原子删除，不存在并发双消费。

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use rand::rngs::OsRng;
use rand::TryRngCore;
use tokio::sync::Mutex;

use uc_core::mobile_sync::{MobileDeviceId, RegisteredDownloadToken, ShortcutDownloadToken};
use uc_core::ports::{ClockPort, ShortcutDownloadTokenError, ShortcutDownloadTokenStorePort};

/// 内部存储一条 token 记录。`expires_at_ms` 用 [`ClockPort`] 注入而非
/// `std::time::Instant`，方便测试用 `FixedClock` 跑过期场景。
#[derive(Debug)]
struct Entry {
    device_id: MobileDeviceId,
    payload: Vec<u8>,
    expires_at_ms: i64,
}

pub struct InMemoryShortcutDownloadTokenStore {
    entries: Mutex<HashMap<ShortcutDownloadToken, Entry>>,
    clock: Arc<dyn ClockPort>,
}

impl InMemoryShortcutDownloadTokenStore {
    pub fn new(clock: Arc<dyn ClockPort>) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            clock,
        }
    }

    /// 当前已存（含未过期）记录数 —— 给单测看清扫除是否生效。
    #[cfg(test)]
    async fn len(&self) -> usize {
        self.entries.lock().await.len()
    }
}

#[async_trait]
impl ShortcutDownloadTokenStorePort for InMemoryShortcutDownloadTokenStore {
    async fn register(
        &self,
        device_id: MobileDeviceId,
        payload: Vec<u8>,
        ttl_ms: i64,
    ) -> Result<RegisteredDownloadToken, ShortcutDownloadTokenError> {
        // 16 字节 OsRng + hex —— 32 字符 token 字符串。冲突概率可忽略，
        // 但仍在 insert 路径检查避免静默覆盖（让"碰撞"立刻可见）。
        let mut id_bytes = [0u8; 16];
        OsRng.try_fill_bytes(&mut id_bytes).map_err(|err| {
            ShortcutDownloadTokenError::Internal(format!("OsRng fill failed: {err}"))
        })?;
        let token = ShortcutDownloadToken::new(format!("dt_{}", hex::encode(id_bytes)));

        let now_ms = self.clock.now_ms();
        let expires_at_ms = now_ms.saturating_add(ttl_ms);

        let mut guard = self.entries.lock().await;
        if guard.contains_key(&token) {
            // 32-bit hex collision in a single process is astronomically
            // improbable — if it ever fires it almost certainly means the
            // adapter was wired with a deterministic RNG by accident.
            return Err(ShortcutDownloadTokenError::Internal(
                "download token collision (impossible without deterministic RNG)".into(),
            ));
        }
        guard.insert(
            token.clone(),
            Entry {
                device_id,
                payload,
                expires_at_ms,
            },
        );

        Ok(RegisteredDownloadToken {
            token,
            expires_at_ms,
        })
    }

    async fn consume(
        &self,
        token: &ShortcutDownloadToken,
    ) -> Result<Option<(MobileDeviceId, Vec<u8>)>, ShortcutDownloadTokenError> {
        let now_ms = self.clock.now_ms();
        let mut guard = self.entries.lock().await;

        // Lazy GC —— 顺手清掉所有过期的，不让 HashMap 无限膨胀。
        guard.retain(|_, entry| entry.expires_at_ms > now_ms);

        // 命中即移除（一次性）。Miss / 过期 / 已消费 一律返回 Ok(None)，
        // store 不区分（SPEC §4 的 port 契约）。
        if let Some(entry) = guard.remove(token) {
            Ok(Some((entry.device_id, entry.payload)))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Mutex as StdMutex;

    /// 可被单测推动的 clock。
    struct ManualClock(StdMutex<i64>);
    impl ManualClock {
        fn new(initial: i64) -> Arc<Self> {
            Arc::new(Self(StdMutex::new(initial)))
        }
        fn advance(&self, delta_ms: i64) {
            let mut g = self.0.lock().unwrap();
            *g = g.saturating_add(delta_ms);
        }
    }
    impl ClockPort for ManualClock {
        fn now_ms(&self) -> i64 {
            *self.0.lock().unwrap()
        }
    }

    fn build_store() -> (Arc<ManualClock>, InMemoryShortcutDownloadTokenStore) {
        let clock = ManualClock::new(1_000);
        let store = InMemoryShortcutDownloadTokenStore::new(clock.clone());
        (clock, store)
    }

    #[tokio::test]
    async fn register_then_consume_returns_payload() {
        let (_, store) = build_store();
        let registered = store
            .register(MobileDeviceId::new("did_x"), b"abc".to_vec(), 60_000)
            .await
            .unwrap();
        assert!(registered.token.as_str().starts_with("dt_"));
        assert_eq!(registered.expires_at_ms, 1_000 + 60_000);

        let consumed = store.consume(&registered.token).await.unwrap();
        let (id, payload) = consumed.expect("must hit");
        assert_eq!(id.as_str(), "did_x");
        assert_eq!(payload, b"abc");

        // 二次消费同一 token —— None。
        assert!(store.consume(&registered.token).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn consume_after_ttl_returns_none_and_evicts() {
        let (clock, store) = build_store();
        let registered = store
            .register(MobileDeviceId::new("did_x"), b"abc".to_vec(), 60_000)
            .await
            .unwrap();
        assert_eq!(store.len().await, 1);

        // 推到 TTL 之后 1 ms。
        clock.advance(60_000 + 1);
        let consumed = store.consume(&registered.token).await.unwrap();
        assert!(consumed.is_none(), "过期 token 必须返回 None");
        // Lazy GC 把过期项清掉。
        assert_eq!(store.len().await, 0);
    }

    #[tokio::test]
    async fn miss_on_unknown_token_returns_none_without_error() {
        let (_, store) = build_store();
        let unknown = ShortcutDownloadToken::new("dt_never_existed");
        let out = store.consume(&unknown).await.unwrap();
        assert!(out.is_none());
    }

    #[tokio::test]
    async fn payload_round_trips_arbitrary_bytes() {
        let (_, store) = build_store();
        let payload: Vec<u8> = (0..=255u8).collect();
        let registered = store
            .register(MobileDeviceId::new("did_x"), payload.clone(), 60_000)
            .await
            .unwrap();
        let consumed = store.consume(&registered.token).await.unwrap().unwrap();
        assert_eq!(consumed.1, payload);
    }
}
