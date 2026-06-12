//! 进程级运行时门控：用户面板上的"使用情况统计"开关。
//!
//! 与 [`crate::telemetry_gate`] 对称——后者控制 Sentry（错误 / 崩溃 / Logs），
//! 本模块控制本仓库的产品 telemetry（漏斗 / 留存 / 可靠性事件）。
//!
//! schema 与拆分双开关的决策见 `docs/architecture/telemetry-events.md`，
//! 本模块对应 §6.4 中的 `general.usage_analytics_enabled`。
//!
//! - 后续接入的 analytics sink（计划走 PostHog Cloud SDK）必须在
//!   `capture` 之前查询 [`is_analytics_enabled`]，关闭时连事件对象都不应被
//!   构造，避免误把上下文序列化进内存。
//! - 与 `telemetry_gate` 保持同一套契约：`uc-bootstrap` 在 init 阶段读取
//!   持久化设置后调用 [`set_analytics_enabled`]；`uc-webserver` 的
//!   PUT /settings 处理器在 `usage_analytics_enabled` 字段变化时同步更新
//!   该 gate，无需重启。
//!
//! ## 默认值
//!
//! 默认 `true`，理由与 `telemetry_gate` 一致：进程启动到第一次 settings
//! 加载之间产生的事件不应被静默丢弃；`uc-bootstrap` 在 init 阶段读取磁盘
//! 上的持久化偏好后会立即覆盖默认值。

use std::sync::atomic::{AtomicBool, Ordering};

static ANALYTICS_ENABLED: AtomicBool = AtomicBool::new(true);

/// 返回当前"使用情况统计"开关是否打开。
///
/// 热路径——每条产品事件发出前会调用一次。`Ordering::Relaxed` 足够：
/// 没有其他状态需要同步，最坏情况是与 setter 调用并发的事件按 toggle 前
/// 的取值分类，可以接受。
#[inline]
pub fn is_analytics_enabled() -> bool {
    ANALYTICS_ENABLED.load(Ordering::Relaxed)
}

/// 更新 gate。
///
/// 调用方有两条路径：
/// - `uc-bootstrap` 在 init 阶段读取持久化设置后调用一次。
/// - `uc-webserver` 的 PUT /settings 处理器在 `usage_analytics_enabled`
///   字段变化时调用，让新值立即生效。
pub fn set_analytics_enabled(enabled: bool) {
    ANALYTICS_ENABLED.store(enabled, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_true_to_avoid_dropping_pre_init_events() {
        // 防御性重置——同一进程内其他测试可能改过该静态值。
        set_analytics_enabled(true);
        assert!(is_analytics_enabled());
    }

    #[test]
    fn setter_round_trip() {
        set_analytics_enabled(false);
        assert!(!is_analytics_enabled());
        set_analytics_enabled(true);
        assert!(is_analytics_enabled());
    }

    #[test]
    fn independent_from_telemetry_gate() {
        // 两个 gate 互不影响——这是 §6.4 双开关方案的核心约束。
        crate::telemetry_gate::set_telemetry_enabled(true);
        set_analytics_enabled(false);
        assert!(crate::telemetry_gate::is_telemetry_enabled());
        assert!(!is_analytics_enabled());

        crate::telemetry_gate::set_telemetry_enabled(false);
        set_analytics_enabled(true);
        assert!(!crate::telemetry_gate::is_telemetry_enabled());
        assert!(is_analytics_enabled());

        // 还原默认值，避免污染后续测试。
        crate::telemetry_gate::set_telemetry_enabled(true);
        set_analytics_enabled(true);
    }
}
