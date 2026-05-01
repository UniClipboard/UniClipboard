//! `MemberRosterFacade` —— 查询路径入口,不做拨号编排。
//!
//! ## 职责范围
//!
//! * `list_with_presence` —— `member_repo.list()` + `presence.current_state()` +
//!   `local_identity.get_current_fingerprint()` 聚合。纯读,不拨号。
//! * `subscribe_presence_events` —— `PresencePort::subscribe` 的 thin 转发。
//!
//! ## 刻意不做
//!
//! * 主动拨号 —— T6 `EnsureReachableAllUseCase` 在 F1 hook 里统一触发;
//!   查询路径不背"触发副作用"的责任。
//! * rename / revoke —— Phase 3 membership 变更能力,Slice 2 不涉及。
//! * last_seen_at 汇总 —— `PresencePort` 当前不追踪时间戳,加了也是永远
//!   `None`,省了先。

use std::sync::Arc;

use tokio::sync::broadcast;
use tracing::instrument;

use uc_core::membership::MemberRepositoryPort;
use uc_core::ports::peer_address::PeerAddressRepositoryPort;
use uc_core::ports::{LocalIdentityPort, PresenceEvent, PresencePort};
use uc_core::trusted_peer::TrustedPeerRepositoryPort;
use uc_core::DeviceId;

use crate::facade::roster::commands::{
    apply_member_sync_preferences_patch, MemberSummary, MemberSyncPreferencesPatch,
    MemberSyncPreferencesView, PeerSnapshotView, RosterEntry,
};
use crate::facade::roster::errors::RosterError;

/// 构造 `MemberRosterFacade` 时需要的 port 束。对齐 `SpaceSetupDeps`
/// 的风格,便于 bootstrap 分步 construct 各 facade。
pub struct MemberRosterDeps {
    pub member_repo: Arc<dyn MemberRepositoryPort>,
    pub peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
    pub trusted_peer_repo: Arc<dyn TrustedPeerRepositoryPort>,
    pub local_identity: Arc<dyn LocalIdentityPort>,
    pub presence: Arc<dyn PresencePort>,
}

/// Roster 查询门面 —— 见模块文档。
pub struct MemberRosterFacade {
    member_repo: Arc<dyn MemberRepositoryPort>,
    peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
    trusted_peer_repo: Arc<dyn TrustedPeerRepositoryPort>,
    local_identity: Arc<dyn LocalIdentityPort>,
    presence: Arc<dyn PresencePort>,
}

impl MemberRosterFacade {
    pub fn new(deps: MemberRosterDeps) -> Self {
        Self {
            member_repo: deps.member_repo,
            peer_addr_repo: deps.peer_addr_repo,
            trusted_peer_repo: deps.trusted_peer_repo,
            local_identity: deps.local_identity,
            presence: deps.presence,
        }
    }

    /// 聚合当前所有成员 + 各自 presence 状态 + 本机标记。
    ///
    /// 读路径保证:`PresencePort::current_state` 按 port 契约是纯缓存读,
    /// 不会拨号 / 不会阻塞 IO。member_repo / local_identity 都是本地存
    /// 储读,整体延迟受 IO 限制但不受网络影响——可以被 UI 高频调用。
    ///
    /// `local_identity.get_current_fingerprint()` 返回 `Ok(None)` 表示本
    /// 机尚未创建身份(pre-A1 / pre-B2),此时所有 entry 都会标 `is_local
    /// == false`——对该窗口期通常没有成员记录所以影响微乎其微,属于
    /// 防御性路径。
    #[instrument(skip_all)]
    pub async fn list_with_presence(&self) -> Result<Vec<RosterEntry>, RosterError> {
        let members = self
            .member_repo
            .list()
            .await
            .map_err(|err| RosterError::MemberRepository(err.to_string()))?;

        let local_fp = self
            .local_identity
            .get_current_fingerprint()
            .await
            .map_err(|err| RosterError::LocalIdentity(err.to_string()))?;

        let mut entries = Vec::with_capacity(members.len());
        for member in members {
            let is_local = local_fp
                .as_ref()
                .is_some_and(|fp| fp == &member.identity_fingerprint);
            let state = self.presence.current_state(&member.device_id).await;
            entries.push(RosterEntry {
                device_id: member.device_id,
                device_name: member.device_name,
                is_local,
                state,
            });
        }
        Ok(entries)
    }

