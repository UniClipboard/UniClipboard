//! [`AnalyticsIdentityPort`] —— 协调 [`AnalyticsPersonId`] 持久化与全局
//! [`EventContext`] 重建。
//!
//! ## 为什么需要这个 port（v2 跨设备 person 聚合）
//!
//! v2 引入"逻辑用户身份"后，"切换 distinct_id 来源"涉及三件事必须**原子**完成：
//!
//! 1. 把 `space_person_id` 写盘（A1 sponsor 自生成 / A2 joiner 接收 sponsor 派发）
//!    或清空（reset / 退 Space）。
//! 2. 重建 process-wide [`EventContext`]，把 `analytics_person_id` 替换为新值。
//! 3. 让调用方据此发 PostHog `$identify`，把老 distinct_id 名下的历史事件
//!    合并归档到新 distinct_id。
//!
//! 第 1 步是文件 IO（uc-observability 自己持有 `analytics_dir`），第 2 步是
//! 进程级状态变更，第 3 步必须由 use case 显式触发（保证时序：先切再发，避免
//! 先发 identify 后切换失败导致服务端把不存在的身份合并）。
//!
//! 把 1 + 2 封装在本 port 后，use case 只需：
//!
//! ```ignore
//! let outcome = analytics_identity.adopt_space_person(new_id)?;
//! analytics.identify(IdentifyPayload::switch_only(outcome.previous_distinct_id, outcome.new_distinct_id));
//! ```
//!
//! 失败语义：[`adopt_space_person`] / [`release_space_person`] 任一步失败都
//! **不会**改动全局 ctx——文件 IO 失败时 ctx 保持原样；ctx 未初始化时直接报错。
//! 这给 use case 一个清晰的"全成功 / 全失败"边界，identify 只在 outcome 返回
//! `Ok` 时才发出。
//!
//! [`AnalyticsPersonId`]: super::context::AnalyticsPersonId
//! [`EventContext`]: super::context::EventContext

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use uuid::Uuid;

use super::context::{global_event_context, set_global_event_context, AnalyticsPersonId};
use super::ids::{clear_space_person_id, set_space_person_id};

/// 协调 `space_person_id` 持久化与 `EventContext` 重建的端口。
///
/// 实现端只负责"切换 distinct_id 的来源"——发 `$identify` 由调用方显式调
/// [`super::port::AnalyticsPort::identify`]，避免本 port 把所有 sink 都吃下
/// （sink 仍是另一条独立装配链）。
pub trait AnalyticsIdentityPort: Send + Sync {
    /// 接受一个 `SpaceShared` 身份。
    ///
    /// 步骤：
    /// 1. 把 `space_person_id` 落盘（覆盖旧值或新建文件）。
    /// 2. 把进程级 [`EventContext`] 的 `analytics_person_id` 替换为
    ///    `SpaceShared(space_person_id)`，其它字段保留。
    /// 3. 返回 [`AdoptOutcome`]——调用方据此发 `$identify`。
    ///
    /// 失败时全局 ctx 不变。
    ///
    /// [`EventContext`]: super::context::EventContext
    fn adopt_space_person(
        &self,
        space_person_id: Uuid,
    ) -> Result<AdoptOutcome, AnalyticsIdentityError>;

    /// 把身份切回 `Solo`（reset / 退 Space / 用户重置 telemetry）。
    ///
    /// 步骤：
    /// 1. 删除本机 `space_person_id` 文件（幂等：不存在不视为错误）。
    /// 2. 把进程级 [`EventContext`] 的 `analytics_person_id` 替换为
    ///    `Solo(anonymous_user_id)`。
    /// 3. 返回 [`ReleaseOutcome`]——调用方据此发 `$identify` 回到 anonymous。
    ///
    /// 失败时全局 ctx 不变。
    ///
    /// [`EventContext`]: super::context::EventContext
    fn release_space_person(&self) -> Result<ReleaseOutcome, AnalyticsIdentityError>;

    /// 读取本机当前持久化的 `space_person_id`。
    ///
    /// 返回 `None` 表示当前处于 `Solo` 状态——本机还没接受过任何 sponsor
    /// 派发，也没自己生成过（v1→v2 升级初期 / 用户刚 reset / 全新设备）。
    ///
    /// sponsor 端 pairing handshake 用这个值填入 `SponsorConfirm.sponsor_space_person_id`
    /// 派给 joiner；返回 `None` 时 sponsor 端不携带此字段，joiner 收到 `None`
    /// 退回 Solo。
    ///
    /// 实现端不抛错——文件读失败 / 损坏 / 不存在都返回 `None`，与
    /// [`super::ids::load_space_person_id`] 的语义一致。
    fn current_space_person_id(&self) -> Option<Uuid>;

