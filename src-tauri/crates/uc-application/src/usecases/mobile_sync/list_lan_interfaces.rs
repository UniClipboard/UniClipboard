//! `ListLanInterfacesUseCase` —— 给 UI / CLI 列出"可以印在二维码 URL 里给
//! iPhone 用"的本机 IPv4 地址。
//!
//! 适配器（`uc-platform`）会把它看到的全部 IPv4 接口透传上来；本 use case
//! 在应用层做一次产品策略过滤：
//!
//! - 接受 RFC1918 私有地址（10/8 / 172.16/12 / 192.168/16）—— 真实 LAN。
//! - 接受 CGNAT 段 100.64.0.0/10 —— Tailscale 默认 IPv4，iPhone 在同一
//!   tailnet 内可直连。这是"显示给用户"语义，与 P2P 直连候选过滤
//!   （`NetworkSettings.allow_overlay_network_addrs`）独立 ——
//!   mobile sync 是用户手动从下拉里挑 IP，不会浪费 path-validation 预算
//!   去试死路，没有跟着 overlay 开关走的理由。
//! - 剔除 loopback、链路本地 169.254/16、Clash fake-ip 198.18/15 等"看似
//!   可达实际不通"的陷阱。
//!
//! 注意：`LanInterface` 在 core 层显式只承载 IPv4，所以 Tailscale 的 IPv6
//! ULA `fd7a:115c:a1e0::/48` 暂不在本 use case 范围内 —— 要支持得先扩
//! `LanInterface` 自身。
//!
//! 输出按"地址段"做粗排序：10.x → 172.16.x → 192.168.x → 100.64.x —— 真实
//! LAN 永远优先于 overlay，方便用户从下拉里直接挑头一个。

use std::sync::Arc;

use tracing::instrument;

use uc_core::mobile_sync::LanInterface;
use uc_core::ports::{LanInterfaceProbeError, LanInterfaceProbePort};

// ─── public-shaped (output / error) ─────────────────────────────────────

/// 应用层视图：仅暴露用户决策需要的字段。
///
/// 对比 core 的 [`LanInterface`]：去掉 `is_loopback`（必为 false，已在过滤
/// 阶段排除），把 `Ipv4Addr` 直接渲染成字符串方便 UI / CLI 展示。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanInterfaceOption {
    /// 系统接口名（`en0` / `eth0` / `Wi-Fi`），用于消歧。
    pub name: String,
    /// IPv4 地址的字符串展示形式（`192.168.1.5`）。
    pub ipv4: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ListLanInterfacesError {
    #[error("lan interface probe failed: {0}")]
    ProbeFailed(String),
}

// ─── use case ───────────────────────────────────────────────────────────

pub(crate) struct ListLanInterfacesUseCase {
    probe: Arc<dyn LanInterfaceProbePort>,
}

impl ListLanInterfacesUseCase {
    pub(crate) fn new(probe: Arc<dyn LanInterfaceProbePort>) -> Self {
        Self { probe }
    }

    #[instrument(skip(self))]
    pub(crate) async fn execute(&self) -> Result<Vec<LanInterfaceOption>, ListLanInterfacesError> {
        let raw = self
            .probe
            .list_interfaces()
            .await
            .map_err(translate_probe_error)?;

        // 排序时需要数值序（"100.64" < "100.127"），而 `LanInterfaceOption`
        // 对外只保留字符串形式。保留原始 `Ipv4Addr` 作为排序 key，排完再丢。
        let mut filtered: Vec<(LanInterfaceOption, std::net::Ipv4Addr)> = raw
            .into_iter()
            .filter(is_lan_candidate)
            .map(|iface| {
                let raw_ipv4 = iface.ipv4;
                (
                    LanInterfaceOption {
                        name: iface.name,
                        ipv4: raw_ipv4.to_string(),
                    },
                    raw_ipv4,
                )
            })
            .collect();

        // 网段优先级排序：10/8 → 172.16/12 → 192.168/16 → 100.64/10；段内按
        // IPv4 数值序稳定（不能用字符串字典序，否则 CGNAT 段会出现
        // "100.127.x" < "100.64.x" 这种反直觉的顺序）。真实 LAN 永远排在
        // overlay 之前，让用户拉下拉看到的第一个候选最常是"主路由网段"。
        filtered.sort_by(|a, b| {
            lan_candidate_bucket(&a.0.ipv4)
                .cmp(&lan_candidate_bucket(&b.0.ipv4))
                .then_with(|| a.1.cmp(&b.1))
        });

        Ok(filtered.into_iter().map(|(opt, _)| opt).collect())
    }
}

