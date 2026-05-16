//! [`AnalyticsPort`] —— 产品 telemetry 的上报抽象。
//!
//! 设计取舍：
//!
//! - **同步 fire-and-forget**：`capture` / `identify` 不返回 `Future`、不返回错误。
//!   产品事件丢失少量是可接受的（schema doc §6 / §10），调用路径绝不应该
//!   因为 telemetry 阻塞或出错。
//! - **不传 `EventContext`**：实现者在构造时持有 context。这样：
//!   - 调用方只关心"发生了什么事"，不关心"我是谁"；
//!   - context 字段的命名冲突由 sink 负责仲裁（[`super::events::Event::properties`]
//!     永远不会与 [`super::context::EventContext`] 字段同名）。
//! - **gate 责任在调用方**：进入 `capture` / `identify` 之前调用方应自行查询
//!   [`crate::analytics_gate::is_analytics_enabled`]。`NoopAnalyticsSink`
//!   不再二次检查——避免给真实 sink 实现一个错误的"我是 noop 就忽略"的范式。
//!
//! ## 为什么需要 [`AnalyticsPort::identify`]（v2）
//!
//! v2 跨设备 person 聚合需要在 distinct_id 切换瞬间发一条 PostHog 标准的
//! `$identify` 系统事件——服务端据此把"老 anonymous person"与"新 space
//! shared person"合并归档（`$identify` 之前的所有事件归并到 new person 名下）。
//!
//! `identify` 与 `capture` 平行而非嵌套：调用方在如下时机显式触发——
//! - sponsor A1 `setup_completed`：把本机 anonymous_user_id 链到新生成的 space_person_id
//! - joiner A2 `pairing_succeeded`：把本机 anonymous_user_id 链到 sponsor 派发的 space_person_id
//! - 用户重置 telemetry：把当前 distinct_id 链回新生成的 anonymous_user_id
//! - switch_space：把当前 space_person_id 链到目标 Space 的 space_person_id
//!
//! 详细 wire 形态见 [`IdentifyPayload`] 与 schema doc §3.4 / §7。

use serde_json::Map;
use serde_json::Value;
use uuid::Uuid;

use super::events::Event;

/// 产品 telemetry 上报抽象。
///
/// 实现示例（计划中）：
/// - `NoopAnalyticsSink` —— 当前文件，丢弃所有事件。
/// - `StdoutSink` —— dev 构建专用，把事件序列化为 JSON 行打印到 stdout。
/// - `PosthogSink` —— 走 PostHog Cloud SDK 上报，包内部维护批量队列。
///
/// 实现者必须：
/// - 在内部持有一份 [`super::context::EventContext`] 与每条事件 properties
///   合并后再上传；
/// - 保证 `capture` 不阻塞调用线程超过几个微秒（重 IO 走 spawn / queue）；
/// - 失败时记录 `tracing::warn!`，**不**向上传播错误。
pub trait AnalyticsPort: Send + Sync {
    fn capture(&self, event: Event);

    /// 触发一次 PostHog 标准 `$identify`，让服务端把 `old_distinct_id` 名下
    /// 的历史事件合并到 `new_distinct_id`。
    ///
    /// 默认 noop —— `NoopAnalyticsSink` 与未来可能的"忽略 identify 的轻量 sink"
    /// 都靠这条保持简洁；真实 sink (`StdoutSink` / `PosthogSink`) 必须 override。
    ///
    /// 调用方义务：
    /// - 仅在 distinct_id 真正变化时调用一次（`old != new`）；
    /// - 在新 distinct_id 已经写入 [`crate::analytics::context::EventContext`]
    ///   并替换全局之后，先调 `identify`，再 emit 后续业务事件——保证后续事件
    ///   已经按新 person 上报。
    ///
    /// 实现者义务：
    /// - 失败时 `tracing::warn!` 不上抛；
    /// - 与 `capture` 同样 fire-and-forget，不阻塞调用线程；
    /// - 已经处于 SpaceShared 状态时调用方可能再次调 identify（switch_space），
    ///   实现者不做去重，**幂等性由调用方保证**。
    #[allow(unused_variables)]
    fn identify(&self, payload: IdentifyPayload) {
        // 默认空实现 —— 见 doc。Noop sink 走默认；真实 sink 主动 override。
    }

    /// 触发一次 PostHog 标准 `$groupidentify`，把 group property 写到指定
    /// `group_type:group_key`（v2 仅用 `group_type = "space"`）。
    ///
    /// 默认 noop。真实 sink (`StdoutSink` / `PosthogSink`) 必须 override。
    ///
    /// 调用时机（schema doc §3.4）：
    /// - A1 sponsor `setup_completed` identify 之后立即 fire 一次，把
    ///   `created_at` / `device_count: 1` 写入新建的 group。
    /// - 后续 person 自动通过事件上的 `$groups` 聚合到 group 下，PostHog 端
    ///   dashboard 直接 query group 下的 distinct person 数即可，无需手动重发。
    #[allow(unused_variables)]
    fn group_identify(&self, payload: GroupIdentifyPayload) {
        // 默认空实现。
    }
}