    /// 用户重置 telemetry：清 `space_person_id` + 重新生成 `anonymous_user_id`
    /// 与 `analytics_device_id` + 重建 EventContext + 返回旧/新 distinct_id。
    ///
    /// schema doc §3.3：本机 reset **不影响**其他设备。其他设备仍持有原
    /// `space_person_id`，Space 维度的 person 不消失，只是本机被切回 Solo
    /// 的全新 anonymous 身份。
    ///
    /// 步骤：
    /// 1. 清除本机 `space_person_id` 文件（幂等）。
    /// 2. 删除 `installation_id` / `analytics_device_id` 文件并重新生成。
    /// 3. 重建进程级 [`EventContext`]：新 `anonymous_user_id`、新
    ///    `analytics_device_id`、`analytics_person_id = Solo(new_anon)`、
    ///    其它字段保留。
    /// 4. 返回 [`ReleaseOutcome`]，调用方据此发 `$identify` 把旧 distinct_id
    ///    名下的最近事件归并到新 anonymous（PostHog 只能合并未删除的 person
    ///    数据；旧 person 在 server 上仍然存在，不会被本端 reset 物理擦除）。
    ///
    /// 失败时全局 ctx 不变；调用方据 Err 决定是否提示用户重试。
    ///
    /// [`EventContext`]: super::context::EventContext
    fn reset_telemetry_identity(&self) -> Result<ReleaseOutcome, AnalyticsIdentityError>;
}

/// `adopt_space_person` 成功返回的两个 distinct_id 端点。
///
/// `previous_distinct_id` 通常是本机 `anonymous_user_id`（v1→v2 首次切换）
/// 或上一个 `space_person_id`（switch_space 跨 Space 切换）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdoptOutcome {
    pub previous_distinct_id: Uuid,
    pub new_distinct_id: Uuid,
}

/// `release_space_person` 成功返回的两个 distinct_id 端点。
///
/// `new_distinct_id` 始终等于本机 `anonymous_user_id`（Solo 状态的 distinct_id）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReleaseOutcome {
    pub previous_distinct_id: Uuid,
    pub new_distinct_id: Uuid,
}

/// `space_id` → telemetry group key 的不可逆哈希。
///
/// schema doc §6.3：原始 `space_id` 永远不上传。SHA-256(space_id) 取前 16
/// hex char（64 bit）作为 group key——既能跨事件做"同 Space 内聚合"又不
/// 暴露原 ID。bootstrap 装配 `EventContext.space_id_hash` 与 use case 调
/// `$groupidentify` 用的 `group_key` 共用本函数，保证 dashboard 上 group
/// 维度自洽。
pub fn hash_space_id_for_telemetry(space_id: &str) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(space_id.as_bytes());
    let mut out = String::with_capacity(16);
    for byte in digest.iter().take(8) {
        use std::fmt::Write;
        let _ = write!(out, "{:02x}", byte);
    }
    out
}

/// 切换身份时可能出现的错误。
#[derive(Debug)]
pub enum AnalyticsIdentityError {
    /// 全局 `EventContext` 还没装配——bootstrap 的 `compose_event_context`
    /// 必须在本 port 任何方法之前完成。
    ContextNotInitialised,

    /// 文件 IO 失败（写 `space_person_id` 文件 / 删除文件等）。
    PersistFailed(anyhow::Error),
}

impl fmt::Display for AnalyticsIdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ContextNotInitialised => write!(
                f,
                "global EventContext not initialised; bootstrap compose_event_context must run first"
            ),
            Self::PersistFailed(_) => write!(f, "persist space_person_id failed"),
        }
    }
}

impl std::error::Error for AnalyticsIdentityError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ContextNotInitialised => None,
            Self::PersistFailed(e) => Some(e.as_ref()),
        }
    }
}

/// `AnalyticsIdentityPort` 的本地实现：把 `space_person_id` 文件存在
/// `analytics_dir` 下，与 [`super::ids`] 共享文件布局。
///
/// 装配点：bootstrap 在 `compose_event_context` 之后构造一份并塞进 facade
/// deps（详见 `uc-bootstrap` 的 `build_analytics_identity`）。
pub struct LocalAnalyticsIdentity {
    analytics_dir: PathBuf,
}

