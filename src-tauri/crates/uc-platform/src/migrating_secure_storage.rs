//! `MigratingSecureStorage` —— 一次性把指定 key 从 legacy 后端搬到 primary 后端的装饰器。
//!
//! ## 适用场景
//!
//! 当某个 secret 的存储后端发生迁移（例如 0.6.0 把 iroh device identity 从
//! macOS Keychain 搬到 `<app_data>/iroh-identity/` 的文件存储），需要让升级
//! 上来的老用户**自动**沿用旧值，而不是因为新后端为空就生成全新的 secret。
//!
//! ## 行为契约
//!
//! * `get(k)`：先查 `primary`；命中返回。否则若 `k ∈ migration_keys` 则查
//!   `legacy_fallback`：legacy 命中时**先写 primary，再 best-effort 删
//!   legacy**，返回 legacy 值；legacy 也 miss 则返回 `Ok(None)`。`k` 不在
//!   `migration_keys` 时**永远不查** legacy（避免给非迁移路径无谓增加访问）。
//! * `set(k, v)` / `delete(k)`：只走 `primary`。迁移装饰器对外只看见
//!   primary 的状态，永远不会回写或扩散到 legacy。
//!
//! ## 失败语义
//!
//! * `primary.get` 失败：直接上抛——primary 是真相，读不出来意味着系统层有问题，
//!   调用方应该看到。
//! * `legacy.get` 失败：仅 `warn!` log，**降级当作 legacy miss**（返回 `Ok(None)`）。
//!   迁移本就是 best-effort——legacy 不可达不应阻断 fresh-install 上的
//!   primary 生成。典型场景：Linux 上 dbus-secret-service 在 snap 里命中
//!   未 plug 的 `password-manager-service`、headless 容器无 keyring、
//!   gnome-keyring 解锁 prompt 1s 超时等，旧版本会让整个 iroh bind /
//!   daemon 启动死在这一条路径上。
//! * `primary.set` 失败：迁移整体失败，向上抛 `SecureStorageError`，**不动**
//!   legacy。这样下次启动还能再试，避免数据丢失。
//! * `legacy.delete` 失败：仅 `warn!` log，不影响业务结果。重复迁移幂等
//!   ——下次启动 primary 已命中，根本不进入 legacy 分支。
//!
//! ## 跨平台 prompt 影响
//!
//! 在 macOS 上读取已存在但 ACL 不匹配当前 codesign 的 keychain item 会触发
//! 系统授权弹窗。但：
//! 1. 旧 keychain 里**没有**该 key 时（fresh install），平台返回 `NoEntry`
//!    不弹窗，所以新装用户零额外 prompt。
//! 2. 老用户升级时即便 prompt 一次，相比"必须重新走完整 pairing 流程"
//!    （输 passphrase + 找 PIN + 等握手）体验显著更好；而且实际场景下用户
//!    通常已经为同 service 的 KEK item 选过 "Always Allow"，同应用读其他 item
//!    在生产签名稳定的 build 上不再 prompt。
//!
//! Windows / Linux 平台的 secret store 没有 per-item ACL prompt 行为，迁移
//! 完全无感。

use std::sync::Arc;

use tracing::{debug, info, warn};
use uc_core::ports::{SecureStorageError, SecureStoragePort};

/// 把 `legacy_fallback` 中白名单 key 一次性搬到 `primary` 的装饰器。
///
/// 详见 module doc。
pub struct MigratingSecureStorage {
    primary: Arc<dyn SecureStoragePort>,
    legacy_fallback: Arc<dyn SecureStoragePort>,
    migration_keys: Vec<String>,
}

impl MigratingSecureStorage {
    /// 构造装饰器。`migration_keys` 是允许从 `legacy_fallback` 迁移的 key
    /// 白名单——只有列在白名单里的 key 才会触发 legacy 查询。
    pub fn new(
        primary: Arc<dyn SecureStoragePort>,
        legacy_fallback: Arc<dyn SecureStoragePort>,
        migration_keys: Vec<String>,
    ) -> Self {
        Self {
            primary,
            legacy_fallback,
            migration_keys,
        }
    }

