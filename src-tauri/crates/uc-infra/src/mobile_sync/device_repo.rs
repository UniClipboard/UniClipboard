//! `InMemoryMobileDeviceRepository` —— [`MobileDeviceRepositoryPort`] 的进
//! 程内实现，v1 最小可工作版本。
//!
//! ## 为什么 in-memory 而不是 sqlite
//!
//! v1 的预期设备数量很小（个位数 iPhone），且重启后让用户重新走"添加
//! iPhone"是可接受的 UX —— 与 SPEC §13 阶段计划一致。落 sqlite 需要写
//! schema migration + repository row mapper + integration test，工作量
//! 远大于功能本身。先用内存版让 daemon / CLI 端到端跑通，
//! `SqliteMobileDeviceRepository` 留给后续 commit 替换（trait 不变，
//! adapter swap 即可）。
//!
//! ## 并发模型
//!
//! `tokio::sync::Mutex<HashMap<MobileDeviceId, MobileDevice>>`。
//!
//! - 所有操作都在异步 lock 下进行，避免 std::sync::Mutex 在 async 路径上
//!   长时间持锁。
//! - 锁粒度：整张表。这是预期内的折衷 —— 设备数小、写极少（注册 / 撤销
//!   是用户级动作），全表锁不会成为瓶颈。
//! - 唯一性约束：device_id 由 HashMap key 天然保证；token_hash 由 save
//!   显式扫描检查，碰撞返回 `MobileDeviceError::TokenHashCollision`。

use std::collections::HashMap;

use async_trait::async_trait;
use tokio::sync::Mutex;

use uc_core::mobile_sync::{MobileDevice, MobileDeviceError, MobileDeviceId, TokenHash};
use uc_core::ports::MobileDeviceRepositoryPort;

#[derive(Default)]
pub struct InMemoryMobileDeviceRepository {
    devices: Mutex<HashMap<MobileDeviceId, MobileDevice>>,
}

impl InMemoryMobileDeviceRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl MobileDeviceRepositoryPort for InMemoryMobileDeviceRepository {
    async fn save(&self, device: &MobileDevice) -> Result<(), MobileDeviceError> {
        let mut guard = self.devices.lock().await;

        // 重复 device_id ⇒ AlreadyExists（adapter 契约 §5）
        if guard.contains_key(&device.device_id) {
            return Err(MobileDeviceError::AlreadyExists(device.device_id.clone()));
        }
        // token_hash 业务唯一约束 —— 显式扫描。
        if guard.values().any(|d| d.token_hash == device.token_hash) {
            return Err(MobileDeviceError::TokenHashCollision);
        }

        guard.insert(device.device_id.clone(), device.clone());
        Ok(())
    }

    async fn find_by_token_hash(
        &self,
        token_hash: &TokenHash,
    ) -> Result<Option<MobileDevice>, MobileDeviceError> {
        let guard = self.devices.lock().await;
        // 设备数预期个位数，O(n) 扫描足够；将来若量级上去再加 hash → id 索引。
        Ok(guard
            .values()
            .find(|d| d.token_hash == *token_hash)
            .cloned())
    }

    async fn find_by_device_id(
        &self,
        device_id: &MobileDeviceId,
    ) -> Result<Option<MobileDevice>, MobileDeviceError> {
        let guard = self.devices.lock().await;
        Ok(guard.get(device_id).cloned())
    }

    async fn list_all(&self) -> Result<Vec<MobileDevice>, MobileDeviceError> {
        let guard = self.devices.lock().await;
        Ok(guard.values().cloned().collect())
    }

    async fn delete(&self, device_id: &MobileDeviceId) -> Result<bool, MobileDeviceError> {
        let mut guard = self.devices.lock().await;
        Ok(guard.remove(device_id).is_some())
    }