impl LocalAnalyticsIdentity {
    pub fn new(analytics_dir: PathBuf) -> Self {
        Self { analytics_dir }
    }
}

/// Noop 实现：所有方法立即返回 `Ok` 但不改任何全局状态。
///
/// 用途：
/// - 单元测试不关心身份切换的 use case 的默认依赖；
/// - bootstrap 在 telemetry 全局禁用时的 fallback（与 `NoopAnalyticsSink` 同位）。
///
/// `previous_distinct_id` / `new_distinct_id` 都返回 [`Uuid::nil`]——和
/// [`AnalyticsPersonId::default`] 的"占位 nil"语义一致，dashboard 上看到全零
/// 立即识别"装配漏了"，与生产路径区分开。
///
/// [`AnalyticsPersonId::default`]: super::context::AnalyticsPersonId::default
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopAnalyticsIdentity;

impl AnalyticsIdentityPort for NoopAnalyticsIdentity {
    #[inline]
    fn adopt_space_person(
        &self,
        space_person_id: Uuid,
    ) -> Result<AdoptOutcome, AnalyticsIdentityError> {
        Ok(AdoptOutcome {
            previous_distinct_id: Uuid::nil(),
            new_distinct_id: space_person_id,
        })
    }

    #[inline]
    fn release_space_person(&self) -> Result<ReleaseOutcome, AnalyticsIdentityError> {
        Ok(ReleaseOutcome {
            previous_distinct_id: Uuid::nil(),
            new_distinct_id: Uuid::nil(),
        })
    }

    #[inline]
    fn current_space_person_id(&self) -> Option<Uuid> {
        // 测试 fallback：noop 始终是 Solo 状态。
        None
    }

    #[inline]
    fn reset_telemetry_identity(&self) -> Result<ReleaseOutcome, AnalyticsIdentityError> {
        Ok(ReleaseOutcome {
            previous_distinct_id: Uuid::nil(),
            new_distinct_id: Uuid::nil(),
        })
    }
}

impl AnalyticsIdentityPort for LocalAnalyticsIdentity {
    fn adopt_space_person(
        &self,
        space_person_id: Uuid,
    ) -> Result<AdoptOutcome, AnalyticsIdentityError> {
        let current =
            global_event_context().ok_or(AnalyticsIdentityError::ContextNotInitialised)?;
        let previous = current.analytics_person_id.as_uuid();

        // Persist 先于 ctx 替换：失败时 ctx 不变，调用方不会把"切换成功"的
        // 错觉传给后续 identify。
        set_space_person_id(&self.analytics_dir, space_person_id)
            .map_err(AnalyticsIdentityError::PersistFailed)?;

        let mut new_ctx = (*current).clone();
        new_ctx.analytics_person_id = AnalyticsPersonId::SpaceShared(space_person_id);
        set_global_event_context(Arc::new(new_ctx));

        Ok(AdoptOutcome {
            previous_distinct_id: previous,
            new_distinct_id: space_person_id,
        })
    }

    fn release_space_person(&self) -> Result<ReleaseOutcome, AnalyticsIdentityError> {
        let current =
            global_event_context().ok_or(AnalyticsIdentityError::ContextNotInitialised)?;
        let previous = current.analytics_person_id.as_uuid();
        let new = current.anonymous_user_id;

        clear_space_person_id(&self.analytics_dir)
            .map_err(AnalyticsIdentityError::PersistFailed)?;

        let mut new_ctx = (*current).clone();
        new_ctx.analytics_person_id = AnalyticsPersonId::Solo(new);
        set_global_event_context(Arc::new(new_ctx));

        Ok(ReleaseOutcome {
            previous_distinct_id: previous,
            new_distinct_id: new,
        })
    }