/// `$identify` 的入参。
///
/// 字段映射到 PostHog wire 形态：
///
/// ```json
/// {
///   "event": "$identify",
///   "distinct_id": "<new_distinct_id>",
///   "properties": {
///     "$anon_distinct_id": "<old_distinct_id>",
///     "$set":      { ...IdentifyPayload.set... },
///     "$set_once": { ...IdentifyPayload.set_once... }
///   }
/// }
/// ```
///
/// `$anon_distinct_id` 必须放在 `properties` 内、**不在顶层**——这是 PostHog
/// alias 合并的协议要求（顶层放 new_distinct_id，properties 放 old）。
#[derive(Debug, Clone)]
pub struct IdentifyPayload {
    /// 切换前的 distinct_id —— 通常是本机 `anonymous_user_id`。
    pub old_distinct_id: Uuid,
    /// 切换后的 distinct_id —— SpaceShared 状态下是 `space_person_id`，
    /// reset 状态下是新生成的 `anonymous_user_id`。
    pub new_distinct_id: Uuid,
    /// 写入 person property 的当前快照（`$set` 语义：每次 identify 覆盖）。
    pub set: Map<String, Value>,
    /// 仅 person 首次出现时写入的不变量（`$set_once` 语义）。
    pub set_once: Map<String, Value>,
}

impl IdentifyPayload {
    /// 便捷构造：仅传两个 distinct_id，set / set_once 为空。
    ///
    /// PR 4/6/8 触发的 identify 多数走最小形态——person 维度的 set/set_once
    /// 由后续 capture 事件通过 PostHog SDK 的 `$set` 字段自动维护。
    pub fn switch_only(old_distinct_id: Uuid, new_distinct_id: Uuid) -> Self {
        Self {
            old_distinct_id,
            new_distinct_id,
            set: Map::new(),
            set_once: Map::new(),
        }
    }
}

/// `$groupidentify` 的入参。
///
/// 字段映射到 PostHog wire 形态：
///
/// ```json
/// {
///   "event": "$groupidentify",
///   "distinct_id": "<distinct_id>",
///   "properties": {
///     "$group_type": "<group_type>",
///     "$group_key":  "<group_key>",
///     "$group_set":      { ...set... }
///   }
/// }
/// ```
///
/// `distinct_id` 走当前进程的 `analytics_person_id`（与 capture 同一来源）；
/// 由 sink 在发包时从全局 [`super::context::EventContext`] 取，**不**由本结构
/// 携带——避免调用方手动传错的失误。
#[derive(Debug, Clone)]
pub struct GroupIdentifyPayload {
    /// PostHog group type（v2 仅用 `"space"`）。
    pub group_type: String,
    /// 该 group 的 key —— v2 用 `space_id_hash`（schema doc §6.3）。
    pub group_key: String,
    /// group property 写入快照（覆盖语义，与 person `$set` 同款）。
    pub set: Map<String, Value>,
}

impl GroupIdentifyPayload {
    /// 便捷构造：v2 当前只用 `group_type = "space"`。
    pub fn for_space(group_key: String, set: Map<String, Value>) -> Self {
        Self {
            group_type: "space".to_string(),
            group_key,
            set,
        }
    }
}

/// 默认的 noop 实现，丢弃所有事件。
///
/// 用途：
/// - 单元测试中不需要真实 sink 的 use case 默认实现。
/// - 进程启动初期（sink 未初始化前）的占位实现。
/// - DSN 未配置或用户主动关闭时的 fallback——但这种场景应在调用方通过
///   [`crate::analytics_gate`] 提前过滤，不要依赖 sink 自己降级。
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopAnalyticsSink;

impl AnalyticsPort for NoopAnalyticsSink {
    #[inline]
    fn capture(&self, _event: Event) {
        // 故意空实现。
    }
}

#[cfg(test)]
mod tests {
    use super::super::events::{
        Direction, Event, PayloadSizeBucket, PayloadType, SyncEventProps, TransportType,
    };
    use super::*;

    #[test]
    fn noop_sink_accepts_all_event_variants_without_panicking() {
        let sink = NoopAnalyticsSink;
        // 把当前 v1 的关键事件都跑一遍——这同时是 trait 对象兼容性的烟测。
        let port: &dyn AnalyticsPort = &sink;
        port.capture(Event::AppFirstOpen);
        port.capture(Event::SyncAttempted(SyncEventProps {
            direction: Direction::Outbound,
            payload_type: PayloadType::Text,
            payload_size_bucket: PayloadSizeBucket::Lt1Kb,
            transport_type: TransportType::Local,
            peer_os: None,
            sync_latency_ms: None,
            failure_reason: None,
            failure_stage: None,
        }));
    }

    #[test]
    fn analytics_port_is_object_safe() {
        // 编译期断言：`AnalyticsPort` 可以做成 trait object。
        // 这关系到 use case 能不能持有 `Arc<dyn AnalyticsPort>`。
        let _boxed: Box<dyn AnalyticsPort> = Box::new(NoopAnalyticsSink);
    }
}
