//! [`AnalyticsPort`] —— 产品 telemetry 的上报抽象。
//!
//! 设计取舍：
//!
//! - **同步 fire-and-forget**：`capture` 不返回 `Future`、不返回错误。
//!   产品事件丢失少量是可接受的（schema doc §6 / §10），调用路径绝不应该
//!   因为 telemetry 阻塞或出错。
//! - **不传 `EventContext`**：实现者在构造时持有 context。这样：
//!   - 调用方只关心"发生了什么事"，不关心"我是谁"；
//!   - context 字段的命名冲突由 sink 负责仲裁（[`super::events::Event::properties`]
//!     永远不会与 [`super::context::EventContext`] 字段同名）。
//! - **gate 责任在调用方**：进入 `capture` 之前调用方应自行查询
//!   [`crate::analytics_gate::is_analytics_enabled`]。`NoopAnalyticsSink`
//!   不再二次检查——避免给真实 sink 实现一个错误的"我是 noop 就忽略"的范式。

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
        }));
    }

    #[test]
    fn analytics_port_is_object_safe() {
        // 编译期断言：`AnalyticsPort` 可以做成 trait object。
        // 这关系到 use case 能不能持有 `Arc<dyn AnalyticsPort>`。
        let _boxed: Box<dyn AnalyticsPort> = Box::new(NoopAnalyticsSink);
    }
}