    /// 列出成员摘要。该方法面向 daemon/http 等外部入口,只返回应用层值对象。
    #[instrument(skip_all)]
    pub async fn list_members(&self) -> Result<Vec<MemberSummary>, RosterError> {
        let members = self
            .member_repo
            .list()
            .await
            .map_err(|err| RosterError::MemberRepository(err.to_string()))?;

        Ok(members
            .into_iter()
            .map(|member| MemberSummary {
                device_id: member.device_id.as_str().to_string(),
                device_name: member.device_name,
            })
            .collect())
    }

    /// 列出对外 peer 快照。该方法复用 roster + presence 聚合规则,并隐藏
    /// core `ReachabilityState` / `DeviceId` 等内部模型。
    #[instrument(skip_all)]
    pub async fn list_peer_snapshots(&self) -> Result<Vec<PeerSnapshotView>, RosterError> {
        let entries = self.list_with_presence().await?;
        Ok(entries
            .into_iter()
            .filter(|entry| !entry.is_local)
            .map(|entry| PeerSnapshotView {
                peer_id: entry.device_id.as_str().to_string(),
                device_name: if entry.device_name.is_empty() {
                    None
                } else {
                    Some(entry.device_name)
                },
                addresses: Vec::new(),
                is_paired: true,
                connected: matches!(entry.state, uc_core::ports::ReachabilityState::Online),
                pairing_state: "Trusted".to_string(),
            })
            .collect())
    }

    /// 读取某个成员的同步偏好。调用方传入字符串设备 ID,不接触 core 类型。
    #[instrument(skip_all, fields(device_id = %device_id))]
    pub async fn get_sync_preferences(
        &self,
        device_id: &str,
    ) -> Result<MemberSyncPreferencesView, RosterError> {
        let device_id = DeviceId::new(device_id);
        let member = self
            .member_repo
            .get(&device_id)
            .await
            .map_err(|err| RosterError::MemberRepository(err.to_string()))?
            .ok_or_else(|| RosterError::NotFound(device_id.as_str().to_string()))?;

        Ok(member.sync_preferences.into())
    }

    /// 局部更新某个成员的同步偏好。合并规则收敛在 application 层。
    #[instrument(skip_all, fields(device_id = %device_id))]
    pub async fn update_sync_preferences(
        &self,
        device_id: &str,
        patch: MemberSyncPreferencesPatch,
    ) -> Result<MemberSyncPreferencesView, RosterError> {
        let device_id = DeviceId::new(device_id);
        let existing = self
            .member_repo
            .get(&device_id)
            .await
            .map_err(|err| RosterError::MemberRepository(err.to_string()))?
            .ok_or_else(|| RosterError::NotFound(device_id.as_str().to_string()))?;

        let updated_preferences =
            apply_member_sync_preferences_patch(existing.sync_preferences, patch);
        let updated = uc_core::SpaceMember {
            sync_preferences: updated_preferences,
            ..existing
        };

        self.member_repo
            .save(&updated)
            .await
            .map_err(|err| RosterError::MemberRepository(err.to_string()))?;

        Ok(updated.sync_preferences.into())
    }

    /// 撤销成员。撤销语义由 application 层表达,daemon 不直接调用 use case。
    ///
    /// Facade 级联清理三张表,以维持设计意图的不变量
    /// `trusted_peer ⊆ member` 与 `peer_addr ⊆ member`:
    ///
    /// 1. `member_repo` —— 主体记录,先删;返回 `Ok(false)` 表示成员不存在
    ///    (`NotFound`),后续两步跳过。
    /// 2. `peer_addr_repo` —— 不删会让 `dispatch_entry` /
    ///    `ensure_reachable_all` 仍把已撤销设备当目标(见
    ///    `dispatch_entry.rs` module doc 关于 "avoids iterating ghost entries
    ///    in `member_repo` that never completed pairing" 的不变量)。
    /// 3. `trusted_peer_repo` —— 不删会让同设备**重新配对失败**:
    ///    `TrustPeerUseCase::execute` 检查到行已存在直接返回
    ///    `AlreadyTrusted`(见 `trust_peer.rs` 注释:"先 Distrust 再 Trust" 的
    ///    显式流程)。`distrust_peer.rs` 注释里讲 "UI 的'解除配对'由 Facade
    ///    级联调用",这里就是那个级联。
    ///
    /// 失败处理:`member_repo.remove` 已成功后没法回滚,后续两步任一失败
    /// 都把错误抛给调用方,启动期 `reconcile_*` 会在下次 boot 兜底。两个
    /// remove port 都是 idempotent,缺行不会算 error。
    #[instrument(skip_all, fields(device_id = %device_id))]
    pub async fn revoke_member(&self, device_id: &str) -> Result<(), RosterError> {
        let device_id = DeviceId::new(device_id);
        let removed = self
            .member_repo
            .remove(&device_id)
            .await
            .map_err(|err| RosterError::MemberRepository(err.to_string()))?;
        if !removed {
            return Err(RosterError::NotFound(device_id.as_str().to_string()));
        }

        // peer_addr_repo.remove 是 idempotent 的(port doc),所以即使条目不
        // 存在也会成功;失败仅在底层存储真正出错时发生。
        self.peer_addr_repo
            .remove(&device_id)
            .await
            .map_err(|err| RosterError::PeerAddressRepository(err.to_string()))?;

        // trusted_peer_repo.remove 返回 bool: true=删除一行,false=本来就没有
        // (peer 还没建立 trust 就被踢)。两种都属正常;只有底层存储故障
        // 时才报错。
        self.trusted_peer_repo
            .remove(&device_id)
            .await
            .map_err(|err| RosterError::TrustedPeerRepository(err.to_string()))?;

        Ok(())
    }

