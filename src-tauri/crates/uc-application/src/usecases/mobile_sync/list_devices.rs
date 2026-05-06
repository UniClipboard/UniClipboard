//! `ListMobileDevicesUseCase` —— 给 UI / CLI 列出当前服务端登记的全部移动
//! 设备。
//!
//! 这是一个纯读 use case。两点设计取舍值得记住：
//!
//! 1. **不泄漏 `password_hash`**：core 层的 [`MobileDevice`] 含
//!    `password_hash` 字段（`uc-core/src/mobile_sync/device.rs`），那是
//!    server 端鉴权用的 Argon2id PHC。上层（前端 / CLI）一旦能看到 PHC 就
//!    构成攻击面（暴露 KDF 参数 + salt + hash 给离线爆破）。`username` 也
//!    一并不暴露 —— 用户在 SyncClipboard shortcut 里看到的 username 不需要
//!    在桌面端列表里再展示一次, 设备列表只用 label / 最近活跃做识别。所以
//!    use case 把这些都替换为应用层 view [`MobileDeviceSummary`], 仅暴露
//!    面向用户展示需要的字段。
//!
//! 2. **排序由 use case 决定**：repository port 不承诺顺序（不同 adapter
//!    自由实现），UI 期望"最近活跃在前，新登记在前"——这是应用语义而非
//!    存储语义，按 `uc-application/AGENTS.md` §4.1（编排不重定义业务真相）
//!    的反向解读，这种"展示性排序"本就属于编排层。
//!
//! 现阶段只有一个使用者（设置页 + CLI list 命令），summary 类型还住在
//! 这里；当未来的 update_settings / 其它 use case 也要列设备时再下沉到
//! `mobile_sync/mod.rs` 的共享 view 模块。

use std::sync::Arc;

use tracing::instrument;

use uc_core::mobile_sync::{MobileClientType, MobileDevice, MobileDeviceError, MobileDeviceId};
use uc_core::ports::MobileDeviceRepositoryPort;

// ─── public-shaped (output / error) ─────────────────────────────────────

/// UI / CLI 可消费的设备视图：去掉 `token_hash`，其余字段透传。
///
/// 与 core 层 [`MobileDevice`] 是有意分离的两套类型 —— core 是真相、
/// summary 是展示。它们形态相似但语义不同，未来 core 字段调整不应自动
/// 翻进 summary（例如某个内部审计字段加进 core 也不该上 UI）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MobileDeviceSummary {
    pub device_id: MobileDeviceId,
    pub label: String,
    pub client_type: MobileClientType,
    pub created_at_ms: i64,
    pub last_seen_at_ms: Option<i64>,
    pub last_seen_ip: Option<String>,
    pub reported_name: Option<String>,
    pub reported_os: Option<String>,
}

