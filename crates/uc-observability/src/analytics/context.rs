//! `EventContext` —— 每个事件必带的共享上下文。
//!
//! 字段定义对应 schema doc §4。Sink 在 capture 时把 [`global_event_context`]
//! 读到的快照与事件 properties 合并上传——调用方不需要重复传这些字段。
//!
//! `analytics_device_id` 与 `uc-core` 域内的业务 `DeviceId` 完全 disjoint，
//! 见 schema doc §3.1。
//!
//! ## 为什么需要 [`AnalyticsPersonId`]（v2 跨设备 person 聚合）
//!
//! v1：每台设备各自的 `anonymous_user_id` 直接做 distinct_id —— PostHog 把
//! 每台设备视为独立 person，"同一真实用户的多台设备"无法聚合做留存。
//!
//! v2：引入 [`AnalyticsPersonId`] 把"distinct_id 的逻辑来源"显式建模：
//!
//! - [`AnalyticsPersonId::Solo`]：未加入 Space，沿用本机 `anonymous_user_id`，
//!   行为与 v1 兼容。
//! - [`AnalyticsPersonId::SpaceShared`]：已加入 Space，使用 sponsor 派发的
//!   `space_person_id`（同 Space 多设备共享）。
//!
//! `EventContext.analytics_person_id` 字段**不进 wire**（`#[serde(skip)]`），
//! 它是 sink 派生 distinct_id 的输入。把这层显式化在 PR 2 让
//! `build_event_payload` 改为基于此字段派生 distinct_id；PR 1 仅引入类型与
//! 持久化能力，不改任何上报路径。详见 schema doc §3.4。
//!
//! ## 时间戳
//!
//! `EventContext` **不**包含 `timestamp` 字段。每条事件的时间戳由 sink 在
//! capture 时打（`Utc::now()`），或交给后端 SDK 自动注入（PostHog 会把
//! `$timestamp` 加到事件 envelope 上）。

use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 事件共享上下文，每条事件都会带这些字段。
///
/// 一个进程一份，session 内不可变（`active_device_count` 在进程启动时
/// 读取一次后缓存——见 schema doc §4 末尾"`active_device_count` 的语义"）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EventContext {
    /// 留存计算的"用户" ID。永久持久化，可由用户在设置页主动重置。
    pub anonymous_user_id: Uuid,
    /// 设备级切片用 ID。**不要**与 `uc-core::DeviceId` 关联或派生。
    pub analytics_device_id: Uuid,
    /// 单次进程运行的会话 ID，进程重启后失效。
    pub session_id: Uuid,

    /// crate version，例如 `"0.7.0-alpha.6"`。
    pub app_version: String,
    /// 发布渠道。
    pub app_channel: AppChannel,

    /// 操作系统。
    pub os: Os,
    /// 操作系统版本字符串，例如 macOS `"15.1"`、Windows `"10.0.22631"`、
    /// 未能探测时为 `"unknown"`。
    pub os_version: String,
    /// CPU 架构。
    pub arch: Arch,
    /// BCP-47 区域标签，例如 `"zh-CN"`、未能探测时为 `"unknown"`。
    pub locale: String,
    /// 时区。理想情况是 IANA 名（`"Asia/Shanghai"`），v1 退化为 UTC offset
    /// （`"+08:00"`）也可接受。
    pub timezone: String,

    /// 安装来源——v1 简化策略详见 schema doc §4.1。
    pub install_source: InstallSource,

    /// 仅本次 session 是首次运行时为 `true`。
    pub is_first_run: bool,
    /// 当前 Space 内已配对设备数，进程启动读一次后缓存。
    pub active_device_count: u32,

    /// `space_id` 的不可逆哈希（SHA-256 取前 16 hex char），未加入 Space 时为
    /// `None`。原始 `space_id` 永远不上传。
    pub space_id_hash: Option<String>,

    /// 派生 distinct_id 的逻辑身份（v2 跨设备 person 聚合）。
    ///
    /// **不进 wire**：`#[serde(skip)]` 让本字段不出现在 sink 上报的 JSON
    /// payload 里——它只是 sink 派生 `distinct_id` 的输入。
    ///
    /// PR 1 阶段所有上报点的 `distinct_id` 仍由 `anonymous_user_id` 派生
    /// （[`super::sinks::build_event_payload`] 未改），所以本字段在 PR 1
    /// 暂未被消费；PR 2 会切换 sink 派生逻辑。详见 schema doc §3.4。
    #[serde(skip)]
    pub analytics_person_id: AnalyticsPersonId,
}