// ─── helpers ────────────────────────────────────────────────────────────

/// "算 LAN 候选"判定。
///
/// 接受 IPv4 RFC1918 私有地址（10/8 / 172.16/12 / 192.168/16）与 CGNAT 段
/// 100.64.0.0/10（Tailscale 默认 IPv4 范围）。
///
/// 链路本地 169.254/16、Clash fake-ip 198.18/15 等始终排除：它们要么本身
/// 就是"看似可达实际不通"的常见陷阱来源，要么与本机 LAN 同步场景不相
/// 关。
/// `register_device.rs` 的多候选地址收集（`collect_advertise_urls`）复用本
/// 判定 —— 下拉展示与进码候选共用同一口径，避免"列表里看得到、码里却
/// 没有"的不一致。
pub(crate) fn is_lan_candidate(iface: &LanInterface) -> bool {
    if iface.is_loopback {
        return false;
    }
    let octets = iface.ipv4.octets();
    match octets {
        [10, _, _, _] => true,
        [172, b, _, _] if (16..=31).contains(&b) => true,
        [192, 168, _, _] => true,
        // 100.64.0.0/10 = 100.64.0.0 – 100.127.255.255。掩码 0xC0 = 11000000，
        // 0x40 = 01000000 → 取首字节高两位 == 01 即落入该段。
        [100, b, _, _] if (b & 0xc0) == 0x40 => true,
        _ => false,
    }
}

/// 排序桶：10.x = 0，172.16.x = 1，192.168.x = 2，100.64–127.x = 3，
/// 其它 = 4（理论上经 `is_lan_candidate` 过滤后不会有）。真实 LAN 永远
/// 排在 overlay 前面，让用户拉下拉首选项稳定。
fn lan_candidate_bucket(ipv4_str: &str) -> u8 {
    if ipv4_str.starts_with("10.") {
        0
    } else if ipv4_str.starts_with("172.") {
        1
    } else if ipv4_str.starts_with("192.168.") {
        2
    } else if ipv4_str.starts_with("100.") {
        3
    } else {
        4
    }
}

fn translate_probe_error(err: LanInterfaceProbeError) -> ListLanInterfacesError {
    match err {
        LanInterfaceProbeError::Probe(msg) => ListLanInterfacesError::ProbeFailed(msg),
    }
}