    async fn record_activity(
        &self,
        device_id: &MobileDeviceId,
        last_seen_at_ms: i64,
        last_seen_ip: Option<String>,
        reported_name: Option<String>,
        reported_os: Option<String>,
    ) -> Result<(), MobileDeviceError> {
        let mut guard = self.devices.lock().await;
        // 找不到 device 不报错 —— 撤销路径下可能并发：use case 已经撤销但
        // 鉴权链路里的 record_activity 还在路上。adapter 直接静默成功，让
        // use case 决定是否在调用前先检查。
        if let Some(device) = guard.get_mut(device_id) {
            device.last_seen_at_ms = Some(last_seen_at_ms);
            if last_seen_ip.is_some() {
                device.last_seen_ip = last_seen_ip;
            }
            if reported_name.is_some() {
                device.reported_name = reported_name;
            }
            if reported_os.is_some() {
                device.reported_os = reported_os;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use uc_core::mobile_sync::MobileClientType;

    fn device(id: &str, token_hash_byte: u8, label: &str) -> MobileDevice {
        MobileDevice {
            device_id: MobileDeviceId::new(id),
            label: label.into(),
            client_type: MobileClientType::IosShortcut,
            token_hash: TokenHash::new([token_hash_byte; 32]),
            created_at_ms: 1_000,
            last_seen_at_ms: None,
            last_seen_ip: None,
            reported_name: None,
            reported_os: None,
        }
    }

    #[tokio::test]
    async fn save_and_find_by_id() {
        let repo = InMemoryMobileDeviceRepository::new();
        let d = device("did_x", 1, "phone");
        repo.save(&d).await.unwrap();
        let got = repo.find_by_device_id(&d.device_id).await.unwrap().unwrap();
        assert_eq!(got.label, "phone");
    }

    #[tokio::test]
    async fn save_rejects_duplicate_device_id() {
        let repo = InMemoryMobileDeviceRepository::new();
        let d1 = device("did_x", 1, "first");
        let mut d2 = device("did_x", 2, "second"); // 同 id 不同 token_hash
        d2.label = "duplicate id".into();
        repo.save(&d1).await.unwrap();
        let err = repo.save(&d2).await.unwrap_err();
        assert!(matches!(err, MobileDeviceError::AlreadyExists(_)));
    }

    #[tokio::test]
    async fn save_rejects_token_hash_collision() {
        let repo = InMemoryMobileDeviceRepository::new();
        let d1 = device("did_a", 7, "first");
        let d2 = device("did_b", 7, "second"); // 同 hash 不同 id
        repo.save(&d1).await.unwrap();
        let err = repo.save(&d2).await.unwrap_err();
        assert!(matches!(err, MobileDeviceError::TokenHashCollision));
    }

    #[tokio::test]
    async fn find_by_token_hash_returns_device_or_none() {
        let repo = InMemoryMobileDeviceRepository::new();
        let d = device("did_x", 9, "phone");
        repo.save(&d).await.unwrap();

        let hit = repo
            .find_by_token_hash(&TokenHash::new([9; 32]))
            .await
            .unwrap();
        assert!(hit.is_some());

        let miss = repo
            .find_by_token_hash(&TokenHash::new([42; 32]))
            .await
            .unwrap();
        assert!(miss.is_none());
    }

    #[tokio::test]
    async fn list_all_returns_all_devices() {
        let repo = InMemoryMobileDeviceRepository::new();
        repo.save(&device("did_a", 1, "A")).await.unwrap();
        repo.save(&device("did_b", 2, "B")).await.unwrap();
        let all = repo.list_all().await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn delete_returns_true_when_existed_false_otherwise() {
        let repo = InMemoryMobileDeviceRepository::new();
        let d = device("did_x", 1, "phone");
        repo.save(&d).await.unwrap();

        assert!(repo.delete(&d.device_id).await.unwrap());
        assert!(repo
            .find_by_device_id(&d.device_id)
            .await
            .unwrap()
            .is_none());
        assert!(!repo.delete(&d.device_id).await.unwrap());
    }

    #[tokio::test]
    async fn record_activity_updates_fields_when_device_exists() {
        let repo = InMemoryMobileDeviceRepository::new();
        let d = device("did_x", 1, "phone");
        repo.save(&d).await.unwrap();

        repo.record_activity(
            &d.device_id,
            5_000,
            Some("192.168.1.5".into()),
            Some("iPhone".into()),
            Some("iOS 18".into()),
        )
        .await
        .unwrap();

        let got = repo.find_by_device_id(&d.device_id).await.unwrap().unwrap();
        assert_eq!(got.last_seen_at_ms, Some(5_000));
        assert_eq!(got.last_seen_ip.as_deref(), Some("192.168.1.5"));
        assert_eq!(got.reported_name.as_deref(), Some("iPhone"));
        assert_eq!(got.reported_os.as_deref(), Some("iOS 18"));
    }

    #[tokio::test]
    async fn record_activity_is_silent_no_op_when_device_missing() {
        // 与撤销并发场景：record_activity 不应报错。
        let repo = InMemoryMobileDeviceRepository::new();
        repo.record_activity(&MobileDeviceId::new("did_ghost"), 5_000, None, None, None)
            .await
            .unwrap();
    }
}
