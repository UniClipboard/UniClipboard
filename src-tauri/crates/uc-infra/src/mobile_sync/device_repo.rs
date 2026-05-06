//! `InMemoryMobileDeviceRepository` —— [`MobileDeviceRepositoryPort`] 的进
//! 程内实现(v3 SyncClipboard 兼容版)。
//!
//! 现在 daemon 链路上默认走 [`crate::db::repositories::DieselMobileDeviceRepository`]
//! (跨进程 / 重启稳定),本类型仅作为 use case 单测的轻量替身保留:测试
//! 侧不愿意为了一两条断言去搭 Diesel + tempdir,直接用 in-memory 就够了。
//!
//! ## 并发模型
//!
//! `tokio::sync::Mutex<HashMap<MobileDeviceId, MobileDevice>>`。
//!
//! - 所有操作都在异步 lock 下进行,避免 std::sync::Mutex 在 async 路径上
//!   长时间持锁。
//! - 锁粒度:整张表。设备数小(个位数)、写极少(注册 / 撤销),全表锁不会
//!   成为瓶颈。
//! - 唯一性约束:device_id 由 HashMap key 天然保证;username 由 save 显式
//!   扫描检查,碰撞返回 [`MobileDeviceError::UsernameCollision`]。

use std::collections::HashMap;

use async_trait::async_trait;
use tokio::sync::Mutex;

use uc_core::mobile_sync::{MobileDevice, MobileDeviceError, MobileDeviceId};
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

        if guard.contains_key(&device.device_id) {
            return Err(MobileDeviceError::AlreadyExists(device.device_id.clone()));
        }
        // username 业务唯一约束 —— 显式扫描。
        if guard.values().any(|d| d.username == device.username) {
            return Err(MobileDeviceError::UsernameCollision);
        }

        guard.insert(device.device_id.clone(), device.clone());
        Ok(())
    }

    async fn find_by_username(
        &self,
        username: &str,
    ) -> Result<Option<MobileDevice>, MobileDeviceError> {
        let guard = self.devices.lock().await;
        // 设备数预期个位数,O(n) 扫描足够;将来若量级上去再加 username → id 索引。
        Ok(guard.values().find(|d| d.username == username).cloned())
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
        // 找不到 device 不报错 —— 撤销路径下可能并发:use case 已经撤销但
        // 鉴权链路里的 record_activity 还在路上。adapter 直接静默成功,让
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

    fn device(id: &str, username_suffix: &str, label: &str) -> MobileDevice {
        MobileDevice {
            device_id: MobileDeviceId::new(id),
            label: label.into(),
            client_type: MobileClientType::IosShortcut,
            username: format!("mobile_{username_suffix}"),
            password_hash: format!("$argon2id$test${username_suffix}"),
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
        let d = device("did_x", "0001", "phone");
        repo.save(&d).await.unwrap();
        let got = repo.find_by_device_id(&d.device_id).await.unwrap().unwrap();
        assert_eq!(got.label, "phone");
    }

    #[tokio::test]
    async fn save_rejects_duplicate_device_id() {
        let repo = InMemoryMobileDeviceRepository::new();
        let d1 = device("did_x", "0001", "first");
        let d2 = device("did_x", "0002", "second"); // 同 id 不同 username
        repo.save(&d1).await.unwrap();
        let err = repo.save(&d2).await.unwrap_err();
        assert!(matches!(err, MobileDeviceError::AlreadyExists(_)));
    }

    #[tokio::test]
    async fn save_rejects_username_collision() {
        let repo = InMemoryMobileDeviceRepository::new();
        let d1 = device("did_a", "abcd", "first");
        let d2 = device("did_b", "abcd", "second"); // 同 username 不同 id
        repo.save(&d1).await.unwrap();
        let err = repo.save(&d2).await.unwrap_err();
        assert!(matches!(err, MobileDeviceError::UsernameCollision));
    }

    #[tokio::test]
    async fn find_by_username_returns_device_or_none() {
        let repo = InMemoryMobileDeviceRepository::new();
        let d = device("did_x", "9999", "phone");
        repo.save(&d).await.unwrap();

        let hit = repo.find_by_username("mobile_9999").await.unwrap();
        assert!(hit.is_some());

        let miss = repo.find_by_username("mobile_ghost").await.unwrap();
        assert!(miss.is_none());
    }

    #[tokio::test]
    async fn list_all_returns_all_devices() {
        let repo = InMemoryMobileDeviceRepository::new();
        repo.save(&device("did_a", "aaaa", "A")).await.unwrap();
        repo.save(&device("did_b", "bbbb", "B")).await.unwrap();
        let all = repo.list_all().await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn delete_returns_true_when_existed_false_otherwise() {
        let repo = InMemoryMobileDeviceRepository::new();
        let d = device("did_x", "0001", "phone");
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
        let d = device("did_x", "0001", "phone");
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
        // 与撤销并发场景:record_activity 不应报错。
        let repo = InMemoryMobileDeviceRepository::new();
        repo.record_activity(&MobileDeviceId::new("did_ghost"), 5_000, None, None, None)
            .await
            .unwrap();
    }
}