    /// `PresencePort::subscribe` 的 thin 转发。
    ///
    /// 每次调用拿一个新 receiver,共享 adapter 的 broadcast 源。标准
    /// `tokio::sync::broadcast` lag 语义:某个 subscriber 落后 capacity 时
    /// 最老的事件会被丢——acceptable,因为最新状态总能通过
    /// `list_with_presence` 或再来一次订阅重建。
    pub fn subscribe_presence_events(&self) -> broadcast::Receiver<PresenceEvent> {
        self.presence.subscribe()
    }
}

#[cfg(test)]
mod tests {
    //! 单元测试围绕 7.1 验收点展开:
    //!
    //! * list_with_presence 聚合正确(成员数 / 顺序 / state 正确映射)
    //! * 本机标记(`is_local`:唯一匹配 fingerprint 的那条)
    //! * subscribe receiver 实时收事件
    //!
    //! 加上错误路径:member_repo / local_identity 故障能翻译成 `RosterError`。
    //!
    //! 并发性不是本 facade 关心点(`list_with_presence` 是串行 await,
    //! 顺序调 `current_state`)—— T6 已经专门覆盖 presence 并发路径。

    use super::*;

    use async_trait::async_trait;
    use chrono::{TimeZone, Utc};
    use std::sync::Mutex as StdMutex;

    use crate::facade::roster::{ContentTypesPatch, MemberSyncPreferencesPatch};
    use uc_core::ids::DeviceId;
    use uc_core::membership::{MemberSyncPreferences, MembershipError, SpaceMember};
    use uc_core::ports::peer_address::{PeerAddressError, PeerAddressRecord};
    use uc_core::ports::{LocalIdentityError, PresenceError, PresenceEvent, ReachabilityState};
    use uc_core::security::IdentityFingerprint;
    use uc_core::trusted_peer::{TrustedPeer, TrustedPeerError};

    // ── mockall: member_repo ────────────────────────────────────────────

    mockall::mock! {
        pub MemberRepo {}

        #[async_trait]
        impl MemberRepositoryPort for MemberRepo {
            async fn get(&self, device_id: &DeviceId) -> Result<Option<SpaceMember>, MembershipError>;
            async fn list(&self) -> Result<Vec<SpaceMember>, MembershipError>;
            async fn save(&self, member: &SpaceMember) -> Result<(), MembershipError>;
            async fn remove(&self, device_id: &DeviceId) -> Result<bool, MembershipError>;
        }
    }

    // ── mockall: peer_addr_repo ─────────────────────────────────────────

    mockall::mock! {
        pub PeerAddrRepo {}

        #[async_trait]
        impl PeerAddressRepositoryPort for PeerAddrRepo {
            async fn get(&self, device: &DeviceId) -> Result<Option<PeerAddressRecord>, PeerAddressError>;
            async fn upsert(&self, record: &PeerAddressRecord) -> Result<(), PeerAddressError>;
            async fn list(&self) -> Result<Vec<PeerAddressRecord>, PeerAddressError>;
            async fn remove(&self, device: &DeviceId) -> Result<(), PeerAddressError>;
        }
    }

    // ── mockall: trusted_peer_repo ──────────────────────────────────────

