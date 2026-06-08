//! 单条 entry 的同步状态视图组装。
//!
//! 为什么需要这个模块:
//! 持久化层只记"投递发生过的事实"(`EntryDeliveryRepositoryPort`),不记"未尝试"
//! 这种状态。视图层要回答的是"这条 entry 对每台可信对端目前的状态如何",这
//! 是一个跨多个仓储的合成动作 —— entry 本身、来源(event)、当前可信对端集合、
//! 已发生的投递事实四者差集合并,才能得出一个完整、不误导的视图。把这些拼接
//! 关在一个 use case 里,facade 上层只看一个动作:`get_entry_delivery_view`。

use std::collections::HashMap;
use std::sync::Arc;

use uc_core::clipboard::{
    DeliveryFailureReason, EntryDeliveryRecord, EntryDeliveryStatus as DomainDeliveryStatus,
};
use uc_core::ids::{DeviceId, EntryId};
use uc_core::mobile_sync::MobileDeviceId;
use uc_core::ports::{
    ClipboardEntryRepositoryPort, ClipboardEventRepositoryPort, DeviceIdentityPort,
    EntryDeliveryRepositoryPort, MobileDeviceRepositoryPort,
};
use uc_core::trusted_peer::TrustedPeerRepositoryPort;
use uc_core::MemberRepositoryPort;

/// 视图模型:某条 entry 的"来源 + 对每个可信对端的同步状态"完整快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryDeliveryView {
    pub entry_id: EntryId,
    pub source: EntrySource,
    pub deliveries: Vec<EntryDeliveryTargetView>,
}

/// entry 的来源描述。`Historical` 用于追踪机制启用前已存在的老 entry,视图
/// 层应据此明确告知用户"无投递信息",而不是把所有 trusted peer 都合成 Pending。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntrySource {
    /// 本机捕获的 entry。
    Local,
    /// 远端推送过来的 entry。`device_name` 取自空间成员目录中的人类可读名;
    /// 不命中(例如已退出空间但仍存有历史 entry)时为 `None`,渲染层 fallback 到 `device_id`。
    Remote {
        device_id: DeviceId,
        device_name: Option<String>,
    },
    /// 新机制启用前已存在的老 entry,没有可信的投递信息可查。
    Historical,
}

/// 单个对端的同步状态视图。`Pending` 不来自数据库,而是"该对端属于可信集合
/// 但尚未在 delivery 表里出现"时由视图层合成。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryDeliveryTargetView {
    pub target_device_id: DeviceId,
    /// 取自空间成员目录中的人类可读名;不命中时为 `None`,渲染层 fallback 到 `target_device_id`。
    pub target_device_name: Option<String>,
    pub status: EntryDeliveryStatusView,
    pub reason_detail: Option<String>,
    /// `Pending` 时为 `None`(未发生过,没有时间可言)。
    pub updated_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryDeliveryStatusView {
    Pending,
    Delivered,
    Duplicate,
    Failed { reason: DeliveryFailureReason },
}

#[derive(Debug, thiserror::Error)]
pub enum GetEntryDeliveryViewError {
    #[error("entry not found: {0}")]
    EntryNotFound(String),
    #[error("storage failure: {0}")]
    Storage(String),
}

/// `mobile_sync:` 前缀——移动端入站在 `apply_incoming.rs` 里
/// 用 `DeviceId::new(format!("mobile_sync:{}", source_device_id))` 生成伪
/// DeviceId。本 use case 需要识别该前缀以从 `MobileDeviceRepositoryPort`
/// 查出设备 label。
const MOBILE_SYNC_DEVICE_PREFIX: &str = "mobile_sync:";

pub(crate) struct GetEntryDeliveryViewUseCase {
    entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
    event_repo: Arc<dyn ClipboardEventRepositoryPort>,
    trusted_peer_repo: Arc<dyn TrustedPeerRepositoryPort>,
    entry_delivery_repo: Arc<dyn EntryDeliveryRepositoryPort>,
    device_identity: Arc<dyn DeviceIdentityPort>,
    member_repo: Arc<dyn MemberRepositoryPort>,
    mobile_device_repo: Arc<dyn MobileDeviceRepositoryPort>,
}

impl GetEntryDeliveryViewUseCase {
    pub(crate) fn new(
        entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
        event_repo: Arc<dyn ClipboardEventRepositoryPort>,
        trusted_peer_repo: Arc<dyn TrustedPeerRepositoryPort>,
        entry_delivery_repo: Arc<dyn EntryDeliveryRepositoryPort>,
        device_identity: Arc<dyn DeviceIdentityPort>,
        member_repo: Arc<dyn MemberRepositoryPort>,
        mobile_device_repo: Arc<dyn MobileDeviceRepositoryPort>,
    ) -> Self {
        Self {
            entry_repo,
            event_repo,
            trusted_peer_repo,
            entry_delivery_repo,
            device_identity,
            member_repo,
            mobile_device_repo,
        }
    }

