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

use std::net::SocketAddr;

use uc_core::settings::model::CongestionController;

use crate::subsystem::sync_engine::IrohNodeConfig;

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
    congestion_controller: CongestionController,
    rendezvous_base_url: Option<String>,
) -> IrohNodeConfig {
    IrohNodeConfig {
        // ↓ 全工程**唯一**取反点 — Pitfall 1 防御铁律。
        disable_relays: !allow_relay_fallback,
        // ↓ 正向同名字段，直接搬运不取反。
        allow_overlay_network_addrs,
        custom_relay_urls,
        congestion_controller,
        rendezvous_base_url,
        // 直连可达性（#900）来源于 env，不在本设置翻译点决定；由
        // `apply_iroh_direct_reachability_from_env` 在 daemon / CLI 入口处填充。
        bind_port: None,
        public_addr: None,
    }
}

/// iroh 直连可达性输入（UniClipboard#900），来源于环境变量。两者默认
/// `None`（沿用现状：随机 UDP 端口、不广播任何公网地址）。
///
/// 这是给 NAT / Docker bridge / VPS 后的无头节点用的：固定 UDP 端口让运维
/// 能端口转发 / 防火墙放行一个已知端口；广播公网地址让远端桌面在 relay 关闭
/// 时仍能凭存下来的地址直连。见 `docs/architecture/adr-007-...md` §2.4。
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct IrohDirectReachability {
    pub bind_port: Option<u16>,
    pub public_addr: Option<SocketAddr>,
}

/// 纯解析两个直连可达性 env 值（保持纯函数便于单测——只吃 `Option<&str>`，
/// 不碰进程环境）。非法值**记 WARN 并回退 `None`**，不让 daemon 启动失败 ——
/// 与 `parse_clipboard_integration_mode` 同样的防御姿态。
///
/// `UC_IROH_BIND_PORT=0` 视为"随机端口"哨兵 → 忽略（要固定就给非零端口）。
pub(crate) fn parse_iroh_direct_reachability(
    bind_port_raw: Option<&str>,
    public_addr_raw: Option<&str>,
) -> IrohDirectReachability {
    let bind_port = bind_port_raw
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
        .and_then(|raw| match raw.parse::<u16>() {
            Ok(0) => {
                tracing::warn!(
                    uc_iroh_bind_port = %raw,
                    "UC_IROH_BIND_PORT=0 is the ephemeral-port sentinel; ignoring (use a non-zero fixed port)",
                );
                None
            }
            Ok(port) => Some(port),
            Err(err) => {
                tracing::warn!(
                    uc_iroh_bind_port = %raw,
                    error = %err,
                    "invalid UC_IROH_BIND_PORT; ignoring (expected an integer 1..=65535)",
                );
                None
            }
        });

    let public_addr = public_addr_raw
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
        .and_then(|raw| match raw.parse::<SocketAddr>() {
            Ok(addr) => Some(addr),
            Err(err) => {
                tracing::warn!(
                    uc_iroh_public_addr = %raw,
                    error = %err,
                    "invalid UC_IROH_PUBLIC_ADDR; ignoring (expected ip:port, e.g. 203.0.113.7:51820)",
                );
                None
            }
        });

    IrohDirectReachability {
        bind_port,
        public_addr,
    }
}

/// 读取直连可达性 env 并写入 `cfg`。在每个 production daemon / CLI 入口
/// （`build_daemon_lifecycle` / `build_cli_app_runtime`）紧跟
/// `relay_policy_to_iroh_config` 之后调用——daemon 与 CLI 配对路径都要广播
/// 同一个公网地址。让 `relay_policy_to_iroh_config` 保持纯（只吃 settings、
/// 不碰 env）。
pub(crate) fn apply_iroh_direct_reachability_from_env(cfg: &mut IrohNodeConfig) {
    let reach = parse_iroh_direct_reachability(
        std::env::var("UC_IROH_BIND_PORT").ok().as_deref(),
        std::env::var("UC_IROH_PUBLIC_ADDR").ok().as_deref(),
    );
    if reach.bind_port.is_some() || reach.public_addr.is_some() {
        tracing::info!(
            target: "settings.network",
            bind_port = ?reach.bind_port,
            public_addr = ?reach.public_addr,
            "iroh direct-reachability configured from env (UC_IROH_BIND_PORT / UC_IROH_PUBLIC_ADDR)",
        );
    }
    cfg.bind_port = reach.bind_port;
    cfg.public_addr = reach.public_addr;
}