    mockall::mock! {
        pub TrustedPeerRepo {}

        #[async_trait]
        impl TrustedPeerRepositoryPort for TrustedPeerRepo {
            async fn get(&self, peer_device_id: &DeviceId) -> Result<Option<TrustedPeer>, TrustedPeerError>;
            async fn list(&self) -> Result<Vec<TrustedPeer>, TrustedPeerError>;
            async fn save(&self, trusted_peer: &TrustedPeer) -> Result<(), TrustedPeerError>;
            async fn remove(&self, peer_device_id: &DeviceId) -> Result<bool, TrustedPeerError>;
        }
    }

    // ── mockall: local_identity ─────────────────────────────────────────

    mockall::mock! {
        pub LocalIdentity {}

        #[async_trait]
        impl LocalIdentityPort for LocalIdentity {
            async fn create(&self) -> Result<IdentityFingerprint, LocalIdentityError>;
            async fn ensure(&self) -> Result<IdentityFingerprint, LocalIdentityError>;
            async fn get_current_fingerprint(
                &self,
            ) -> Result<Option<IdentityFingerprint>, LocalIdentityError>;
        }
    }

    // ── hand-written fake: PresencePort ─────────────────────────────────
    //
    // 手写 fake 比 mockall 更适合这个场景:
    // 1. `current_state` 要按不同 device_id 返不同 state —— mockall 的
    //    `.withf(...).returning(...)` 每次得配一条 expectation,啰嗦。
    // 2. `subscribe` 要返 Receiver,Receiver 不 Clone,mockall 里配一次性
    //    返回值要借个 `Mutex<Option<..>>` 比较绕。
    // 3. subscribe 测试要 emit 一个事件给 receiver,需要直接持 Sender,
    //    fake 直接暴露 `emit(event)` 比通过 mockall 间接更清晰。

    struct FakePresence {
        states: StdMutex<Vec<(DeviceId, ReachabilityState)>>,
        tx: broadcast::Sender<PresenceEvent>,
    }

    impl FakePresence {
        fn new(entries: Vec<(DeviceId, ReachabilityState)>) -> Self {
            let (tx, _rx) = broadcast::channel(16);
            Self {
                states: StdMutex::new(entries),
                tx,
            }
        }
        fn emit(&self, event: PresenceEvent) {
            // 忽略无订阅时的 send 失败 —— 测试里只 emit 一次,调用前
            // 先拿了 receiver。
            let _ = self.tx.send(event);
        }
    }

    #[async_trait]
    impl PresencePort for FakePresence {
        async fn ensure_reachable(
            &self,
            _device: &DeviceId,
        ) -> Result<ReachabilityState, PresenceError> {
            unreachable!("MemberRosterFacade 不走 ensure_reachable 路径")
        }
        async fn current_state(&self, device: &DeviceId) -> ReachabilityState {
            self.states
                .lock()
                .unwrap()
                .iter()
                .find(|(d, _)| d == device)
                .map(|(_, s)| *s)
                .unwrap_or(ReachabilityState::Unknown)
        }
        fn subscribe(&self) -> broadcast::Receiver<PresenceEvent> {
            self.tx.subscribe()
        }
    }

    // ── helpers ─────────────────────────────────────────────────────────

    fn fp(seed: &str) -> IdentityFingerprint {
        // IdentityFingerprint 要求固定 16 字符 base32-like 字符串(见其
        // from_raw_string 校验)。本测试用固定 seed 组 + pad。
        let padded = format!("{:A<16}", seed)
            .chars()
            .take(16)
            .collect::<String>();
        IdentityFingerprint::from_raw_string(&padded).expect("测试 seed 要能通过 fingerprint 校验")
    }

    fn member(device: &str, name: &str, fingerprint: IdentityFingerprint) -> SpaceMember {
        SpaceMember {
            device_id: DeviceId::new(device),
            device_name: name.to_string(),
            identity_fingerprint: fingerprint,
            joined_at: Utc.with_ymd_and_hms(2026, 4, 21, 10, 0, 0).unwrap(),
            sync_preferences: MemberSyncPreferences::default(),
        }
    }