    fn is_migratable(&self, key: &str) -> bool {
        self.migration_keys.iter().any(|k| k == key)
    }
}

impl SecureStoragePort for MigratingSecureStorage {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
        if let Some(value) = self.primary.get(key)? {
            return Ok(Some(value));
        }
        if !self.is_migratable(key) {
            return Ok(None);
        }
        // Legacy backend failure must NOT block startup: migration is by
        // design best-effort. Fresh installs on Linux without a working
        // secret-service backend (e.g. snap strict + missing
        // `password-manager-service` plug, headless containers, or a
        // gnome-keyring lock prompt that hangs ~1s before timing out)
        // legitimately have no legacy data — treating an unreachable legacy
        // as "no-entry" lets the caller (re)generate primary instead of
        // hard-failing the whole iroh bind / daemon startup path.
        let legacy_value = match self.legacy_fallback.get(key) {
            Ok(v) => v,
            Err(err) => {
                warn!(
                    key = %key,
                    error = %err,
                    "legacy secure storage unreachable; treating as no-entry — \
                     primary will be (re)generated; migration is best-effort"
                );
                return Ok(None);
            }
        };
        match legacy_value {
            Some(value) => {
                // 先把 legacy 值落地到 primary，再删 legacy。任一步顺序错都会
                // 导致升级路径不幂等：先删后写而中间崩溃 → 数据永久丢失。
                self.primary.set(key, &value)?;
                match self.legacy_fallback.delete(key) {
                    Ok(()) => {
                        info!(
                            key = %key,
                            "migrated secret from legacy secure storage to primary"
                        );
                    }
                    Err(err) => {
                        // 删除失败不影响业务：下次启动 primary 命中后根本不
                        // 走 legacy 分支，残留条目不会引起任何冲突；用户主动
                        // factory_reset 时再统一清理。
                        warn!(
                            key = %key,
                            error = %err,
                            "migrated secret to primary, but legacy delete failed; \
                             residual entry will be ignored on next boot"
                        );
                    }
                }
                Ok(Some(value))
            }
            None => {
                debug!(
                    key = %key,
                    "no legacy secret found; primary remains empty"
                );
                Ok(None)
            }
        }
    }

    fn set(&self, key: &str, value: &[u8]) -> Result<(), SecureStorageError> {
        self.primary.set(key, value)
    }

    fn delete(&self, key: &str) -> Result<(), SecureStorageError> {
        self.primary.delete(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// In-memory `SecureStoragePort` 用于测试。配合 `Mutex<Counters>` 暴露
    /// 调用次数，让测试断言"legacy 在不应被访问时一次都没被调过"。
    #[derive(Default)]
    struct InMemoryStore {
        map: Mutex<HashMap<String, Vec<u8>>>,
        counters: Mutex<Counters>,
        fail_get: Mutex<bool>,
        fail_set: Mutex<bool>,
        fail_delete: Mutex<bool>,
    }

    #[derive(Default, Debug, Clone)]
    struct Counters {
        gets: usize,
        sets: usize,
        deletes: usize,
    }

    impl InMemoryStore {
        fn new() -> Arc<Self> {
            Arc::new(Self::default())
        }

        fn with_entry(key: &str, value: &[u8]) -> Arc<Self> {
            let store = Self::default();
            store
                .map
                .lock()
                .unwrap()
                .insert(key.to_string(), value.to_vec());
            Arc::new(store)
        }

        fn counters(&self) -> Counters {
            self.counters.lock().unwrap().clone()
        }

        fn has(&self, key: &str) -> bool {
            self.map.lock().unwrap().contains_key(key)
        }

        fn get_value(&self, key: &str) -> Option<Vec<u8>> {
            self.map.lock().unwrap().get(key).cloned()
        }

        fn arm_get_failure(&self) {
            *self.fail_get.lock().unwrap() = true;
        }

        fn arm_set_failure(&self) {
            *self.fail_set.lock().unwrap() = true;
        }

        fn arm_delete_failure(&self) {
            *self.fail_delete.lock().unwrap() = true;
        }
    }

    impl SecureStoragePort for InMemoryStore {
        fn get(&self, key: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
            self.counters.lock().unwrap().gets += 1;
            if *self.fail_get.lock().unwrap() {
                return Err(SecureStorageError::PermissionDenied(
                    "test: legacy backend unreachable".into(),
                ));
            }
            Ok(self.map.lock().unwrap().get(key).cloned())
        }

        fn set(&self, key: &str, value: &[u8]) -> Result<(), SecureStorageError> {
            self.counters.lock().unwrap().sets += 1;
            if *self.fail_set.lock().unwrap() {
                return Err(SecureStorageError::Other("test: set failed".into()));
            }
            self.map
                .lock()
                .unwrap()
                .insert(key.to_string(), value.to_vec());
            Ok(())
        }

        fn delete(&self, key: &str) -> Result<(), SecureStorageError> {
            self.counters.lock().unwrap().deletes += 1;
            if *self.fail_delete.lock().unwrap() {
                return Err(SecureStorageError::Other("test: delete failed".into()));
            }
            self.map.lock().unwrap().remove(key);
            Ok(())
        }
    }

    const KEY: &str = "iroh-identity:v1";
    const OTHER_KEY: &str = "kek:default";

    fn build(
        primary: Arc<InMemoryStore>,
        legacy: Arc<InMemoryStore>,
        whitelist: &[&str],
    ) -> MigratingSecureStorage {
        MigratingSecureStorage::new(
            primary,
            legacy,
            whitelist.iter().map(|s| s.to_string()).collect(),
        )
    }

    #[test]
    fn primary_hit_skips_legacy() {
        let primary = InMemoryStore::with_entry(KEY, b"primary-value");
        let legacy = InMemoryStore::with_entry(KEY, b"legacy-value");
        let store = build(Arc::clone(&primary), Arc::clone(&legacy), &[KEY]);

        let got = store.get(KEY).unwrap();

        assert_eq!(got.as_deref(), Some(&b"primary-value"[..]));
        assert_eq!(legacy.counters().gets, 0, "legacy must not be queried");
    }

    #[test]
    fn legacy_hit_migrates_then_deletes_legacy() {
        let primary = InMemoryStore::new();
        let legacy = InMemoryStore::with_entry(KEY, b"legacy-value");
        let store = build(Arc::clone(&primary), Arc::clone(&legacy), &[KEY]);

        let got = store.get(KEY).unwrap();

        assert_eq!(got.as_deref(), Some(&b"legacy-value"[..]));
        assert_eq!(
            primary.get_value(KEY).as_deref(),
            Some(&b"legacy-value"[..]),
            "value must be persisted into primary"
        );
        assert!(
            !legacy.has(KEY),
            "legacy entry must be deleted after migration"
        );
        let legacy_counters = legacy.counters();
        assert_eq!(legacy_counters.gets, 1);
        assert_eq!(legacy_counters.deletes, 1);
    }

    #[test]
    fn second_get_after_migration_does_not_touch_legacy() {
        let primary = InMemoryStore::new();
        let legacy = InMemoryStore::with_entry(KEY, b"legacy-value");
        let store = build(Arc::clone(&primary), Arc::clone(&legacy), &[KEY]);

        let _ = store.get(KEY).unwrap();
        let before = legacy.counters();
        let _ = store.get(KEY).unwrap();
        let after = legacy.counters();

        assert_eq!(
            before.gets, after.gets,
            "legacy must not be re-queried once primary is populated"
        );
    }

    #[test]
    fn both_miss_returns_none_without_writing_primary() {
        let primary = InMemoryStore::new();
        let legacy = InMemoryStore::new();
        let store = build(Arc::clone(&primary), Arc::clone(&legacy), &[KEY]);

        let got = store.get(KEY).unwrap();

        assert!(got.is_none());
        assert_eq!(primary.counters().sets, 0);
        assert!(!primary.has(KEY));
    }

    #[test]
    fn non_whitelisted_key_never_consults_legacy() {
        let primary = InMemoryStore::new();
        let legacy = InMemoryStore::with_entry(OTHER_KEY, b"sensitive-kek");
        let store = build(Arc::clone(&primary), Arc::clone(&legacy), &[KEY]);

        let got = store.get(OTHER_KEY).unwrap();

        assert!(got.is_none());
        assert_eq!(
            legacy.counters().gets,
            0,
            "non-migratable key must not trigger legacy access"
        );
    }

    #[test]
    fn primary_set_failure_keeps_legacy_intact() {
        let primary = InMemoryStore::new();
        primary.arm_set_failure();
        let legacy = InMemoryStore::with_entry(KEY, b"legacy-value");
        let store = build(Arc::clone(&primary), Arc::clone(&legacy), &[KEY]);

        let result = store.get(KEY);

        assert!(result.is_err(), "primary write failure must propagate");
        assert!(
            legacy.has(KEY),
            "legacy entry must remain so next boot can retry"
        );
        assert_eq!(legacy.counters().deletes, 0);
    }

    #[test]
    fn legacy_get_failure_treated_as_miss_keeps_primary_empty() {
        // Migration is best-effort: an unreachable legacy backend (e.g. snap
        // strict missing `password-manager-service` plug, headless
        // container, or a gnome-keyring lock prompt timing out) must not
        // block startup. The whole `IrohNodeBuilder::bind` daemon-startup
        // path on Linux died here for a full second before this guarantee
        // existed; absence of legacy data is a normal fresh-install state
        // and the caller (re)generates primary downstream.
        let primary = InMemoryStore::new();
        let legacy = InMemoryStore::new();
        legacy.arm_get_failure();
        let store = build(Arc::clone(&primary), Arc::clone(&legacy), &[KEY]);

        let got = store
            .get(KEY)
            .expect("legacy get failure must be swallowed");

        assert!(
            got.is_none(),
            "legacy unreachable must look like 'no legacy data' to the caller"
        );
        assert_eq!(
            primary.counters().sets,
            0,
            "no value to migrate, so primary must not be written"
        );
        assert!(
            !primary.has(KEY),
            "primary stays empty so caller can generate fresh material"
        );
        assert_eq!(
            legacy.counters().deletes,
            0,
            "legacy must not be touched (delete) when its get failed"
        );
    }

    #[test]
    fn legacy_delete_failure_still_returns_migrated_value() {
        let primary = InMemoryStore::new();
        let legacy = InMemoryStore::with_entry(KEY, b"legacy-value");
        legacy.arm_delete_failure();
        let store = build(Arc::clone(&primary), Arc::clone(&legacy), &[KEY]);

        let got = store.get(KEY).unwrap();

        assert_eq!(got.as_deref(), Some(&b"legacy-value"[..]));
        assert_eq!(
            primary.get_value(KEY).as_deref(),
            Some(&b"legacy-value"[..]),
            "primary must still be populated even if legacy delete fails"
        );
    }

    #[test]
    fn set_writes_only_to_primary() {
        let primary = InMemoryStore::new();
        let legacy = InMemoryStore::new();
        let store = build(Arc::clone(&primary), Arc::clone(&legacy), &[KEY]);

        store.set(KEY, b"new-value").unwrap();

        assert!(primary.has(KEY));
        assert!(!legacy.has(KEY));
        assert_eq!(legacy.counters().sets, 0);
    }

    #[test]
    fn delete_removes_only_from_primary() {
        let primary = InMemoryStore::with_entry(KEY, b"primary-value");
        let legacy = InMemoryStore::with_entry(KEY, b"legacy-value");
        let store = build(Arc::clone(&primary), Arc::clone(&legacy), &[KEY]);

        store.delete(KEY).unwrap();

        assert!(!primary.has(KEY));
        assert!(
            legacy.has(KEY),
            "delete must not propagate to legacy fallback"
        );
        assert_eq!(legacy.counters().deletes, 0);
    }
}