// ─── tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    use std::net::Ipv4Addr;

    use async_trait::async_trait;

    struct FixedProbe(Vec<LanInterface>);

    #[async_trait]
    impl LanInterfaceProbePort for FixedProbe {
        async fn list_interfaces(&self) -> Result<Vec<LanInterface>, LanInterfaceProbeError> {
            Ok(self.0.clone())
        }
    }

    fn make(name: &str, ip: [u8; 4], is_loopback: bool) -> LanInterface {
        LanInterface {
            name: name.into(),
            ipv4: Ipv4Addr::new(ip[0], ip[1], ip[2], ip[3]),
            is_loopback,
        }
    }

    #[tokio::test]
    async fn empty_when_probe_returns_nothing() {
        let uc = ListLanInterfacesUseCase::new(Arc::new(FixedProbe(vec![])));
        let out = uc.execute().await.expect("ok");
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn drops_loopback() {
        let uc = ListLanInterfacesUseCase::new(Arc::new(FixedProbe(vec![
            make("lo0", [127, 0, 0, 1], true),
            make("lo1", [127, 0, 0, 2], true),
        ])));
        let out = uc.execute().await.expect("ok");
        assert!(out.is_empty(), "loopback must be filtered: {out:?}");
    }

    #[tokio::test]
    async fn drops_non_candidate_addresses() {
        let uc = ListLanInterfacesUseCase::new(Arc::new(FixedProbe(vec![
            make("en2", [169, 254, 1, 5], false), // link-local
            make("en3", [8, 8, 8, 8], false),     // public
            make("en4", [172, 32, 1, 5], false),  // 172.32 不在 16-31 范围内 → 不是 RFC1918
            make("en5", [198, 18, 1, 5], false),  // Clash fake-ip
            make("en6", [100, 63, 0, 1], false),  // 100.63 不在 CGNAT 段（CGNAT 起点是 100.64）
            make("en7", [100, 128, 0, 1], false), // 100.128 已超出 CGNAT 段终点 100.127
        ])));
        let out = uc.execute().await.expect("ok");
        assert!(out.is_empty(), "non-candidate must be filtered: {out:?}");
    }

    #[tokio::test]
    async fn keeps_rfc1918_and_orders_by_bucket() {
        // 故意打乱顺序，断言 use case 按 10/8 → 172.16/12 → 192.168/16 排。
        let uc = ListLanInterfacesUseCase::new(Arc::new(FixedProbe(vec![
            make("en_a", [192, 168, 1, 5], false),
            make("en_b", [10, 0, 0, 5], false),
            make("en_c", [172, 16, 0, 5], false),
            make("en_d", [192, 168, 2, 5], false),
            make("en_e", [10, 1, 1, 1], false),
            make("en_f", [127, 0, 0, 1], true), // loopback 应被剔除
        ])));
        let out = uc.execute().await.expect("ok");
        let order: Vec<&str> = out.iter().map(|o| o.ipv4.as_str()).collect();
        assert_eq!(
            order,
            vec![
                "10.0.0.5",
                "10.1.1.1",
                "172.16.0.5",
                "192.168.1.5",
                "192.168.2.5"
            ]
        );
        assert!(out.iter().all(|o| !o.ipv4.starts_with("127.")));
    }

    #[tokio::test]
    async fn keeps_172_16_through_172_31_but_drops_outside() {
        // 边界条件：RFC1918 的 172.x 段是 172.16/12（即 172.16.0.0–172.31.255.255）。
        let uc = ListLanInterfacesUseCase::new(Arc::new(FixedProbe(vec![
            make("en_in_low", [172, 16, 0, 1], false),
            make("en_in_high", [172, 31, 255, 254], false),
            make("en_out_low", [172, 15, 255, 254], false),
            make("en_out_high", [172, 32, 0, 1], false),
        ])));
        let out = uc.execute().await.expect("ok");
        let ips: Vec<&str> = out.iter().map(|o| o.ipv4.as_str()).collect();
        assert_eq!(ips, vec!["172.16.0.1", "172.31.255.254"]);
    }

    #[tokio::test]
    async fn keeps_cgnat_for_tailscale_and_orders_after_rfc1918() {
        // 同时存在真实 LAN 与 Tailscale CGNAT 地址时，真实 LAN 必须排在前面；
        // Tailscale 地址也要出现，让用户能选。
        let uc = ListLanInterfacesUseCase::new(Arc::new(FixedProbe(vec![
            make("utun3", [100, 64, 0, 5], false), // Tailscale CGNAT 起点
            make("en1", [192, 168, 1, 200], false),
            make("utun4", [100, 127, 255, 254], false), // CGNAT 终点
        ])));
        let out = uc.execute().await.expect("ok");
        let ips: Vec<&str> = out.iter().map(|o| o.ipv4.as_str()).collect();
        assert_eq!(
            ips,
            vec!["192.168.1.200", "100.64.0.5", "100.127.255.254"],
            "real LAN must come before Tailscale CGNAT"
        );
    }

    #[tokio::test]
    async fn translates_probe_error() {
        struct ExplodingProbe;
        #[async_trait]
        impl LanInterfaceProbePort for ExplodingProbe {
            async fn list_interfaces(&self) -> Result<Vec<LanInterface>, LanInterfaceProbeError> {
                Err(LanInterfaceProbeError::Probe("ifaddr failed".into()))
            }
        }
        let uc = ListLanInterfacesUseCase::new(Arc::new(ExplodingProbe));
        let err = uc.execute().await.unwrap_err();
        assert!(
            matches!(err, ListLanInterfacesError::ProbeFailed(ref s) if s.contains("ifaddr failed")),
            "expected ProbeFailed(ifaddr failed), got {err:?}"
        );
    }
}