/// 派生 distinct_id 的逻辑身份（schema doc §3.4）。
///
/// PostHog 用 distinct_id 做 person 聚合主键。v1 直接拷 `anonymous_user_id`
/// 导致"同一真实用户的多台设备"在 PostHog 上是不同 person；v2 引入这层
/// enum 显式区分两种来源：
///
/// - [`Solo`](Self::Solo)：未加入 Space，distinct_id = `anonymous_user_id`，
///   行为与 v1 兼容。
/// - [`SpaceShared`](Self::SpaceShared)：已加入 Space，distinct_id =
///   `space_person_id`（A1 sponsor 创建 Space 时生成、A2 joiner 经 pairing
///   加密通道继承）。同 Space 多设备共享同一个 `space_person_id`，PostHog
///   端自动聚合为同一 person。
///
/// **不可派生自业务身份**：`space_person_id` 是独立的 UUIDv7，与
/// `uc-core::DeviceId` / `space_id` 完全 disjoint，不可互推（schema doc §3.1）。
///
/// **wire 形态**：本枚举本身用 internally tagged 形态（`{"kind":"solo","id":"..."}`），
/// 但当前不直接序列化进 [`EventContext`]（见 `analytics_person_id` 字段
/// 的 `#[serde(skip)]`）。tagged 形态备给未来若需要持久化或 IPC 传递时使用。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum AnalyticsPersonId {
    Solo(Uuid),
    SpaceShared(Uuid),
}

impl AnalyticsPersonId {
    /// 取出内层 UUID，无论是 Solo 还是 SpaceShared。
    ///
    /// sink 在派生 distinct_id 时调用此方法——枚举本身只编码"来源语义"，
    /// 上报需要的就是 UUID 本身。
    pub fn as_uuid(&self) -> Uuid {
        match self {
            Self::Solo(id) | Self::SpaceShared(id) => *id,
        }
    }

    /// 是否处于 SpaceShared 状态。便于上层判断"我是不是已经接受过 sponsor 派发"。
    pub fn is_space_shared(&self) -> bool {
        matches!(self, Self::SpaceShared(_))
    }
}

/// `Default` 仅供 `#[serde(skip)]` 反序列化时占位，**不应**被业务路径依赖。
///
/// 设为 `Solo(Uuid::nil())` 而非真实 ID：万一某个测试 fixture 没显式设过
/// 这个字段，sink 派生出的 distinct_id 会是全零的可识别值，便于 dashboard
/// 端发现"装配漏了"。生产路径下 `bootstrap` 必填此字段。
impl Default for AnalyticsPersonId {
    fn default() -> Self {
        Self::Solo(Uuid::nil())
    }
}

/// [`build_event_context`] 的输入——把"调用方提供的字段"与"本模块内部探测的
/// 字段"做了切分：
///
/// - 调用方提供：身份 IDs、`app_version`、`app_channel`、`install_source`、
///   `is_first_run`、`active_device_count`、`space_id_hash`。
/// - 本模块探测：`os` / `os_version` / `arch` / `locale` / `timezone`（走
///   [`super::probe`]）。`session_id` 也在 build 时由 [`Uuid::now_v7`] 生成。
#[derive(Debug, Clone)]
pub struct EventContextInputs {
    pub anonymous_user_id: Uuid,
    pub analytics_device_id: Uuid,
    pub app_version: String,
    pub app_channel: AppChannel,
    pub install_source: InstallSource,
    pub is_first_run: bool,
    pub active_device_count: u32,
    pub space_id_hash: Option<String>,
    /// 派生 distinct_id 的逻辑身份（v2）。
    ///
    /// bootstrap 在已有 `space_person_id` 持久化时传 `SpaceShared(...)`，
    /// 否则传 `Solo(anonymous_user_id)`。详见 schema doc §3.4 与
    /// [`AnalyticsPersonId`]。
    pub analytics_person_id: AnalyticsPersonId,
}