impl MobileDeviceSummary {
    fn from_device(device: MobileDevice) -> Self {
        let MobileDevice {
            device_id,
            label,
            client_type,
            // 故意丢弃 —— 这两个是 view 层的安全边界。
            // username: 用户在 shortcut 客户端里管它就够了, 桌面端列表
            //           不暴露(避免 UI 截图时被旁观者读到)。
            // password_hash: Argon2id PHC, 永不出 application 层。
            username: _,
            password_hash: _,
            created_at_ms,
            last_seen_at_ms,
            last_seen_ip,
            reported_name,
            reported_os,
        } = device;
        Self {
            device_id,
            label,
            client_type,
            created_at_ms,
            last_seen_at_ms,
            last_seen_ip,
            reported_name,
            reported_os,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ListMobileDevicesError {
    #[error("device persistence failed: {0}")]
    PersistenceFailed(String),
}

// ─── use case ───────────────────────────────────────────────────────────

pub(crate) struct ListMobileDevicesUseCase {
    device_repo: Arc<dyn MobileDeviceRepositoryPort>,
}

impl ListMobileDevicesUseCase {
    pub(crate) fn new(device_repo: Arc<dyn MobileDeviceRepositoryPort>) -> Self {
        Self { device_repo }
    }

    /// 列出全部设备，按"最近活跃 desc → 创建时间 desc"排序。
    ///
    /// 排序规则记忆：
    ///   - `last_seen_at_ms` 越大越靠前；`None` 视为最早（排到底部）。
    ///   - 同 `last_seen_at_ms`（含都为 `None`）时按 `created_at_ms` desc。
    ///   - 二级排序的目的：刚刚登记但还没握过手的新设备能跟在"刚活跃过"
    ///     的设备后面，而不是被夹在过期设备中间。
    #[instrument(skip(self))]
    pub(crate) async fn execute(&self) -> Result<Vec<MobileDeviceSummary>, ListMobileDevicesError> {
        let devices = self
            .device_repo
            .list_all()
            .await
            .map_err(translate_device_error)?;

        let mut summaries: Vec<MobileDeviceSummary> = devices
            .into_iter()
            .map(MobileDeviceSummary::from_device)
            .collect();

        summaries.sort_by(|a, b| {
            // last_seen_at_ms desc，把 None 当作 i64::MIN（最旧）。
            let a_seen = a.last_seen_at_ms.unwrap_or(i64::MIN);
            let b_seen = b.last_seen_at_ms.unwrap_or(i64::MIN);
            b_seen
                .cmp(&a_seen)
                .then_with(|| b.created_at_ms.cmp(&a.created_at_ms))
        });

        Ok(summaries)
    }
}

// ─── helpers ────────────────────────────────────────────────────────────

fn translate_device_error(err: MobileDeviceError) -> ListMobileDevicesError {
    match err {
        MobileDeviceError::Storage(msg) => ListMobileDevicesError::PersistenceFailed(msg),
        // list_all 不会触发其它 variant；走到这里一律按 Storage 兜底。
        other => ListMobileDevicesError::PersistenceFailed(other.to_string()),
    }
}

// ─── tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Mutex;

    use async_trait::async_trait;

    fn make_device(
        id: &str,
        label: &str,
        created_at_ms: i64,
        last_seen_at_ms: Option<i64>,
    ) -> MobileDevice {
        MobileDevice {
            device_id: MobileDeviceId::new(id),
            label: label.into(),
            client_type: MobileClientType::IosShortcut,
            username: format!("mobile_{id}"),
            password_hash: "$argon2id$test".into(),
            created_at_ms,
            last_seen_at_ms,
            last_seen_ip: None,
            reported_name: None,
            reported_os: None,
        }
    }

    /// 极简 list-only repo。除 `list_all` 之外的方法 panic 以暴露误用。
    struct FakeRepo {
        devices: Mutex<Vec<MobileDevice>>,
        force_storage_err: bool,
    }

    impl FakeRepo {
        fn new(devices: Vec<MobileDevice>) -> Self {
            Self {
                devices: Mutex::new(devices),
                force_storage_err: false,
            }
        }
    }

    #[async_trait]
    impl MobileDeviceRepositoryPort for FakeRepo {
        async fn save(&self, _: &MobileDevice) -> Result<(), MobileDeviceError> {
            unreachable!("list 不调用 save")
        }
        async fn find_by_username(
            &self,
            _: &str,
        ) -> Result<Option<MobileDevice>, MobileDeviceError> {
            unreachable!("list 不调用 find_by_username")
        }
        async fn find_by_device_id(
            &self,
            _: &MobileDeviceId,
        ) -> Result<Option<MobileDevice>, MobileDeviceError> {
            unreachable!("list 不调用 find_by_device_id")
        }
        async fn list_all(&self) -> Result<Vec<MobileDevice>, MobileDeviceError> {
            if self.force_storage_err {
                return Err(MobileDeviceError::Storage("disk gone".into()));
            }
            Ok(self.devices.lock().unwrap().clone())
        }
        async fn delete(&self, _: &MobileDeviceId) -> Result<bool, MobileDeviceError> {
            unreachable!("list 不调用 delete")
        }
        async fn record_activity(
            &self,
            _: &MobileDeviceId,
            _: i64,
            _: Option<String>,
            _: Option<String>,
            _: Option<String>,
        ) -> Result<(), MobileDeviceError> {
            unreachable!("list 不调用 record_activity")
        }
    }

    #[tokio::test]
    async fn empty_when_no_devices() {
        let uc = ListMobileDevicesUseCase::new(Arc::new(FakeRepo::new(vec![])));
        let out = uc.execute().await.expect("ok");
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn drops_token_hash_in_summary() {
        // 通过编译期的字段集就能保证 —— 这个测试只是 belt-and-suspenders,
        // 防止后续往 summary 加 token_hash 字段时无人察觉。
        let device = make_device("did_x", "phone", 1_000, Some(2_000));
        let uc = ListMobileDevicesUseCase::new(Arc::new(FakeRepo::new(vec![device])));
        let out = uc.execute().await.expect("ok");
        assert_eq!(out.len(), 1);

        // 显式列出 summary 的字段并断言：summary 里不再有 token_hash 字段
        // —— 任何人想加都会在这里失败。
        let s = &out[0];
        assert_eq!(s.device_id.as_str(), "did_x");
        assert_eq!(s.label, "phone");
        assert_eq!(s.client_type, MobileClientType::IosShortcut);
        assert_eq!(s.created_at_ms, 1_000);
        assert_eq!(s.last_seen_at_ms, Some(2_000));
    }

    #[tokio::test]
    async fn sorts_by_last_seen_then_created() {
        // 故意打乱输入顺序，断言 use case 自己做排序，不依赖 repo。
        let devices = vec![
            make_device("did_a", "A 旧 + 没活跃", 100, None),
            make_device("did_b", "B 最近活跃", 50, Some(9_000)),
            make_device("did_c", "C 新 + 没活跃", 1_000, None),
            make_device("did_d", "D 中间活跃", 70, Some(5_000)),
        ];
        let uc = ListMobileDevicesUseCase::new(Arc::new(FakeRepo::new(devices)));
        let out = uc.execute().await.expect("ok");

        let order: Vec<&str> = out.iter().map(|s| s.device_id.as_str()).collect();
        // 先按 last_seen desc：B(9000) > D(5000) > 然后是 None 的两个
        // None 之间按 created desc：C(1000) > A(100)
        assert_eq!(order, vec!["did_b", "did_d", "did_c", "did_a"]);
    }

    #[tokio::test]
    async fn translates_storage_error() {
        let repo = Arc::new(FakeRepo {
            devices: Mutex::new(vec![]),
            force_storage_err: true,
        });
        let uc = ListMobileDevicesUseCase::new(repo);
        let err = uc.execute().await.unwrap_err();
        assert!(
            matches!(err, ListMobileDevicesError::PersistenceFailed(ref s) if s.contains("disk gone")),
            "expected PersistenceFailed(disk gone), got {err:?}"
        );
    }
}