    fn build_facade(
        member_repo: MockMemberRepo,
        local_identity: MockLocalIdentity,
        presence: Arc<FakePresence>,
    ) -> MemberRosterFacade {
        // 多数测试不走 revoke_member 路径,默认给两个不设 expectation 的
        // mock —— mockall 严格模式下若意外被调用会 panic,等价于
        // "断言这两个 repo 在该测试中不应被触发"。
        build_facade_with_unpair_repos(
            member_repo,
            MockPeerAddrRepo::new(),
            MockTrustedPeerRepo::new(),
            local_identity,
            presence,
        )
    }

    fn build_facade_with_unpair_repos(
        member_repo: MockMemberRepo,
        peer_addr_repo: MockPeerAddrRepo,
        trusted_peer_repo: MockTrustedPeerRepo,
        local_identity: MockLocalIdentity,
        presence: Arc<FakePresence>,
    ) -> MemberRosterFacade {
        MemberRosterFacade::new(MemberRosterDeps {
            member_repo: Arc::new(member_repo),
            peer_addr_repo: Arc::new(peer_addr_repo),
            trusted_peer_repo: Arc::new(trusted_peer_repo),
            local_identity: Arc::new(local_identity),
            presence,
        })
    }

    // ── tests ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn list_with_presence_empty_roster_returns_empty_vec() {
        let mut repo = MockMemberRepo::new();
        repo.expect_list().times(1).returning(|| Ok(vec![]));
        let mut id = MockLocalIdentity::new();
        // 空 roster 也要读一次 local fingerprint —— 顺序不敏感但要发生
        id.expect_get_current_fingerprint()
            .times(1)
            .returning(|| Ok(Some(fp("LOCAL"))));
        let presence = Arc::new(FakePresence::new(vec![]));