    pub(crate) async fn execute(
        &self,
        entry_id: &EntryId,
    ) -> Result<EntryDeliveryView, GetEntryDeliveryViewError> {
        // 1. entry 不存在 → 明确报错,不要静默返回 Historical。
        let entry = self
            .entry_repo
            .get_entry(entry_id)
            .await
            .map_err(|e| GetEntryDeliveryViewError::Storage(e.to_string()))?
            .ok_or_else(|| GetEntryDeliveryViewError::EntryNotFound(entry_id.to_string()))?;

        // 2. 历史 entry 直接降级:不合成 Pending,deliveries 留空,
        //    视图层据此渲染"无投递记录"。
        if !entry.delivery_tracked {
            return Ok(EntryDeliveryView {
                entry_id: entry_id.clone(),
                source: EntrySource::Historical,
                deliveries: Vec::new(),
            });
        }

        // 3. 通过 event 反查来源设备,与本机 device_id 比较得到 Local / Remote。
        //    None 表示 event 缺失或反查不可用 → 来源不可信,降级为 Historical
        //    (绝不当作 Local,否则会把远端 entry 误展示为本机产生)。
        let local_device = self.device_identity.current_device_id();
        let source_device = self
            .event_repo
            .get_source_device(&entry.event_id)
            .await
            .map_err(|e| GetEntryDeliveryViewError::Storage(e.to_string()))?;

        let Some(source_device) = source_device else {
            return Ok(EntryDeliveryView {
                entry_id: entry_id.clone(),
                source: EntrySource::Historical,
                deliveries: Vec::new(),
            });
        };

        let is_local = source_device == local_device;

        // 名字索引:`SpaceMember.device_name` 由配对流程在双方建立成员资格时
        // 写入,这里只读、不刷新。用一次 list 取全集再建 HashMap,避免对
        // peer 集合做 N 次 get。member_repo 故障不阻断视图,降级为"无名字"。
        let name_index: HashMap<DeviceId, String> = match self.member_repo.list().await {
            Ok(members) => members
                .into_iter()
                .filter(|m| !m.device_name.trim().is_empty())
                .map(|m| (m.device_id, m.device_name))
                .collect(),
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    entry_id = %entry_id,
                    "delivery view: member_repo.list failed; falling back to id-only names",
                );
                HashMap::new()
            }
        };

        let source = if is_local {
            EntrySource::Local
        } else {
            let device_name = match name_index.get(&source_device).cloned() {
                Some(name) => Some(name),
                None => self.resolve_mobile_device_label(&source_device).await,
            };
            EntrySource::Remote {
                device_id: source_device,
                device_name,
            }
        };

        // 4. 远端 entry:本机不会对它做出站 dispatch,所以 delivery 表对该
        //    entry 应为空。提前返回,避免无意义的 trusted_peer 列举。
        if !is_local {
            return Ok(EntryDeliveryView {
                entry_id: entry_id.clone(),
                source,
                deliveries: Vec::new(),
            });
        }

        // 5. 本机 entry:trusted_peer 全集 LEFT JOIN delivery 表合成视图。
        //    delivery 表中"孤儿"行(target 已不在 trusted_peer 全集)被
        //    自动忽略,这是有意的:用户解除配对后,UI 上不该再显示鬼魂设备。
        let trusted = self
            .trusted_peer_repo
            .list()
            .await
            .map_err(|e| GetEntryDeliveryViewError::Storage(e.to_string()))?;

        let deliveries = self
            .entry_delivery_repo
            .list_by_entry(entry_id)
            .await
            .map_err(|e| GetEntryDeliveryViewError::Storage(e.to_string()))?;

        let mut delivery_index: HashMap<&str, &EntryDeliveryRecord> =
            HashMap::with_capacity(deliveries.len());
        for d in &deliveries {
            delivery_index.insert(d.target_device_id.as_str(), d);
        }

        let mut target_views: Vec<EntryDeliveryTargetView> = Vec::with_capacity(trusted.len());

        // delivery 表里目标已不在 trusted_peer 集合的"孤儿"行被自动忽略
        // (默认不展示;若未来需要"显示已解除配对的设备",由 facade 增加
        // 参数切换,而不是把 trusted_peer 集合外的也展示出来)。
        for peer in trusted {
            let target_id = peer.peer_device_id.clone();
            let target_name = name_index.get(&target_id).cloned();
            match delivery_index.get(target_id.as_str()) {
                Some(rec) => {
                    target_views.push(EntryDeliveryTargetView {
                        target_device_id: target_id,
                        target_device_name: target_name,
                        status: map_status(&rec.status),
                        reason_detail: rec.reason_detail.clone(),
                        updated_at_ms: Some(rec.updated_at_ms),
                    });
                }
                None => {
                    // trusted peer 但没有 delivery 行 → 还没尝试投递。
                    target_views.push(EntryDeliveryTargetView {
                        target_device_id: target_id,
                        target_device_name: target_name,
                        status: EntryDeliveryStatusView::Pending,
                        reason_detail: None,
                        updated_at_ms: None,
                    });
                }
            }
        }

        Ok(EntryDeliveryView {
            entry_id: entry_id.clone(),
            source,
            deliveries: target_views,
        })
    }

    /// 若 `device_id` 带 `mobile_sync:` 前缀,从移动设备仓库查出
    /// `MobileDevice.label` 作为展示名。仓库故障静默降级为 `None`。
    async fn resolve_mobile_device_label(&self, device_id: &DeviceId) -> Option<String> {
        let suffix = device_id.as_str().strip_prefix(MOBILE_SYNC_DEVICE_PREFIX)?;
        let mobile_id = MobileDeviceId::new(suffix);
        match self.mobile_device_repo.find_by_device_id(&mobile_id).await {
            Ok(Some(device)) if !device.label.trim().is_empty() => Some(device.label),
            Ok(_) => None,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    mobile_device_id = %mobile_id,
                    "delivery view: mobile_device_repo lookup failed; falling back to id-only name",
                );
                None
            }
        }
    }
}