/// 构造 `EventContext`。
///
/// `session_id` 由本函数生成；调用方不应自行传入——避免不同调用点产生
/// 不一致的 session 概念。平台字段（OS / arch / locale / timezone）由
/// [`super::probe`] 探测；探测失败时使用 `"unknown"` 占位，**不**返回
/// 错误——telemetry 缺字段比 telemetry 缺事件代价小。
pub fn build_event_context(inputs: EventContextInputs) -> EventContext {
    EventContext {
        anonymous_user_id: inputs.anonymous_user_id,
        analytics_device_id: inputs.analytics_device_id,
        session_id: Uuid::now_v7(),
        app_version: inputs.app_version,
        app_channel: inputs.app_channel,
        os: super::probe::detect_os(),
        os_version: super::probe::detect_os_version(),
        arch: super::probe::detect_arch(),
        locale: super::probe::detect_locale(),
        timezone: super::probe::detect_timezone(),
        install_source: inputs.install_source,
        is_first_run: inputs.is_first_run,
        active_device_count: inputs.active_device_count,
        space_id_hash: inputs.space_id_hash,
        analytics_person_id: inputs.analytics_person_id,
    }
}

// —— 进程级全局注册表 ————————————————————————————————————————

/// 进程级 `EventContext` 单例。
///
/// 选 `RwLock<Option<Arc<...>>>` 而不是 `OnceLock` 的理由：用户在设置页
/// 重置 telemetry IDs（schema doc §3.3）时需要重建 context，`OnceLock` 不
/// 支持原地替换。读路径走 `Arc::clone`，每事件一次原子计数操作，可忽略。
static GLOBAL_EVENT_CONTEXT: RwLock<Option<Arc<EventContext>>> = RwLock::new(None);

/// 注册 / 替换进程级 `EventContext`。
///
/// - bootstrap init 阶段调用一次；
/// - 用户重置 telemetry IDs 后再次调用以让新 context 立即生效。
///
/// 失败时（极罕见的锁中毒）写 `tracing::warn!` 后丢弃——不传播错误，避免
/// 阻塞业务路径。
pub fn set_global_event_context(ctx: Arc<EventContext>) {
    match GLOBAL_EVENT_CONTEXT.write() {
        Ok(mut guard) => *guard = Some(ctx),
        Err(_) => {
            tracing::warn!("global event context lock 中毒，丢弃本次更新");
        }
    }
}

/// 读当前 `EventContext` 快照。返回 `None` 表示 bootstrap 还没设置过。
///
/// 热路径——sink 在 capture 时调用。每次返回 `Arc::clone`，调用方持有的
/// 是当时的快照，后续 [`set_global_event_context`] 不会影响已经取到的
/// `Arc`。
pub fn global_event_context() -> Option<Arc<EventContext>> {
    GLOBAL_EVENT_CONTEXT.read().ok()?.clone()
}

/// 清空 `EventContext`。仅用于 telemetry IDs 重置或测试。
///
/// 清空后到 [`set_global_event_context`] 再次设置之间，[`global_event_context`]
/// 返回 `None`，sink 应跳过事件——但调用方应通过 [`crate::analytics_gate`]
/// 提前过滤，不要依赖 sink 自己降级。
pub fn clear_global_event_context() {
    if let Ok(mut guard) = GLOBAL_EVENT_CONTEXT.write() {
        *guard = None;
    }
}

/// 测试专用：跨测试 fn 串行化对 [`GLOBAL_EVENT_CONTEXT`] 的访问。
///
/// 全局 RwLock 是单例资源——单 fn 内顺序断言就够。但本 crate 现在有
/// 三个 lifecycle 测试都触达全局（`context::tests::global_event_context_lifecycle`、
/// `sinks::stdout::tests::stdout_sink_lifecycle`、
/// `sinks::posthog::tests::posthog_sink_lifecycle`），cargo test 默认线程并发会
/// 让其中一个的 `clear` 把另一个的 `set` 顶掉。所有这类测试在 fn 入口拿一次
/// 此锁的 `MutexGuard`，整个 fn 体作为 critical section。
///
/// 锁中毒（前一个测试 panic）兜底走 `into_inner`，让后续测试照常拿到锁——单纯
/// "前一个 case 失败"不应级联失败下一个 case。
#[cfg(test)]
pub(crate) fn lock_global_event_context_for_tests() -> std::sync::MutexGuard<'static, ()> {
    use std::sync::{Mutex, OnceLock};
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

