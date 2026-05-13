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

/// 把业务侧 `Settings.network` 翻译为 infra 侧 `IrohNodeConfig`。
///
/// 语义反转点（**唯一**）：
/// - `allow_relay_fallback = true`  → `disable_relays = false`（允许 fallback，
///   存量行为；老用户跨网段同步仍可工作）
/// - `allow_relay_fallback = false` → `disable_relays = true`（LAN-only，跨
///   网段设备会失联）
///
/// `allow_overlay_network_addrs` 为正向同名字段，直接传递不取反。
///
/// `custom_relay_urls` 为正向同名列表，空列表表示继续使用 iroh 默认 relay；
/// 非空列表由 infra 翻译为 `RelayMode::Custom`。
///
/// 参数：
/// - `allow_relay_fallback`：业务正向语义，由 `uc-core::Settings.network` 透传
/// - `allow_overlay_network_addrs`：业务正向语义，由 `uc-core::Settings.network`
///   透传；专业用户开关，控制是否把 VPN/overlay 类虚拟网卡 IP 作为 iroh 直连候选
/// - `custom_relay_urls`：用户配置的 relay URL 列表；空列表沿用默认 relay
/// - `rendezvous_base_url`：`None` 走 `RENDEZVOUS_BASE_URL` 默认；production 调
///   用方传 `None`；集成测试覆盖 override
pub(crate) fn relay_policy_to_iroh_config(
    allow_relay_fallback: bool,
    allow_overlay_network_addrs: bool,
    custom_relay_urls: Vec<String>,
    rendezvous_base_url: Option<String>,
) -> IrohNodeConfig {
    IrohNodeConfig {
        // ↓ 全工程**唯一**取反点 — Pitfall 1 防御铁律。
        disable_relays: !allow_relay_fallback,
        // ↓ 正向同名字段，直接搬运不取反。
        allow_overlay_network_addrs,
        custom_relay_urls,
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
        let cfg = relay_policy_to_iroh_config(true, false, Vec::new(), None);
        assert!(!cfg.disable_relays, "allow=true MUST produce disable=false");
        assert!(cfg.rendezvous_base_url.is_none());
    }

    /// Pitfall 1 防御 truth-table（反向）：allow=false 必须导致 disable=true。
    #[test]
    fn allow_false_means_disable_true() {
        let cfg = relay_policy_to_iroh_config(false, false, Vec::new(), None);
        assert!(cfg.disable_relays, "allow=false MUST produce disable=true");
        assert!(cfg.rendezvous_base_url.is_none());
    }

    /// rendezvous override 透明传递（产线 None；集成测试 Some(url)）。
    #[test]
    fn rendezvous_override_passes_through() {
        let cfg = relay_policy_to_iroh_config(true, false, Vec::new(), Some("http://test".into()));
        assert_eq!(cfg.rendezvous_base_url, Some("http://test".into()));
    }

    /// allow_overlay_network_addrs 正向同名搬运（不取反）。
    #[test]
    fn overlay_addrs_true_passes_through() {
        let cfg = relay_policy_to_iroh_config(true, true, Vec::new(), None);
        assert!(cfg.allow_overlay_network_addrs);
    }

    /// allow_overlay_network_addrs=false 默认搬运。
    #[test]
    fn overlay_addrs_false_passes_through() {
        let cfg = relay_policy_to_iroh_config(true, false, Vec::new(), None);
        assert!(!cfg.allow_overlay_network_addrs);
    }

    /// 两个开关相互独立（正交）：disable_relays 由 allow_relay_fallback 决定，
    /// 与 allow_overlay_network_addrs 无关。
    #[test]
    fn switches_are_independent() {
        let cfg = relay_policy_to_iroh_config(false, true, Vec::new(), None);
        assert!(cfg.disable_relays, "LAN-only on, overlay on");
        assert!(cfg.allow_overlay_network_addrs);
    }

    /// custom_relay_urls 正向列表搬运，空列表/非空列表都不参与取反。
    #[test]
    fn custom_relay_urls_pass_through() {
        let cfg = relay_policy_to_iroh_config(
            true,
            false,
            vec!["https://relay.example.com.".to_string()],
            None,
        );
        assert_eq!(
            cfg.custom_relay_urls,
            vec!["https://relay.example.com.".to_string()]
        );
    }
}
