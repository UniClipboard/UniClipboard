//! `ListMobileDevicesUseCase` —— 给 UI / CLI 列出当前服务端登记的全部移动
//! 设备。
//!
//! 这是一个纯读 use case。两点设计取舍值得记住：
//!
//! 1. **不泄漏 `password_hash`**：core 层的 [`MobileDevice`] 含
//!    `password_hash` 字段（`uc-core/src/mobile_sync/device.rs`），那是
//!    server 端鉴权用的 Argon2id PHC。上层（前端 / CLI）一旦能看到 PHC 就
//!    构成攻击面（暴露 KDF 参数 + salt + hash 给离线爆破）。`username` 则
//!    透传给 view —— 用户希望在桌面设备列表上能直接看到该设备的登录账号
//!    用作识别（与 label 互补：label 可改名重复, username 是 server 主键
//!    级稳定的)，与 password_hash 那种"绝对不能出 application 层"的强约
//!    束不同。所以 use case 把 password_hash 剥掉，其余字段（含 username）
//!    一并落进应用层 view [`MobileDeviceSummary`]。
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
use uc_core::ports::ListMobileDevicesPort;

// ─── public-shaped (output / error) ─────────────────────────────────────

/// UI / CLI 可消费的设备视图：去掉 `password_hash`，其余字段透传。
///
/// 与 core 层 [`MobileDevice`] 是有意分离的两套类型 —— core 是真相、
/// summary 是展示。它们形态相似但语义不同，未来 core 字段调整不应自动
/// 翻进 summary（例如某个内部审计字段加进 core 也不该上 UI）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MobileDeviceSummary {
    pub device_id: MobileDeviceId,
    pub label: String,
    pub client_type: MobileClientType,
    /// 设备登录账号（Basic Auth 用户名）。展示给用户作为辅助识别 ——
    /// label 可重命名重复，username 在 server 端是稳定主键。
    pub username: String,
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
            username,
            // 故意丢弃 —— Argon2id PHC，永不出 application 层。
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
            username,
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
    list: Arc<dyn ListMobileDevicesPort>,
}

impl ListMobileDevicesUseCase {
    pub(crate) fn new(list: Arc<dyn ListMobileDevicesPort>) -> Self {
        Self { list }
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
        let devices = self.list.list_all().await.map_err(translate_device_error)?;

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

    // DeviceRepo mock 与 mobile_sync 其它 use case 共用,集中在 test_support。
    use super::super::test_support::MockDeviceRepo;

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

    /// 用 mockall 装出一个"返回固定 list_all 结果"的 repo。其它方法不设
    /// expectation, 一旦被误用 mockall 自动 panic。
    fn repo_returning(devices: Vec<MobileDevice>) -> MockDeviceRepo {
        let mut repo = MockDeviceRepo::new();
        repo.expect_list_all()
            .returning(move || Ok(devices.clone()));
        repo
    }

    #[tokio::test]
    async fn empty_when_no_devices() {
        let uc = ListMobileDevicesUseCase::new(Arc::new(repo_returning(vec![])));
        let out = uc.execute().await.expect("ok");
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn drops_password_hash_but_keeps_username_in_summary() {
        // 通过编译期的字段集就能保证 —— 这个测试只是 belt-and-suspenders,
        // 防止后续不小心把 password_hash 加进 summary。
        // username 现在是显式暴露字段，用作 UI 辅助识别（与 label 互补）。
        let device = make_device("did_x", "phone", 1_000, Some(2_000));
        let uc = ListMobileDevicesUseCase::new(Arc::new(repo_returning(vec![device])));
        let out = uc.execute().await.expect("ok");
        assert_eq!(out.len(), 1);

        let s = &out[0];
        assert_eq!(s.device_id.as_str(), "did_x");
        assert_eq!(s.label, "phone");
        assert_eq!(s.client_type, MobileClientType::IosShortcut);
        // username 现在显式透传 —— make_device 把它构成 `mobile_<id>`。
        assert_eq!(s.username, "mobile_did_x");
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
        let uc = ListMobileDevicesUseCase::new(Arc::new(repo_returning(devices)));
        let out = uc.execute().await.expect("ok");

        let order: Vec<&str> = out.iter().map(|s| s.device_id.as_str()).collect();
        // 先按 last_seen desc：B(9000) > D(5000) > 然后是 None 的两个
        // None 之间按 created desc：C(1000) > A(100)
        assert_eq!(order, vec!["did_b", "did_d", "did_c", "did_a"]);
    }

    #[tokio::test]
    async fn translates_storage_error() {
        let mut repo = MockDeviceRepo::new();
        repo.expect_list_all()
            .returning(|| Err(MobileDeviceError::Storage("disk gone".into())));

        let uc = ListMobileDevicesUseCase::new(Arc::new(repo));
        let err = uc.execute().await.unwrap_err();
        assert!(
            matches!(err, ListMobileDevicesError::PersistenceFailed(ref s) if s.contains("disk gone")),
            "expected PersistenceFailed(disk gone), got {err:?}"
        );
    }
}