// —— 字段类型 ————————————————————————————————————————

/// 应用发布渠道。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AppChannel {
    Alpha,
    Beta,
    Stable,
}

/// 操作系统。`Other` 兜底未知 unix-like / 嵌入式平台。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Os {
    Macos,
    Windows,
    Linux,
    Ios,
    Android,
    Other,
}

/// CPU 架构。`Other` 兜底，新增架构应优先扩展枚举值。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Arch {
    #[serde(rename = "x86_64")]
    X86_64,
    #[serde(rename = "aarch64")]
    Aarch64,
    #[serde(rename = "other")]
    Other,
}

/// 安装来源——v1 固定枚举，避免开放字符串导致脏数据。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum InstallSource {
    V2ex,
    Reddit,
    HackerNews,
    Github,
    Twitter,
    Direct,
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_inputs() -> EventContextInputs {
        let anon = Uuid::now_v7();
        EventContextInputs {
            anonymous_user_id: anon,
            analytics_device_id: Uuid::now_v7(),
            app_version: "0.7.0-alpha.6".into(),
            app_channel: AppChannel::Alpha,
            install_source: InstallSource::Github,
            is_first_run: true,
            active_device_count: 2,
            space_id_hash: Some("abcdef0123456789".into()),
            analytics_person_id: AnalyticsPersonId::Solo(anon),
        }
    }

    // —— 枚举 wire 形态钉死（schema doc §5.3 / §8）——————————————

    #[test]
    fn os_serializes_to_lowercase() {
        assert_eq!(serde_json::to_value(Os::Macos).unwrap(), "macos");
        assert_eq!(serde_json::to_value(Os::Windows).unwrap(), "windows");
        assert_eq!(serde_json::to_value(Os::Linux).unwrap(), "linux");
        assert_eq!(serde_json::to_value(Os::Ios).unwrap(), "ios");
        assert_eq!(serde_json::to_value(Os::Android).unwrap(), "android");
        assert_eq!(serde_json::to_value(Os::Other).unwrap(), "other");
    }

    #[test]
    fn arch_preserves_canonical_form() {
        assert_eq!(serde_json::to_value(Arch::X86_64).unwrap(), "x86_64");
        assert_eq!(serde_json::to_value(Arch::Aarch64).unwrap(), "aarch64");
        assert_eq!(serde_json::to_value(Arch::Other).unwrap(), "other");
    }

    #[test]
    fn install_source_uses_snake_case() {
        assert_eq!(serde_json::to_value(InstallSource::V2ex).unwrap(), "v2ex");
        assert_eq!(
            serde_json::to_value(InstallSource::Reddit).unwrap(),
            "reddit"
        );
        assert_eq!(
            serde_json::to_value(InstallSource::HackerNews).unwrap(),
            "hacker_news"
        );
        assert_eq!(
            serde_json::to_value(InstallSource::Github).unwrap(),
            "github"
        );
        assert_eq!(
            serde_json::to_value(InstallSource::Twitter).unwrap(),
            "twitter"
        );
        assert_eq!(
            serde_json::to_value(InstallSource::Direct).unwrap(),
            "direct"
        );
        assert_eq!(
            serde_json::to_value(InstallSource::Unknown).unwrap(),
            "unknown"
        );
    }

    #[test]
    fn app_channel_uses_snake_case() {
        assert_eq!(serde_json::to_value(AppChannel::Alpha).unwrap(), "alpha");
        assert_eq!(serde_json::to_value(AppChannel::Beta).unwrap(), "beta");
        assert_eq!(serde_json::to_value(AppChannel::Stable).unwrap(), "stable");
    }

    // —— EventContext 序列化 ——————————————————————————————

    #[test]
    fn event_context_round_trips_through_json() {
        // `analytics_person_id` 字段标了 `#[serde(skip)]`，在 round-trip 后会
        // 退回 [`AnalyticsPersonId::default`]——这是 PR 1 的有意设计，sink 派生
        // distinct_id 时直接读运行时 ctx，不依赖 wire 上的字段。
        // 把两侧都归零再比较，验证除该字段外的其它字段必须 byte-for-byte 还原。
        let ctx = build_event_context(sample_inputs());
        let json = serde_json::to_value(&ctx).unwrap();
        let back: EventContext = serde_json::from_value(json).unwrap();

        let mut ctx_norm = ctx.clone();
        let mut back_norm = back.clone();
        ctx_norm.analytics_person_id = AnalyticsPersonId::default();
        back_norm.analytics_person_id = AnalyticsPersonId::default();
        assert_eq!(ctx_norm, back_norm);
    }

    /// PR 1 红线：`analytics_person_id` 是 sink 派生 distinct_id 的输入，
    /// **不应**直接出现在 wire payload 上——否则 PostHog dashboard 会多出
    /// 一个 nested object 字段，污染所有现有埋点的字段集，违反 schema doc §8
    /// "wire 演化非破坏"。
    #[test]
    fn analytics_person_id_does_not_serialize_into_event_context() {
        let ctx = build_event_context(sample_inputs());
        let json = serde_json::to_value(&ctx).unwrap();
        let map = json.as_object().unwrap();
        assert!(
            !map.contains_key("analytics_person_id"),
            "analytics_person_id 必须 #[serde(skip)]，不进 wire：{map:?}"
        );
    }

    // —— AnalyticsPersonId 行为 ————————————————————————————————

    #[test]
    fn analytics_person_id_solo_serializes_with_kind_tag() {
        let id = Uuid::parse_str("018f0000-0000-7000-8000-000000000001").unwrap();
        let json = serde_json::to_value(AnalyticsPersonId::Solo(id)).unwrap();
        assert_eq!(json["kind"], "solo");
        assert_eq!(json["id"], id.to_string());
    }

    #[test]
    fn analytics_person_id_space_shared_serializes_with_kind_tag() {
        let id = Uuid::parse_str("018f0000-0000-7000-8000-000000000002").unwrap();
        let json = serde_json::to_value(AnalyticsPersonId::SpaceShared(id)).unwrap();
        assert_eq!(json["kind"], "space_shared");
        assert_eq!(json["id"], id.to_string());
    }

    #[test]
    fn analytics_person_id_round_trips_through_json() {
        for original in [
            AnalyticsPersonId::Solo(Uuid::now_v7()),
            AnalyticsPersonId::SpaceShared(Uuid::now_v7()),
        ] {
            let json = serde_json::to_value(&original).unwrap();
            let back: AnalyticsPersonId = serde_json::from_value(json).unwrap();
            assert_eq!(original, back);
        }
    }

    #[test]
    fn analytics_person_id_as_uuid_returns_inner_value() {
        let id = Uuid::now_v7();
        assert_eq!(AnalyticsPersonId::Solo(id).as_uuid(), id);
        assert_eq!(AnalyticsPersonId::SpaceShared(id).as_uuid(), id);
    }

    #[test]
    fn analytics_person_id_is_space_shared_only_for_space_shared_variant() {
        let id = Uuid::now_v7();
        assert!(!AnalyticsPersonId::Solo(id).is_space_shared());
        assert!(AnalyticsPersonId::SpaceShared(id).is_space_shared());
    }

    #[test]
    fn analytics_person_id_default_is_solo_nil() {
        // schema doc §3.4 末尾：default 是占位值，业务路径必须显式赋。
        // 设为 nil UUID 是为了在 dashboard 上"看到全零"立即识别装配漏洞。
        assert_eq!(
            AnalyticsPersonId::default(),
            AnalyticsPersonId::Solo(Uuid::nil())
        );
    }

    #[test]
    fn event_context_field_names_are_snake_case() {
        let ctx = build_event_context(sample_inputs());
        let json = serde_json::to_value(&ctx).unwrap();
        let map = json.as_object().unwrap();
        for key in map.keys() {
            assert!(
                key.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                "EventContext field `{key}` 不是 snake_case"
            );
        }
    }

    #[test]
    fn event_context_does_not_carry_timestamp() {
        // schema 修订：timestamp 是 sink 级字段，不在 context 中。
        let ctx = build_event_context(sample_inputs());
        let json = serde_json::to_value(&ctx).unwrap();
        let map = json.as_object().unwrap();
        assert!(
            !map.contains_key("timestamp"),
            "EventContext 不应携带 timestamp"
        );
    }

    // —— factory ————————————————————————————————————————

    #[test]
    fn build_event_context_assigns_fresh_session_id() {
        let inputs = sample_inputs();
        let ctx_a = build_event_context(inputs.clone());
        let ctx_b = build_event_context(inputs);
        assert_ne!(
            ctx_a.session_id, ctx_b.session_id,
            "每次 build 都应分配新的 session_id"
        );
        // session_id 必须是 UUIDv7，便于按时间排序排查。
        assert_eq!(ctx_a.session_id.get_version_num(), 7);
    }

    #[test]
    fn build_event_context_passes_through_caller_supplied_fields() {
        let inputs = sample_inputs();
        let expected_anon = inputs.anonymous_user_id;
        let expected_device = inputs.analytics_device_id;
        let expected_version = inputs.app_version.clone();

        let ctx = build_event_context(inputs);
        assert_eq!(ctx.anonymous_user_id, expected_anon);
        assert_eq!(ctx.analytics_device_id, expected_device);
        assert_eq!(ctx.app_version, expected_version);
        assert_eq!(ctx.app_channel, AppChannel::Alpha);
        assert_eq!(ctx.install_source, InstallSource::Github);
        assert!(ctx.is_first_run);
        assert_eq!(ctx.active_device_count, 2);
        assert_eq!(ctx.space_id_hash.as_deref(), Some("abcdef0123456789"));
    }

    #[test]
    fn build_event_context_populates_platform_fields_non_empty() {
        // 探测细节由 probe 模块覆盖；这里只断言"占位也比空字符串好"，
        // 防御后续重构把 detect_* 改成可能返回空串的版本。
        let ctx = build_event_context(sample_inputs());
        assert!(!ctx.os_version.is_empty());
        assert!(!ctx.locale.is_empty());
        assert!(!ctx.timezone.is_empty());
    }

    // —— 全局注册表 ————————————————————————————————————
    //
    // 全局静态状态天生是单线程语义；多个 `#[test]` 同时改同一份 `RwLock`
    // 会互相覆盖。把所有"接触全局"的断言合并到一个测试里，靠测试函数自身
    // 顺序保证确定性，避免引入 `serial_test` 这种依赖。

    #[test]
    fn global_event_context_lifecycle() {
        let _guard = lock_global_event_context_for_tests();
        // 1) 起始态：set 之前应该是 None。
        clear_global_event_context();
        assert!(global_event_context().is_none(), "clear 后初始态应为 None");

        // 2) round-trip：set 之后 get 拿到同一份 Arc。
        let first = Arc::new(build_event_context(sample_inputs()));
        set_global_event_context(first.clone());
        let read = global_event_context().expect("set 后应能读到");
        assert!(
            Arc::ptr_eq(&first, &read),
            "global 应直接 Arc::clone 同一份"
        );

        // 3) 替换：再次 set 把 first 顶掉——schema doc §3.3 reset 流程依赖此。
        let mut second_inputs = sample_inputs();
        second_inputs.is_first_run = false;
        let second = Arc::new(build_event_context(second_inputs));
        set_global_event_context(second.clone());
        let read = global_event_context().expect("替换后应能读到");
        assert!(Arc::ptr_eq(&read, &second), "RwLock 模式必须支持替换");
        assert!(!read.is_first_run);

        // 4) clear：恢复初始态，避免污染同 binary 的其他测试。
        clear_global_event_context();
        assert!(global_event_context().is_none(), "clear 必须真清空");
    }
}
