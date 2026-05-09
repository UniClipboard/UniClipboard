//! `GatedAnalyticsSink` —— 在 `capture` 入口统一查询
//! [`crate::analytics_gate`] 的 wrapper sink。
//!
//! 决策（task_plan.md Decisions Made 表）：`usage_analytics_enabled` 是
//! 横切关注点，不该污染 sink 实现。`StdoutSink` / 未来 `PosthogSink`
//! 都不感知 gate；本 wrapper 在它们外面统一守卫一次。
//!
//! ## 装配点
//!
//! `uc-bootstrap::analytics::build_analytics_sink` 在装配时把真实 sink
//! 用本 wrapper 包一层后存进 `AppDeps.analytics`。settings PUT handler
//! 翻 `usage_analytics_enabled` 时只动 `analytics_gate` 的 atomic
//! 静态值，sink 本身不重建。
//!
//! ## 与 `AnalyticsPort` 契约的关系
//!
//! [`AnalyticsPort`] 模块文档说"gate 责任在调用方"。从 use case 视角看，
//! `GatedAnalyticsSink` 就是真实 sink 的"调用方"——所以契约依然成立：
//! 业务 use case 调本 wrapper，wrapper 查询 gate 后再调 inner sink。

use std::sync::Arc;

use super::super::events::Event;
use super::super::port::AnalyticsPort;
use crate::analytics_gate::is_analytics_enabled;

/// 在 `capture` 入口查询 [`is_analytics_enabled`] 的 wrapper sink。
///
/// gate 关闭时直接 `return`，不构造任何 IO 也不调 inner。打开时透传
/// 给 inner sink。
pub struct GatedAnalyticsSink {
    inner: Arc<dyn AnalyticsPort>,
}

impl GatedAnalyticsSink {
    pub fn new(inner: Arc<dyn AnalyticsPort>) -> Self {
        Self { inner }
    }
}

impl AnalyticsPort for GatedAnalyticsSink {
    #[inline]
    fn capture(&self, event: Event) {
        if !is_analytics_enabled() {
            return;
        }
        self.inner.capture(event);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::super::super::events::Event;
    use super::*;
    use crate::analytics_gate::set_analytics_enabled;

    /// 计数 sink，验证 wrapper 是否真的把事件透传给 inner。
    #[derive(Default)]
    struct CountingSink {
        count: AtomicUsize,
    }

    impl CountingSink {
        fn count(&self) -> usize {
            self.count.load(Ordering::Relaxed)
        }
    }

    impl AnalyticsPort for CountingSink {
        fn capture(&self, _event: Event) {
            self.count.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// gate 是 process-wide 静态值，多个测试 fn 并行会竞态。和
    /// `analytics::context::tests::global_event_context_lifecycle` 同样
    /// 用单一 fn 串行化，避免引入 `serial_test` 依赖。
    #[test]
    fn gated_sink_lifecycle() {
        // 用 Arc 让 wrapper 与测试断言共享同一个计数器。
        let inner = Arc::new(CountingSink::default());
        let inner_for_assert = Arc::clone(&inner);
        let sink = GatedAnalyticsSink::new(inner as Arc<dyn AnalyticsPort>);

        // —— case 1：gate 开 → inner 被调 ——
        set_analytics_enabled(true);
        sink.capture(Event::AppFirstOpen);
        sink.capture(Event::AppFirstOpen);
        assert_eq!(inner_for_assert.count(), 2, "gate 开时事件应透传");

        // —— case 2：gate 关 → inner 不被调，count 不增 ——
        set_analytics_enabled(false);
        sink.capture(Event::AppFirstOpen);
        sink.capture(Event::AppFirstOpen);
        sink.capture(Event::AppFirstOpen);
        assert_eq!(inner_for_assert.count(), 2, "gate 关时事件应丢弃");

        // —— case 3：gate 再度开启 → 透传恢复 ——
        set_analytics_enabled(true);
        sink.capture(Event::AppFirstOpen);
        assert_eq!(inner_for_assert.count(), 3, "gate 翻回开应恢复透传");

        // —— 收尾：还原默认值（true），避免污染其他测试 ——
        set_analytics_enabled(true);
    }
}