fn map_status(status: &DomainDeliveryStatus) -> EntryDeliveryStatusView {
    match status {
        DomainDeliveryStatus::Delivered => EntryDeliveryStatusView::Delivered,
        DomainDeliveryStatus::Duplicate => EntryDeliveryStatusView::Duplicate,
        DomainDeliveryStatus::Failed { reason } => EntryDeliveryStatusView::Failed {
            reason: reason.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use async_trait::async_trait;
    use chrono::Utc;
    use std::sync::Mutex;
    use uc_core::clipboard::ClipboardEntry;
    use uc_core::ids::EventId;
    use uc_core::mobile_sync::{MobileClientType, MobileDevice, MobileDeviceError};
    use uc_core::security::IdentityFingerprint;
    use uc_core::trusted_peer::{TrustedPeer, TrustedPeerError};
    use uc_core::ClipboardSelectionDecision;
    use uc_core::ObservedClipboardRepresentation;
    use uc_core::{MemberSyncPreferences, MembershipError, SpaceMember};

    // ── 测试 doubles ───────────────────────────────────────────────────

    struct FakeEntryRepo {
        entries: Mutex<HashMap<String, ClipboardEntry>>,
    }
    impl FakeEntryRepo {
        fn new() -> Self {
            Self {
                entries: Mutex::new(HashMap::new()),
            }
        }
        fn insert(&self, entry: ClipboardEntry) {
            self.entries
                .lock()
                .unwrap()
                .insert(entry.entry_id.to_string(), entry);
        }
    }
    #[async_trait]
    impl ClipboardEntryRepositoryPort for FakeEntryRepo {
        async fn save_entry_and_selection(
            &self,
            _entry: &ClipboardEntry,
            _selection: &ClipboardSelectionDecision,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        async fn get_entry(&self, entry_id: &EntryId) -> anyhow::Result<Option<ClipboardEntry>> {
            Ok(self
                .entries
                .lock()
                .unwrap()
                .get(&entry_id.to_string())
                .cloned())
        }
        async fn list_entries(
            &self,
            _limit: usize,
            _offset: usize,
        ) -> anyhow::Result<Vec<ClipboardEntry>> {
            Ok(Vec::new())
        }
        async fn delete_entry(&self, _entry_id: &EntryId) -> anyhow::Result<()> {
            Ok(())
        }
    }

    struct FakeEventRepo {
        sources: Mutex<HashMap<String, Option<DeviceId>>>,
    }
    impl FakeEventRepo {
        fn new() -> Self {
            Self {
                sources: Mutex::new(HashMap::new()),
            }
        }
        fn set_source(&self, event_id: &EventId, device: Option<DeviceId>) {
            self.sources
                .lock()
                .unwrap()
                .insert(event_id.to_string(), device);
        }
    }
    #[async_trait]
    impl ClipboardEventRepositoryPort for FakeEventRepo {
        async fn get_representation(
            &self,
            _id: &EventId,
            _representation_id: &str,
        ) -> anyhow::Result<ObservedClipboardRepresentation> {
            anyhow::bail!("unused in delivery view tests")
        }
        async fn get_source_device(&self, event_id: &EventId) -> anyhow::Result<Option<DeviceId>> {
            Ok(self
                .sources
                .lock()
                .unwrap()
                .get(&event_id.to_string())
                .cloned()
                .unwrap_or(None))
        }
    }

    struct FakeTrustedPeerRepo {
        peers: Mutex<Vec<TrustedPeer>>,
    }
    impl FakeTrustedPeerRepo {
        fn new(peers: Vec<DeviceId>) -> Self {
            let local = DeviceId::new("local-device".to_string());
            let fingerprint = IdentityFingerprint::from_raw_string("AAAABBBBCCCCDDDD")
                .expect("test fingerprint must be valid");
            let now = Utc::now();
            let list = peers
                .into_iter()
                .map(|peer| TrustedPeer {
                    local_device_id: local.clone(),
                    peer_device_id: peer,
                    peer_fingerprint: fingerprint.clone(),
                    trusted_at: now,
                })
                .collect();
            Self {
                peers: Mutex::new(list),
            }
        }
    }
    #[async_trait]
    impl TrustedPeerRepositoryPort for FakeTrustedPeerRepo {
        async fn get(
            &self,
            _peer_device_id: &DeviceId,
        ) -> Result<Option<TrustedPeer>, TrustedPeerError> {
            Ok(None)
        }
        async fn list(&self) -> Result<Vec<TrustedPeer>, TrustedPeerError> {
            Ok(self.peers.lock().unwrap().clone())
        }
        async fn save(&self, _trusted_peer: &TrustedPeer) -> Result<(), TrustedPeerError> {
            Ok(())
        }
        async fn remove(&self, _peer_device_id: &DeviceId) -> Result<bool, TrustedPeerError> {
            Ok(false)
        }
    }

    struct FakeDeliveryRepo {
        records: Mutex<Vec<EntryDeliveryRecord>>,
    }
    impl FakeDeliveryRepo {
        fn new(records: Vec<EntryDeliveryRecord>) -> Self {
            Self {
                records: Mutex::new(records),
            }
        }
    }
    #[async_trait]
    impl EntryDeliveryRepositoryPort for FakeDeliveryRepo {
        async fn record_attempt(
            &self,
            _record: &EntryDeliveryRecord,
        ) -> Result<(), uc_core::clipboard::EntryDeliveryError> {
            Ok(())
        }
        async fn list_by_entry(
            &self,
            entry_id: &EntryId,
        ) -> Result<Vec<EntryDeliveryRecord>, uc_core::clipboard::EntryDeliveryError> {
            Ok(self
                .records
                .lock()
                .unwrap()
                .iter()
                .filter(|r| &r.entry_id == entry_id)
                .cloned()
                .collect())
        }
    }

    struct FixedIdentity(DeviceId);
    impl DeviceIdentityPort for FixedIdentity {
        fn current_device_id(&self) -> DeviceId {
            self.0.clone()
        }
    }

    /// 简易 SpaceMember 仓储 fake。`list` 返回构造时给的 (device_id, name)
    /// 列表;`fail_list = true` 时模拟 member_repo 故障,验证视图降级路径。
    struct FakeMemberRepo {
        members: Vec<SpaceMember>,
        fail_list: bool,
    }
    impl FakeMemberRepo {
        fn new(named: Vec<(DeviceId, &str)>) -> Self {
            let fingerprint = IdentityFingerprint::from_raw_string("AAAABBBBCCCCDDDD")
                .expect("test fingerprint must be valid");
            let now = Utc::now();
            let members = named
                .into_iter()
                .map(|(device_id, name)| SpaceMember {
                    device_id,
                    device_name: name.to_string(),
                    identity_fingerprint: fingerprint.clone(),
                    joined_at: now,
                    sync_preferences: MemberSyncPreferences::default(),
                })
                .collect();
            Self {
                members,
                fail_list: false,
            }
        }
        fn failing() -> Self {
            Self {
                members: Vec::new(),
                fail_list: true,
            }
        }
    }
    #[async_trait]
    impl MemberRepositoryPort for FakeMemberRepo {
        async fn get(&self, device_id: &DeviceId) -> Result<Option<SpaceMember>, MembershipError> {
            Ok(self
                .members
                .iter()
                .find(|m| &m.device_id == device_id)
                .cloned())
        }
        async fn list(&self) -> Result<Vec<SpaceMember>, MembershipError> {
            if self.fail_list {
                Err(MembershipError::Repository("simulated".into()))
            } else {
                Ok(self.members.clone())
            }
        }
        async fn save(&self, _member: &SpaceMember) -> Result<(), MembershipError> {
            Ok(())
        }
        async fn remove(&self, _device_id: &DeviceId) -> Result<bool, MembershipError> {
            Ok(false)
        }
    }

    mockall::mock! {
        MobileDeviceRepo {}
        #[async_trait]
        impl MobileDeviceRepositoryPort for MobileDeviceRepo {
            async fn save(&self, device: &MobileDevice) -> Result<(), MobileDeviceError>;
            async fn find_by_username(
                &self,
                username: &str,
            ) -> Result<Option<MobileDevice>, MobileDeviceError>;
            async fn find_by_device_id(
                &self,
                device_id: &MobileDeviceId,
            ) -> Result<Option<MobileDevice>, MobileDeviceError>;
            async fn list_all(&self) -> Result<Vec<MobileDevice>, MobileDeviceError>;
            async fn delete(&self, device_id: &MobileDeviceId) -> Result<bool, MobileDeviceError>;
            async fn record_activity(
                &self,
                device_id: &MobileDeviceId,
                last_seen_at_ms: i64,
                last_seen_ip: Option<String>,
                reported_name: Option<String>,
                reported_os: Option<String>,
            ) -> Result<(), MobileDeviceError>;
            async fn update_password_hash(
                &self,
                device_id: &MobileDeviceId,
                new_password_hash: String,
            ) -> Result<bool, MobileDeviceError>;
        }
    }

    fn make_empty_mobile_device_repo() -> MockMobileDeviceRepo {
        let mut m = MockMobileDeviceRepo::new();
        m.expect_find_by_device_id().returning(|_| Ok(None));
        m
    }

    // ── helpers ────────────────────────────────────────────────────────

    fn local_id() -> DeviceId {
        DeviceId::new("local-device".to_string())
    }
    fn peer(id: &str) -> DeviceId {
        DeviceId::new(id.to_string())
    }
    fn entry_id(id: &str) -> EntryId {
        EntryId::from(id.to_string())
    }
    fn event_id(id: &str) -> EventId {
        EventId::from(id.to_string())
    }

    fn make_entry(id: &str, event: &str, tracked: bool) -> ClipboardEntry {
        ClipboardEntry::new(entry_id(id), event_id(event), 0, None, 0)
            .with_delivery_tracked(tracked)
    }

    fn delivered(entry: &str, target: DeviceId, at: i64) -> EntryDeliveryRecord {
        EntryDeliveryRecord {
            entry_id: entry_id(entry),
            target_device_id: target,
            status: DomainDeliveryStatus::Delivered,
            reason_detail: None,
            updated_at_ms: at,
        }
    }
    fn failed_offline(entry: &str, target: DeviceId, at: i64) -> EntryDeliveryRecord {
        EntryDeliveryRecord {
            entry_id: entry_id(entry),
            target_device_id: target,
            status: DomainDeliveryStatus::Failed {
                reason: DeliveryFailureReason::Offline,
            },
            reason_detail: None,
            updated_at_ms: at,
        }
    }

    fn build_uc(
        entry_repo: Arc<FakeEntryRepo>,
        event_repo: Arc<FakeEventRepo>,
        trusted_peer_repo: Arc<FakeTrustedPeerRepo>,
        delivery_repo: Arc<FakeDeliveryRepo>,
    ) -> GetEntryDeliveryViewUseCase {
        build_uc_with_members(
            entry_repo,
            event_repo,
            trusted_peer_repo,
            delivery_repo,
            Arc::new(FakeMemberRepo::new(vec![])),
        )
    }

    fn build_uc_with_members(
        entry_repo: Arc<FakeEntryRepo>,
        event_repo: Arc<FakeEventRepo>,
        trusted_peer_repo: Arc<FakeTrustedPeerRepo>,
        delivery_repo: Arc<FakeDeliveryRepo>,
        member_repo: Arc<FakeMemberRepo>,
    ) -> GetEntryDeliveryViewUseCase {
        build_uc_full(
            entry_repo,
            event_repo,
            trusted_peer_repo,
            delivery_repo,
            member_repo,
            Arc::new(make_empty_mobile_device_repo()),
        )
    }

    fn build_uc_full(
        entry_repo: Arc<FakeEntryRepo>,
        event_repo: Arc<FakeEventRepo>,
        trusted_peer_repo: Arc<FakeTrustedPeerRepo>,
        delivery_repo: Arc<FakeDeliveryRepo>,
        member_repo: Arc<FakeMemberRepo>,
        mobile_device_repo: Arc<dyn MobileDeviceRepositoryPort>,
    ) -> GetEntryDeliveryViewUseCase {
        GetEntryDeliveryViewUseCase::new(
            entry_repo,
            event_repo,
            trusted_peer_repo,
            delivery_repo,
            Arc::new(FixedIdentity(local_id())),
            member_repo,
            mobile_device_repo,
        )
    }

    // ── 分支 1: entry 不存在 → EntryNotFound ────────────────────────────

    #[tokio::test]
    async fn entry_not_found_returns_error() {
        let uc = build_uc(
            Arc::new(FakeEntryRepo::new()),
            Arc::new(FakeEventRepo::new()),
            Arc::new(FakeTrustedPeerRepo::new(vec![])),
            Arc::new(FakeDeliveryRepo::new(vec![])),
        );
        let err = uc.execute(&entry_id("ghost")).await.unwrap_err();
        assert!(matches!(err, GetEntryDeliveryViewError::EntryNotFound(_)));
    }

    // ── 分支 2: delivery_tracked = false → Historical ──────────────────

    #[tokio::test]
    async fn historical_entry_returns_historical_source() {
        let entries = Arc::new(FakeEntryRepo::new());
        entries.insert(make_entry("e1", "ev1", false));
        let uc = build_uc(
            entries,
            Arc::new(FakeEventRepo::new()),
            Arc::new(FakeTrustedPeerRepo::new(vec![peer("p1")])),
            Arc::new(FakeDeliveryRepo::new(vec![])),
        );
        let view = uc.execute(&entry_id("e1")).await.unwrap();
        assert_eq!(view.source, EntrySource::Historical);
        assert!(view.deliveries.is_empty());
    }

    // ── 分支 3: source_device = None → Historical fallback ─────────────

    #[tokio::test]
    async fn missing_source_device_falls_back_to_historical() {
        let entries = Arc::new(FakeEntryRepo::new());
        entries.insert(make_entry("e1", "ev1", true));
        let events = Arc::new(FakeEventRepo::new());
        // 不调 set_source → get_source_device 返回 Ok(None)
        let uc = build_uc(
            entries,
            events,
            Arc::new(FakeTrustedPeerRepo::new(vec![peer("p1")])),
            Arc::new(FakeDeliveryRepo::new(vec![])),
        );
        let view = uc.execute(&entry_id("e1")).await.unwrap();
        assert_eq!(view.source, EntrySource::Historical);
        assert!(
            view.deliveries.is_empty(),
            "无可信来源时不得合成 Pending 误导用户"
        );
    }

    // ── 分支 4: 远端 entry → Remote, deliveries 空 ─────────────────────

    #[tokio::test]
    async fn remote_entry_returns_remote_source_with_empty_deliveries() {
        let entries = Arc::new(FakeEntryRepo::new());
        entries.insert(make_entry("e1", "ev1", true));
        let events = Arc::new(FakeEventRepo::new());
        events.set_source(&event_id("ev1"), Some(peer("origin-peer")));
        let uc = build_uc(
            entries,
            events,
            Arc::new(FakeTrustedPeerRepo::new(vec![peer("p1"), peer("p2")])),
            Arc::new(FakeDeliveryRepo::new(vec![])),
        );
        let view = uc.execute(&entry_id("e1")).await.unwrap();
        assert_eq!(
            view.source,
            EntrySource::Remote {
                device_id: peer("origin-peer"),
                device_name: None,
            }
        );
        assert!(
            view.deliveries.is_empty(),
            "远端 entry 视图不应列举对其他 peer 的转发"
        );
    }

    // ── 分支 5: 本机 entry · 无 peer → Local, deliveries 空 ─────────────

    #[tokio::test]
    async fn local_entry_with_no_trusted_peers_returns_empty_deliveries() {
        let entries = Arc::new(FakeEntryRepo::new());
        entries.insert(make_entry("e1", "ev1", true));
        let events = Arc::new(FakeEventRepo::new());
        events.set_source(&event_id("ev1"), Some(local_id()));
        let uc = build_uc(
            entries,
            events,
            Arc::new(FakeTrustedPeerRepo::new(vec![])),
            Arc::new(FakeDeliveryRepo::new(vec![])),
        );
        let view = uc.execute(&entry_id("e1")).await.unwrap();
        assert_eq!(view.source, EntrySource::Local);
        assert!(view.deliveries.is_empty());
    }

    // ── 分支 6: 本机 entry · 混合状态 (Delivered / Failed / Pending) ───

    #[tokio::test]
    async fn local_entry_mixes_delivered_failed_and_pending() {
        let entries = Arc::new(FakeEntryRepo::new());
        entries.insert(make_entry("e1", "ev1", true));
        let events = Arc::new(FakeEventRepo::new());
        events.set_source(&event_id("ev1"), Some(local_id()));
        let trusted = Arc::new(FakeTrustedPeerRepo::new(vec![
            peer("p1"),
            peer("p2"),
            peer("p3"),
        ]));
        let delivery = Arc::new(FakeDeliveryRepo::new(vec![
            delivered("e1", peer("p1"), 100),
            failed_offline("e1", peer("p2"), 200),
            // p3 不在 delivery 表 → 应合成 Pending
        ]));
        let uc = build_uc(entries, events, trusted, delivery);
        let view = uc.execute(&entry_id("e1")).await.unwrap();
        assert_eq!(view.source, EntrySource::Local);
        assert_eq!(view.deliveries.len(), 3);

        let by_target: HashMap<String, &EntryDeliveryTargetView> = view
            .deliveries
            .iter()
            .map(|t| (t.target_device_id.to_string(), t))
            .collect();
        assert_eq!(by_target["p1"].status, EntryDeliveryStatusView::Delivered);
        assert_eq!(by_target["p1"].updated_at_ms, Some(100));
        assert!(matches!(
            by_target["p2"].status,
            EntryDeliveryStatusView::Failed {
                reason: DeliveryFailureReason::Offline
            }
        ));
        assert_eq!(by_target["p2"].updated_at_ms, Some(200));
        assert_eq!(by_target["p3"].status, EntryDeliveryStatusView::Pending);
        assert_eq!(by_target["p3"].updated_at_ms, None);
    }

    // ── 分支 7: 孤儿过滤 (delivery 中 target 不在 trusted_peer) ────────

    #[tokio::test]
    async fn orphan_delivery_rows_are_filtered_out() {
        let entries = Arc::new(FakeEntryRepo::new());
        entries.insert(make_entry("e1", "ev1", true));
        let events = Arc::new(FakeEventRepo::new());
        events.set_source(&event_id("ev1"), Some(local_id()));
        let trusted = Arc::new(FakeTrustedPeerRepo::new(vec![peer("p1")]));
        let delivery = Arc::new(FakeDeliveryRepo::new(vec![
            delivered("e1", peer("p1"), 100),
            // p2 已解除信任,但 delivery 表保留了历史行 → 视图层应过滤
            delivered("e1", peer("p2"), 200),
        ]));
        let uc = build_uc(entries, events, trusted, delivery);
        let view = uc.execute(&entry_id("e1")).await.unwrap();
        assert_eq!(view.deliveries.len(), 1, "孤儿 target 应被丢弃");
        assert_eq!(view.deliveries[0].target_device_id, peer("p1"));
    }

    // ── 分支 8: device_name 解析 — 命中/未命中 fallback ─────────────────
    //
    // SpaceMember 表里 p1 有名字、p2 是空字符串(等同缺失)、p3 完全不在
    // 表中。视图应分别填 Some("MacBook")、None、None,前端按 None 做截断
    // device_id fallback,不要硬塞空字符串。

    #[tokio::test]
    async fn resolves_device_names_from_space_member_repo() {
        let entries = Arc::new(FakeEntryRepo::new());
        entries.insert(make_entry("e1", "ev1", true));
        let events = Arc::new(FakeEventRepo::new());
        events.set_source(&event_id("ev1"), Some(local_id()));
        let trusted = Arc::new(FakeTrustedPeerRepo::new(vec![
            peer("p1"),
            peer("p2"),
            peer("p3"),
        ]));
        let delivery = Arc::new(FakeDeliveryRepo::new(vec![delivered(
            "e1",
            peer("p1"),
            100,
        )]));
        let members = Arc::new(FakeMemberRepo::new(vec![
            (peer("p1"), "MacBook Pro"),
            (peer("p2"), "   "), // 空白等同缺失
        ]));

        let uc = build_uc_with_members(entries, events, trusted, delivery, members);
        let view = uc.execute(&entry_id("e1")).await.unwrap();

        let by_target: HashMap<String, &EntryDeliveryTargetView> = view
            .deliveries
            .iter()
            .map(|t| (t.target_device_id.to_string(), t))
            .collect();
        assert_eq!(
            by_target["p1"].target_device_name.as_deref(),
            Some("MacBook Pro")
        );
        assert!(
            by_target["p2"].target_device_name.is_none(),
            "空白名应被视为缺失"
        );
        assert!(
            by_target["p3"].target_device_name.is_none(),
            "member 表中没有 p3 → 视图层不应硬造名字"
        );
    }

    // ── 分支 9: 远端 entry · source.device_name 命中 ────────────────────

    #[tokio::test]
    async fn remote_source_device_name_resolves_from_members() {
        let entries = Arc::new(FakeEntryRepo::new());
        entries.insert(make_entry("e1", "ev1", true));
        let events = Arc::new(FakeEventRepo::new());
        events.set_source(&event_id("ev1"), Some(peer("origin-peer")));
        let members = Arc::new(FakeMemberRepo::new(vec![(
            peer("origin-peer"),
            "Sender Desktop",
        )]));

        let uc = build_uc_with_members(
            entries,
            events,
            Arc::new(FakeTrustedPeerRepo::new(vec![])),
            Arc::new(FakeDeliveryRepo::new(vec![])),
            members,
        );
        let view = uc.execute(&entry_id("e1")).await.unwrap();
        assert_eq!(
            view.source,
            EntrySource::Remote {
                device_id: peer("origin-peer"),
                device_name: Some("Sender Desktop".to_string()),
            }
        );
    }

    // ── 分支 9b: 移动端来源 · device_name 从 mobile_device_repo 解析 ────
    //
    // 移动端入站写的 from_device 是 `mobile_sync:<mobile_device_id>` 伪
    // DeviceId,不在 SpaceMember 表里。视图层应剥离前缀后从
    // MobileDeviceRepositoryPort 查出 label 作为展示名。

    #[tokio::test]
    async fn mobile_source_device_name_resolves_from_mobile_device_repo() {
        let entries = Arc::new(FakeEntryRepo::new());
        entries.insert(make_entry("e1", "ev1", true));
        let events = Arc::new(FakeEventRepo::new());
        events.set_source(&event_id("ev1"), Some(peer("mobile_sync:did_iphone15")));

        let mut mobile_repo = MockMobileDeviceRepo::new();
        mobile_repo
            .expect_find_by_device_id()
            .withf(|id| id.as_str() == "did_iphone15")
            .times(1)
            .returning(|_| {
                Ok(Some(MobileDevice {
                    device_id: MobileDeviceId::new("did_iphone15"),
                    label: "我的 iPhone 15".to_string(),
                    client_type: MobileClientType::IosShortcut,
                    username: "mobile_abc".to_string(),
                    password_hash: String::new(),
                    created_at_ms: 0,
                    last_seen_at_ms: None,
                    last_seen_ip: None,
                    reported_name: None,
                    reported_os: None,
                }))
            });

        let uc = build_uc_full(
            entries,
            events,
            Arc::new(FakeTrustedPeerRepo::new(vec![])),
            Arc::new(FakeDeliveryRepo::new(vec![])),
            Arc::new(FakeMemberRepo::new(vec![])),
            Arc::new(mobile_repo),
        );
        let view = uc.execute(&entry_id("e1")).await.unwrap();
        assert_eq!(
            view.source,
            EntrySource::Remote {
                device_id: peer("mobile_sync:did_iphone15"),
                device_name: Some("我的 iPhone 15".to_string()),
            }
        );
    }

    #[tokio::test]
    async fn mobile_source_without_registered_device_falls_back_to_none() {
        let entries = Arc::new(FakeEntryRepo::new());
        entries.insert(make_entry("e1", "ev1", true));
        let events = Arc::new(FakeEventRepo::new());
        events.set_source(&event_id("ev1"), Some(peer("mobile_sync:did_unknown")));

        let uc = build_uc(
            entries,
            events,
            Arc::new(FakeTrustedPeerRepo::new(vec![])),
            Arc::new(FakeDeliveryRepo::new(vec![])),
        );
        let view = uc.execute(&entry_id("e1")).await.unwrap();
        assert_eq!(
            view.source,
            EntrySource::Remote {
                device_id: peer("mobile_sync:did_unknown"),
                device_name: None,
            },
            "未注册的移动设备应 fallback 到 None,前端截断 device_id 展示"
        );
    }

    // ── 分支 10: member_repo 故障降级 — 不阻断视图,全部 fallback 到 id ──

    #[tokio::test]
    async fn member_repo_failure_falls_back_to_id_only_names() {
        let entries = Arc::new(FakeEntryRepo::new());
        entries.insert(make_entry("e1", "ev1", true));
        let events = Arc::new(FakeEventRepo::new());
        events.set_source(&event_id("ev1"), Some(local_id()));
        let trusted = Arc::new(FakeTrustedPeerRepo::new(vec![peer("p1")]));
        let delivery = Arc::new(FakeDeliveryRepo::new(vec![delivered(
            "e1",
            peer("p1"),
            100,
        )]));
        let members = Arc::new(FakeMemberRepo::failing());

        let uc = build_uc_with_members(entries, events, trusted, delivery, members);
        let view = uc.execute(&entry_id("e1")).await.expect("仍应返回视图");
        assert_eq!(view.deliveries.len(), 1);
        assert!(
            view.deliveries[0].target_device_name.is_none(),
            "member_repo 故障时名字降级为 None,而不是阻断整个视图"
        );
    }
}
