//! 单一翻译点：`network.allow_relay_fallback`（业务正向语义）→
//! `IrohNodeConfig.disable_relays`（infra 反向语义）。
//!
//! ## 反向命名铁律（Pitfall 1 防御 — 见 .planning/research/PITFALLS.md §Pitfall 1）
//!
//! - UI = "LAN-only Mode = ON"
//! - 后端 = `network.allow_relay_fallback = false`
//! - infra (iroh) = `IrohNodeConfig.disable_relays = true`
//!
//! 三层语义两次反转。**全工程除本模块 + `uc-infra/src/network/iroh/node.rs:153-162`
//! 字段定义 + 测试文件外，严禁在其他位置出现 `disable_relays = !allow_relay_fallback`
//! 类的取反**。DTO ↔ View ↔ core 三层只搬运 `allow_relay_fallback` 业务正向语义。
//!
//! ## OTLP 不联动（Pitfall 6 防御）
//!
//! 本模块**禁止**引用 `general.telemetry_enabled` 或任何 OTLP 配置 ——
//! 网络策略与遥测开关独立。
//!
//! ## D-A1 物理位置 + D-B3 启动日志规范
//!
//! 见 `.planning/phases/094-backend-network-allow-relay-fallback/094-CONTEXT.md`。

use crate::space_setup::IrohNodeConfig;

/// 把业务侧 `network.allow_relay_fallback` 翻译为 infra 侧 `IrohNodeConfig`。
///
/// 语义反转点（**唯一**）：
/// - `allow_relay_fallback = true`  → `disable_relays = false`（允许 fallback，
///   存量行为；老用户跨网段同步仍可工作）
/// - `allow_relay_fallback = false` → `disable_relays = true`（LAN-only，跨
///   网段设备会失联）
///
/// 参数：
/// - `allow_relay_fallback`：业务正向语义，由 `uc-core::Settings.network` 透传
/// - `rendezvous_base_url`：`None` 走 `RENDEZVOUS_BASE_URL` 默认；production 调
///   用方传 `None`；集成测试覆盖 override
pub(crate) fn relay_policy_to_iroh_config(
    allow_relay_fallback: bool,
    rendezvous_base_url: Option<String>,
) -> IrohNodeConfig {
    IrohNodeConfig {
        // ↓ 全工程**唯一**取反点 — Pitfall 1 防御铁律。
        disable_relays: !allow_relay_fallback,
        rendezvous_base_url,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pitfall 1 防御 truth-table（正向）：allow=true 必须导致 disable=false。
    /// 这与下面 `allow_false_means_disable_true` 两个测试**不能合并** —
    /// 单方向断言无法捕捉"代码错把恒等当取反"或"反向写漏"两类 bug。
    #[test]
    fn allow_true_means_disable_false() {
        let cfg = relay_policy_to_iroh_config(true, None);
        assert!(!cfg.disable_relays, "allow=true MUST produce disable=false");
        assert!(cfg.rendezvous_base_url.is_none());
    }

    /// Pitfall 1 防御 truth-table（反向）：allow=false 必须导致 disable=true。
    #[test]
    fn allow_false_means_disable_true() {
        let cfg = relay_policy_to_iroh_config(false, None);
        assert!(cfg.disable_relays, "allow=false MUST produce disable=true");
        assert!(cfg.rendezvous_base_url.is_none());
    }

    /// rendezvous override 透明传递（产线 None；集成测试 Some(url)）。
    #[test]
    fn rendezvous_override_passes_through() {
        let cfg = relay_policy_to_iroh_config(true, Some("http://test".into()));
        assert_eq!(cfg.rendezvous_base_url, Some("http://test".into()));
    }
}