    fn current_space_person_id(&self) -> Option<Uuid> {
        match super::ids::load_space_person_id(&self.analytics_dir) {
            Ok(opt) => opt,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "current_space_person_id: 读盘失败，按 Solo 退化"
                );
                None
            }
        }
    }

    fn reset_telemetry_identity(&self) -> Result<ReleaseOutcome, AnalyticsIdentityError> {
        let current =
            global_event_context().ok_or(AnalyticsIdentityError::ContextNotInitialised)?;
        let previous = current.analytics_person_id.as_uuid();

        // Step 1: 清 space_person_id（幂等）。
        super::ids::clear_space_person_id(&self.analytics_dir)
            .map_err(AnalyticsIdentityError::PersistFailed)?;
        // Step 2: 删除 anonymous + device id 文件，下一步 load_or_create 会
        // 重新生成两个全新 ID。
        super::ids::reset(&self.analytics_dir).map_err(AnalyticsIdentityError::PersistFailed)?;
        // Step 3: 重新生成两个 ID 并落盘（is_first_run 会是 true）。
        let new_ids = super::ids::load_or_create(&self.analytics_dir)
            .map_err(AnalyticsIdentityError::PersistFailed)?;
        let new_anon = new_ids.anonymous_user_id;

        // Step 4: 重建全局 EventContext —— 替换 anonymous_user_id /
        // analytics_device_id / analytics_person_id 三个字段，其它保留。
        let mut new_ctx = (*current).clone();
        new_ctx.anonymous_user_id = new_anon;
        new_ctx.analytics_device_id = new_ids.analytics_device_id;
        new_ctx.analytics_person_id = AnalyticsPersonId::Solo(new_anon);
        set_global_event_context(Arc::new(new_ctx));

        Ok(ReleaseOutcome {
            previous_distinct_id: previous,
            new_distinct_id: new_anon,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::context::{
        build_event_context, clear_global_event_context, lock_global_event_context_for_tests,
        AppChannel, EventContextInputs, InstallSource,
    };
    use super::super::ids::load_space_person_id;
    use tempfile::TempDir;

    fn install_solo_ctx(anon: Uuid) {
        let ctx = build_event_context(EventContextInputs {
            anonymous_user_id: anon,
            analytics_device_id: Uuid::now_v7(),
            app_version: "0.7.0-alpha.7".into(),
            app_channel: AppChannel::Alpha,
            install_source: InstallSource::Unknown,
            is_first_run: false,
            active_device_count: 1,
            space_id_hash: None,
            analytics_person_id: AnalyticsPersonId::Solo(anon),
        });
        set_global_event_context(Arc::new(ctx));
    }

    /// adopt → release 在同一个 fn 串行化：global EventContext 是单例。
    /// 锁通过 [`lock_global_event_context_for_tests`] 跨 fn 互斥。
    #[test]
    fn adopt_then_release_flow() {
        let _guard = lock_global_event_context_for_tests();
        clear_global_event_context();

        let dir = TempDir::new().unwrap();
        let port = LocalAnalyticsIdentity::new(dir.path().to_path_buf());

        // —— 前置：装一个 Solo ctx ——
        let anon = Uuid::now_v7();
        install_solo_ctx(anon);
        assert!(load_space_person_id(dir.path()).unwrap().is_none());

        // —— adopt：persist + ctx 切换为 SpaceShared，返回 (anon, new) ——
        let new_id = Uuid::now_v7();
        let outcome = port.adopt_space_person(new_id).unwrap();
        assert_eq!(outcome.previous_distinct_id, anon);
        assert_eq!(outcome.new_distinct_id, new_id);

        let after_adopt = global_event_context().unwrap();
        assert_eq!(
            after_adopt.analytics_person_id,
            AnalyticsPersonId::SpaceShared(new_id),
            "adopt 后 ctx 应为 SpaceShared"
        );
        // anonymous_user_id 字段保留，未被覆盖。
        assert_eq!(after_adopt.anonymous_user_id, anon);
        // persist 落盘成功。
        assert_eq!(load_space_person_id(dir.path()).unwrap(), Some(new_id));

        // —— release：清盘 + ctx 回到 Solo(anon)，返回 (new, anon) ——
        let outcome = port.release_space_person().unwrap();
        assert_eq!(outcome.previous_distinct_id, new_id);
        assert_eq!(outcome.new_distinct_id, anon);

        let after_release = global_event_context().unwrap();
        assert_eq!(
            after_release.analytics_person_id,
            AnalyticsPersonId::Solo(anon),
            "release 后 ctx 应回到 Solo(anonymous_user_id)"
        );
        assert!(load_space_person_id(dir.path()).unwrap().is_none());

        // 收尾：清空，避免污染其它测试。
        clear_global_event_context();
    }

    #[test]
    fn adopt_without_global_context_returns_context_not_initialised() {
        let _guard = lock_global_event_context_for_tests();
        clear_global_event_context();

        let dir = TempDir::new().unwrap();
        let port = LocalAnalyticsIdentity::new(dir.path().to_path_buf());

        let err = port.adopt_space_person(Uuid::now_v7()).unwrap_err();
        assert!(matches!(err, AnalyticsIdentityError::ContextNotInitialised));
        // 也不应留下落盘文件——让 caller 的"按 outcome 决定是否 identify"语义成立。
        assert!(load_space_person_id(dir.path()).unwrap().is_none());
    }

    #[test]
    fn release_without_global_context_returns_context_not_initialised() {
        let _guard = lock_global_event_context_for_tests();
        clear_global_event_context();

        let dir = TempDir::new().unwrap();
        let port = LocalAnalyticsIdentity::new(dir.path().to_path_buf());

        let err = port.release_space_person().unwrap_err();
        assert!(matches!(err, AnalyticsIdentityError::ContextNotInitialised));
    }

    /// adopt 持久化失败时全局 ctx 不变——caller 的 identify 不应被发出。
    // —— hash_space_id_for_telemetry（schema doc §6.3）——————————

    #[test]
    fn hash_space_id_yields_16_hex_chars() {
        let h = hash_space_id_for_telemetry("space-abcdef-0123");
        assert_eq!(h.len(), 16, "schema doc §6.3：取前 16 hex");
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()), "hash 必须全 hex");
    }

    #[test]
    fn hash_space_id_is_deterministic() {
        // 同输入必须同输出，否则跨事件 group 聚合失效。
        assert_eq!(
            hash_space_id_for_telemetry("space-xyz"),
            hash_space_id_for_telemetry("space-xyz")
        );
    }

    #[test]
    fn hash_space_id_distinct_inputs_distinct_outputs() {
        // SHA-256 截 64 bit 理论上仍可能碰撞，但弱断言：典型场景下应不同。
        assert_ne!(
            hash_space_id_for_telemetry("space-aaa"),
            hash_space_id_for_telemetry("space-bbb")
        );
    }

    /// reset_telemetry_identity：清 space + 重新生成 anonymous/device + 重建 ctx。
    #[test]
    fn reset_telemetry_identity_replaces_anonymous_and_clears_space_person() {
        let _guard = lock_global_event_context_for_tests();
        clear_global_event_context();

        let dir = TempDir::new().unwrap();
        let port = LocalAnalyticsIdentity::new(dir.path().to_path_buf());

        // 前置：先 load_or_create 落盘一对 anonymous/device，再装 ctx 模拟
        // SpaceShared 状态。
        let original_ids = super::super::ids::load_or_create(dir.path()).unwrap();
        install_solo_ctx(original_ids.anonymous_user_id);
        let space_person = Uuid::now_v7();
        port.adopt_space_person(space_person).unwrap();

        let outcome = port.reset_telemetry_identity().unwrap();

        // previous_distinct_id 等于 reset 之前 ctx 中的 person id（SpaceShared）。
        assert_eq!(outcome.previous_distinct_id, space_person);
        // new_distinct_id 是新生成的 anonymous，**不**等于原 anonymous。
        assert_ne!(outcome.new_distinct_id, original_ids.anonymous_user_id);
        assert_ne!(outcome.new_distinct_id, space_person);

        // 全局 ctx：身份切回 Solo(new_anon)，新 anonymous 写到字段上。
        let after = global_event_context().unwrap();
        assert_eq!(
            after.analytics_person_id,
            AnalyticsPersonId::Solo(outcome.new_distinct_id)
        );
        assert_eq!(after.anonymous_user_id, outcome.new_distinct_id);
        // analytics_device_id 同样应被刷新。
        assert_ne!(after.analytics_device_id, original_ids.analytics_device_id);
        // space_person_id 文件被清。
        assert!(super::super::ids::load_space_person_id(dir.path())
            .unwrap()
            .is_none());

        clear_global_event_context();
    }

    #[test]
    fn adopt_persist_failure_leaves_global_context_unchanged() {
        let _guard = lock_global_event_context_for_tests();
        clear_global_event_context();

        // 用一个不存在父目录、且无法创建的路径触发文件 IO 失败：
        // macOS / Linux 都禁止往 `/dev/null/...` 创建子目录。
        let port = LocalAnalyticsIdentity::new(PathBuf::from("/dev/null/analytics-deny"));

        let anon = Uuid::now_v7();
        install_solo_ctx(anon);
        let before = global_event_context().unwrap().analytics_person_id.clone();

        let err = port.adopt_space_person(Uuid::now_v7()).unwrap_err();
        assert!(matches!(err, AnalyticsIdentityError::PersistFailed(_)));

        let after = global_event_context().unwrap().analytics_person_id.clone();
        assert_eq!(
            before, after,
            "persist 失败时 ctx 必须保持原样，否则 caller 会按错误的身份继续上报"
        );

        clear_global_event_context();
    }
}