        let facade = build_facade(repo, id, presence);
        let entries = facade.list_with_presence().await.expect("ok");
        assert!(entries.is_empty());
    }

    #[tokio::test]
    async fn list_with_presence_marks_exactly_the_local_member() {
        let local_fp = fp("LOCAL");
        let remote_fp = fp("REMOTE");
        let m_local = member("dev-local", "laptop", local_fp.clone());
        let m_remote = member("dev-remote", "phone", remote_fp.clone());

        let mut repo = MockMemberRepo::new();
        let members = vec![m_local.clone(), m_remote.clone()];
        repo.expect_list()
            .times(1)
            .returning(move || Ok(members.clone()));
        let mut id = MockLocalIdentity::new();
        id.expect_get_current_fingerprint()
            .times(1)
            .returning(move || Ok(Some(local_fp.clone())));
        let presence = Arc::new(FakePresence::new(vec![
            (DeviceId::new("dev-local"), ReachabilityState::Online),
            (DeviceId::new("dev-remote"), ReachabilityState::Offline),
        ]));

        let facade = build_facade(repo, id, presence);
        let entries = facade.list_with_presence().await.expect("ok");
        assert_eq!(entries.len(), 2);

        let local = entries
            .iter()
            .find(|e| e.device_id == DeviceId::new("dev-local"))
            .expect("local entry");
        let remote = entries
            .iter()
            .find(|e| e.device_id == DeviceId::new("dev-remote"))
            .expect("remote entry");

        assert!(local.is_local, "fingerprint 匹配的那条必须 is_local = true");
        assert_eq!(local.device_name, "laptop");
        assert_eq!(local.state, ReachabilityState::Online);

        assert!(
            !remote.is_local,
            "fingerprint 不匹配的那条 is_local = false"
        );
        assert_eq!(remote.device_name, "phone");
        assert_eq!(remote.state, ReachabilityState::Offline);
    }

    #[tokio::test]
    async fn list_with_presence_without_local_identity_marks_all_false() {
        // pre-A1 / pre-B2 防御路径:local_identity 返回 Ok(None),仍能
        // 正常返回 roster,所有 entry is_local = false。
        let m = member("dev-x", "box", fp("SOMEFP"));
        let mut repo = MockMemberRepo::new();
        let members = vec![m];
        repo.expect_list()
            .times(1)
            .returning(move || Ok(members.clone()));
        let mut id = MockLocalIdentity::new();
        id.expect_get_current_fingerprint()
            .times(1)
            .returning(|| Ok(None));
        let presence = Arc::new(FakePresence::new(vec![(
            DeviceId::new("dev-x"),
            ReachabilityState::Unknown,
        )]));

        let facade = build_facade(repo, id, presence);
        let entries = facade.list_with_presence().await.expect("ok");
        assert_eq!(entries.len(), 1);
        assert!(!entries[0].is_local);
        assert_eq!(entries[0].state, ReachabilityState::Unknown);
    }

    #[tokio::test]
    async fn list_with_presence_state_defaults_to_unknown_when_presence_has_no_entry() {
        // PresencePort 契约:从未 probed 的 device 返 Unknown。本测试确保
        // facade 直接把这个值透传进 RosterEntry,不做二次翻译。
        let m = member("dev-fresh", "new-one", fp("FRESHFP"));
        let mut repo = MockMemberRepo::new();
        let members = vec![m];
        repo.expect_list()
            .times(1)
            .returning(move || Ok(members.clone()));
        let mut id = MockLocalIdentity::new();
        id.expect_get_current_fingerprint()
            .times(1)
            .returning(|| Ok(Some(fp("LOCAL"))));
        let presence = Arc::new(FakePresence::new(vec![])); // 无缓存

        let facade = build_facade(repo, id, presence);
        let entries = facade.list_with_presence().await.expect("ok");
        assert_eq!(entries[0].state, ReachabilityState::Unknown);
    }

    #[tokio::test]
    async fn list_with_presence_surfaces_member_repo_failure() {
        let mut repo = MockMemberRepo::new();
        repo.expect_list()
            .times(1)
            .returning(|| Err(MembershipError::Repository("sqlite down".into())));
        let mut id = MockLocalIdentity::new();
        id.expect_get_current_fingerprint().times(0);
        let presence = Arc::new(FakePresence::new(vec![]));

        let facade = build_facade(repo, id, presence);
        let err = facade.list_with_presence().await.unwrap_err();
        match err {
            RosterError::MemberRepository(msg) => {
                assert!(msg.contains("sqlite down"), "msg = {msg}");
            }
            other => panic!("expected MemberRepository variant, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn list_with_presence_surfaces_local_identity_failure() {
        let mut repo = MockMemberRepo::new();
        repo.expect_list().times(1).returning(|| Ok(vec![]));
        let mut id = MockLocalIdentity::new();
        id.expect_get_current_fingerprint()
            .times(1)
            .returning(|| Err(LocalIdentityError::Storage("keychain locked".into())));
        let presence = Arc::new(FakePresence::new(vec![]));

        let facade = build_facade(repo, id, presence);
        let err = facade.list_with_presence().await.unwrap_err();
        match err {
            RosterError::LocalIdentity(msg) => {
                assert!(msg.contains("keychain locked"), "msg = {msg}");
            }
            other => panic!("expected LocalIdentity variant, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn subscribe_presence_events_delivers_events_through_facade() {
        // T7 验收点:subscribe receiver 实时收事件。拿 receiver → 通过
        // fake 的 emit 发事件 → 确认 receiver 能收到。
        let repo = MockMemberRepo::new(); // 本测试不查 list
        let id = MockLocalIdentity::new(); // 也不查 identity
        let presence = Arc::new(FakePresence::new(vec![]));

        let facade = MemberRosterFacade::new(MemberRosterDeps {
            member_repo: Arc::new(repo),
            peer_addr_repo: Arc::new(MockPeerAddrRepo::new()),
            trusted_peer_repo: Arc::new(MockTrustedPeerRepo::new()),
            local_identity: Arc::new(id),
            presence: Arc::clone(&presence) as Arc<dyn PresencePort>,
        });

        let mut rx = facade.subscribe_presence_events();
        let expected = PresenceEvent {
            device_id: DeviceId::new("dev-x"),
            state: ReachabilityState::Online,
            at: Utc.with_ymd_and_hms(2026, 4, 21, 12, 0, 0).unwrap(),
        };
        presence.emit(expected.clone());

        let got = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("事件在超时内到达")
            .expect("broadcast 成功 recv");
        assert_eq!(got.device_id, expected.device_id);
        assert_eq!(got.state, expected.state);
    }

    #[tokio::test]
    async fn subscribe_presence_events_hands_out_independent_receivers() {
        // broadcast 语义:每次 subscribe() 拿到独立 receiver,一次 emit 两
        // 个 receiver 都能各自收到。
        let repo = MockMemberRepo::new();
        let id = MockLocalIdentity::new();
        let presence = Arc::new(FakePresence::new(vec![]));
        let facade = MemberRosterFacade::new(MemberRosterDeps {
            member_repo: Arc::new(repo),
            peer_addr_repo: Arc::new(MockPeerAddrRepo::new()),
            trusted_peer_repo: Arc::new(MockTrustedPeerRepo::new()),
            local_identity: Arc::new(id),
            presence: Arc::clone(&presence) as Arc<dyn PresencePort>,
        });

        let mut rx1 = facade.subscribe_presence_events();
        let mut rx2 = facade.subscribe_presence_events();
        presence.emit(PresenceEvent {
            device_id: DeviceId::new("d"),
            state: ReachabilityState::Online,
            at: Utc.with_ymd_and_hms(2026, 4, 21, 12, 0, 0).unwrap(),
        });

        let got1 = tokio::time::timeout(std::time::Duration::from_secs(1), rx1.recv())
            .await
            .unwrap()
            .unwrap();
        let got2 = tokio::time::timeout(std::time::Duration::from_secs(1), rx2.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got1.state, ReachabilityState::Online);
        assert_eq!(got2.state, ReachabilityState::Online);
    }

    #[tokio::test]
    async fn get_sync_preferences_accepts_application_device_id() {
        let mut repo = MockMemberRepo::new();
        let member = member("dev-1", "phone", fp("REMOTE"));
        let expected = member.sync_preferences.clone();
        repo.expect_get()
            .times(1)
            .returning(move |_| Ok(Some(member.clone())));
        let id = MockLocalIdentity::new();
        let presence = Arc::new(FakePresence::new(vec![]));
        let facade = build_facade(repo, id, presence);

        let got = facade.get_sync_preferences("dev-1").await.expect("ok");

        assert_eq!(got.send_enabled, expected.send_enabled);
        assert_eq!(got.receive_enabled, expected.receive_enabled);
        assert_eq!(
            got.send_content_types.text,
            expected.send_content_types.text
        );
    }

    #[tokio::test]
    async fn update_sync_preferences_patch_preserves_unmentioned_fields() {
        let mut repo = MockMemberRepo::new();
        let mut existing = member("dev-1", "phone", fp("REMOTE"));
        existing.sync_preferences.send_enabled = true;
        existing.sync_preferences.receive_enabled = true;
        existing.sync_preferences.send_content_types.text = false;
        existing.sync_preferences.send_content_types.image = true;
        let existing_for_get = existing.clone();

        repo.expect_get()
            .times(1)
            .returning(move |_| Ok(Some(existing_for_get.clone())));
        repo.expect_save()
            .times(1)
            .withf(|member| {
                member.device_id == DeviceId::new("dev-1")
                    && !member.sync_preferences.send_enabled
                    && member.sync_preferences.receive_enabled
                    && member.sync_preferences.send_content_types.text
                    && member.sync_preferences.send_content_types.image
            })
            .returning(|_| Ok(()));
        let id = MockLocalIdentity::new();
        let presence = Arc::new(FakePresence::new(vec![]));
        let facade = build_facade(repo, id, presence);

        let updated = facade
            .update_sync_preferences(
                "dev-1",
                MemberSyncPreferencesPatch {
                    send_enabled: Some(false),
                    receive_enabled: None,
                    send_content_types: Some(ContentTypesPatch {
                        text: Some(true),
                        image: None,
                        link: None,
                        file: None,
                        code_snippet: None,
                        rich_text: None,
                    }),
                    receive_content_types: None,
                },
            )
            .await
            .expect("ok");

        assert!(!updated.send_enabled);
        assert!(updated.receive_enabled);
        assert!(updated.send_content_types.text);
        assert!(updated.send_content_types.image);
    }

    #[tokio::test]
    async fn revoke_member_clears_all_three_repos() {
        // 修复点验收:revoke_member 必须级联清三张表,否则:
        //   - peer_addr 残留 → dispatch / presence 仍把已撤销设备当目标
        //   - trusted_peer 残留 → 同设备重新配对会被 AlreadyTrusted 阻塞
        let mut repo = MockMemberRepo::new();
        repo.expect_remove()
            .times(1)
            .withf(|device_id| device_id == &DeviceId::new("dev-1"))
            .returning(|_| Ok(true));
        let mut peer_addr = MockPeerAddrRepo::new();
        peer_addr
            .expect_remove()
            .times(1)
            .withf(|device_id| device_id == &DeviceId::new("dev-1"))
            .returning(|_| Ok(()));
        let mut trusted = MockTrustedPeerRepo::new();
        trusted
            .expect_remove()
            .times(1)
            .withf(|device_id| device_id == &DeviceId::new("dev-1"))
            .returning(|_| Ok(true));
        let id = MockLocalIdentity::new();
        let presence = Arc::new(FakePresence::new(vec![]));
        let facade = build_facade_with_unpair_repos(repo, peer_addr, trusted, id, presence);

        facade.revoke_member("dev-1").await.expect("ok");
    }

    #[tokio::test]
    async fn revoke_member_tolerates_missing_trusted_peer_row() {
        // peer 配对完成前/中途断流时,member_repo 已经写但 trusted_peer 还没
        // 写——unpair 时 trusted_peer.remove 返回 Ok(false)(不存在),应当
        // 视为正常完成,不应抛错。
        let mut repo = MockMemberRepo::new();
        repo.expect_remove().times(1).returning(|_| Ok(true));
        let mut peer_addr = MockPeerAddrRepo::new();
        peer_addr.expect_remove().times(1).returning(|_| Ok(()));
        let mut trusted = MockTrustedPeerRepo::new();
        trusted.expect_remove().times(1).returning(|_| Ok(false));
        let id = MockLocalIdentity::new();
        let presence = Arc::new(FakePresence::new(vec![]));
        let facade = build_facade_with_unpair_repos(repo, peer_addr, trusted, id, presence);

        facade.revoke_member("dev-1").await.expect("ok");
    }

    #[tokio::test]
    async fn revoke_member_skips_cleanup_when_member_not_found() {
        // member_repo.remove 返回 false 表示成员不存在 —— facade 直接返回
        // NotFound,不应再去碰 peer_addr_repo / trusted_peer_repo。两个 mock
        // 都不设 expect_remove,被调用即 panic。
        let mut repo = MockMemberRepo::new();
        repo.expect_remove().times(1).returning(|_| Ok(false));
        let peer_addr = MockPeerAddrRepo::new();
        let trusted = MockTrustedPeerRepo::new();
        let id = MockLocalIdentity::new();
        let presence = Arc::new(FakePresence::new(vec![]));
        let facade = build_facade_with_unpair_repos(repo, peer_addr, trusted, id, presence);

        let err = facade.revoke_member("ghost").await.unwrap_err();
        assert!(matches!(err, RosterError::NotFound(d) if d == "ghost"));
    }

    #[tokio::test]
    async fn revoke_member_propagates_peer_addr_repo_failure() {
        // member 已经删了无法回滚 —— peer_addr 失败短路返回错误,
        // trusted_peer 也不应再被调用(短路,mock 不设 expect_remove)。
        let mut repo = MockMemberRepo::new();
        repo.expect_remove().times(1).returning(|_| Ok(true));
        let mut peer_addr = MockPeerAddrRepo::new();
        peer_addr
            .expect_remove()
            .times(1)
            .returning(|_| Err(PeerAddressError::Internal("disk full".into())));
        let trusted = MockTrustedPeerRepo::new();
        let id = MockLocalIdentity::new();
        let presence = Arc::new(FakePresence::new(vec![]));
        let facade = build_facade_with_unpair_repos(repo, peer_addr, trusted, id, presence);

        let err = facade.revoke_member("dev-1").await.unwrap_err();
        match err {
            RosterError::PeerAddressRepository(msg) => {
                assert!(msg.contains("disk full"), "msg = {msg}");
            }
            other => panic!("expected PeerAddressRepository, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn revoke_member_propagates_trusted_peer_repo_failure() {
        // member + peer_addr 都成功后,trusted_peer 失败要冒泡 ——
        // UI 才能感知"trust 没清,重新配对仍可能 AlreadyTrusted",
        // 启动期 reconcile_trusted_peers 会在下次 boot 兜底。
        let mut repo = MockMemberRepo::new();
        repo.expect_remove().times(1).returning(|_| Ok(true));
        let mut peer_addr = MockPeerAddrRepo::new();
        peer_addr.expect_remove().times(1).returning(|_| Ok(()));
        let mut trusted = MockTrustedPeerRepo::new();
        trusted
            .expect_remove()
            .times(1)
            .returning(|_| Err(TrustedPeerError::Repository("io error".into())));
        let id = MockLocalIdentity::new();
        let presence = Arc::new(FakePresence::new(vec![]));
        let facade = build_facade_with_unpair_repos(repo, peer_addr, trusted, id, presence);

        let err = facade.revoke_member("dev-1").await.unwrap_err();
        match err {
            RosterError::TrustedPeerRepository(msg) => {
                assert!(msg.contains("io error"), "msg = {msg}");
            }
            other => panic!("expected TrustedPeerRepository, got {other:?}"),
        }
    }
}