/// Override the congestion controller from `UC_CONGESTION_CONTROLLER` env.
/// Invalid values are logged and ignored (keeps the settings-derived default).
pub(crate) fn apply_congestion_controller_from_env(cfg: &mut IrohNodeConfig) {
    if let Ok(raw) = std::env::var("UC_CONGESTION_CONTROLLER") {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return;
        }
        match trimmed.parse::<CongestionController>() {
            Ok(cc) => {
                tracing::info!(
                    target: "settings.network",
                    congestion_controller = %cc,
                    "congestion controller overridden from env (UC_CONGESTION_CONTROLLER)",
                );
                cfg.congestion_controller = cc;
            }
            Err(err) => {
                tracing::warn!(
                    uc_congestion_controller = %raw,
                    error = %err,
                    "invalid UC_CONGESTION_CONTROLLER; ignoring (expected cubic or bbr3)",
                );
            }
        }
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
        let cfg = relay_policy_to_iroh_config(
            true,
            false,
            Vec::new(),
            CongestionController::default(),
            None,
        );
        assert!(!cfg.disable_relays, "allow=true MUST produce disable=false");
        assert!(cfg.rendezvous_base_url.is_none());
    }

    /// Pitfall 1 防御 truth-table（反向）：allow=false 必须导致 disable=true。
    #[test]
    fn allow_false_means_disable_true() {
        let cfg = relay_policy_to_iroh_config(
            false,
            false,
            Vec::new(),
            CongestionController::default(),
            None,
        );
        assert!(cfg.disable_relays, "allow=false MUST produce disable=true");
        assert!(cfg.rendezvous_base_url.is_none());
    }

    /// rendezvous override 透明传递（产线 None；集成测试 Some(url)）。
    #[test]
    fn rendezvous_override_passes_through() {
        let cfg = relay_policy_to_iroh_config(
            true,
            false,
            Vec::new(),
            CongestionController::default(),
            Some("http://test".into()),
        );
        assert_eq!(cfg.rendezvous_base_url, Some("http://test".into()));
    }

    /// allow_overlay_network_addrs 正向同名搬运（不取反）。
    #[test]
    fn overlay_addrs_true_passes_through() {
        let cfg = relay_policy_to_iroh_config(
            true,
            true,
            Vec::new(),
            CongestionController::default(),
            None,
        );
        assert!(cfg.allow_overlay_network_addrs);
    }

    /// allow_overlay_network_addrs=false 默认搬运。
    #[test]
    fn overlay_addrs_false_passes_through() {
        let cfg = relay_policy_to_iroh_config(
            true,
            false,
            Vec::new(),
            CongestionController::default(),
            None,
        );
        assert!(!cfg.allow_overlay_network_addrs);
    }

    /// 两个开关相互独立（正交）：disable_relays 由 allow_relay_fallback 决定，
    /// 与 allow_overlay_network_addrs 无关。
    #[test]
    fn switches_are_independent() {
        let cfg = relay_policy_to_iroh_config(
            false,
            true,
            Vec::new(),
            CongestionController::default(),
            None,
        );
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
            CongestionController::default(),
            None,
        );
        assert_eq!(
            cfg.custom_relay_urls,
            vec!["https://relay.example.com.".to_string()]
        );
    }

    /// settings 翻译点不负责直连可达性——两个字段恒为 None，由 env applier 填充。
    #[test]
    fn relay_policy_leaves_direct_reachability_unset() {
        let cfg = relay_policy_to_iroh_config(
            true,
            false,
            Vec::new(),
            CongestionController::default(),
            None,
        );
        assert_eq!(cfg.bind_port, None);
        assert_eq!(cfg.public_addr, None);
    }

    /// #900：两个 env 都未设置 → 沿用现状（None/None）。
    #[test]
    fn direct_reachability_unset_is_none() {
        let reach = parse_iroh_direct_reachability(None, None);
        assert_eq!(reach, IrohDirectReachability::default());
    }

    /// #900：合法端口 + 合法地址 → Some/Some。
    #[test]
    fn direct_reachability_valid_values_parse() {
        let reach = parse_iroh_direct_reachability(Some("51820"), Some("203.0.113.7:51820"));
        assert_eq!(reach.bind_port, Some(51820));
        assert_eq!(
            reach.public_addr,
            Some("203.0.113.7:51820".parse::<SocketAddr>().unwrap())
        );
    }

    /// #900：前后空白被裁剪，仍能解析。
    #[test]
    fn direct_reachability_trims_whitespace() {
        let reach = parse_iroh_direct_reachability(Some("  51820 "), Some(" 203.0.113.7:51820 "));
        assert_eq!(reach.bind_port, Some(51820));
        assert_eq!(
            reach.public_addr,
            Some("203.0.113.7:51820".parse::<SocketAddr>().unwrap())
        );
    }

    /// #900：空字符串等价于未设置。
    #[test]
    fn direct_reachability_empty_is_none() {
        let reach = parse_iroh_direct_reachability(Some("   "), Some(""));
        assert_eq!(reach.bind_port, None);
        assert_eq!(reach.public_addr, None);
    }

    /// #900：端口 0 是随机端口哨兵 → 忽略（回退 None）。
    #[test]
    fn direct_reachability_port_zero_ignored() {
        let reach = parse_iroh_direct_reachability(Some("0"), None);
        assert_eq!(reach.bind_port, None);
    }

    /// #900：非法值 WARN 后回退 None，不让 daemon 启动失败。
    #[test]
    fn direct_reachability_invalid_values_fall_back_to_none() {
        let reach = parse_iroh_direct_reachability(Some("abc"), Some("not-an-addr"));
        assert_eq!(reach.bind_port, None);
        assert_eq!(reach.public_addr, None);

        // 端口超出 u16 范围同样回退。
        let overflow = parse_iroh_direct_reachability(Some("70000"), None);
        assert_eq!(overflow.bind_port, None);

        // 缺少端口的裸 IP 不是合法 SocketAddr → 回退。
        let no_port = parse_iroh_direct_reachability(None, Some("203.0.113.7"));
        assert_eq!(no_port.public_addr, None);
    }
}
