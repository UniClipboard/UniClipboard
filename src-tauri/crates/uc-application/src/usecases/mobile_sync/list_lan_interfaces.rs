//! `ListLanInterfacesUseCase` —— 给 UI / CLI 列出"可以印在二维码 URL 里给
//! iPhone 用"的本机 IPv4 地址。
//!
//! 适配器（`uc-platform`）会把它看到的全部 IPv4 接口透传上来；本 use case
//! 在应用层做一次产品策略过滤：只保留 RFC1918 私有地址、剔除 loopback 与
//! 链路本地。这样把"v1 不接受 CGNAT / Tailscale ULA"等决策固定在一处，
//! 将来开放（受 `NetworkSettings.allow_overlay_network_addrs` 控制）时只
//! 改本 use case，不改 adapter 也不改 core 域。
//!
//! 输出按"地址段"做粗排序：10.x → 172.16.x → 192.168.x —— 这是绝大多数
//! 家庭 / 办公网最优先识别的顺序，方便用户从下拉里直接挑头一个。

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

        let mut filtered: Vec<LanInterfaceOption> = raw
            .into_iter()
            .filter(is_rfc1918_lan_candidate)
            .map(|iface| LanInterfaceOption {
                name: iface.name,
                ipv4: iface.ipv4.to_string(),
            })
            .collect();

        // 网段优先级排序：10/8 → 172.16/12 → 192.168/16；段内按字符串字典序
        // 稳定。这样用户拉下拉看到的第一个候选最常是"主路由网段"。
        filtered.sort_by(|a, b| {
            rfc1918_bucket(&a.ipv4)
                .cmp(&rfc1918_bucket(&b.ipv4))
                .then_with(|| a.ipv4.cmp(&b.ipv4))
        });

        Ok(filtered)
    }
}

// ─── helpers ────────────────────────────────────────────────────────────

/// v1 的"算 LAN 候选"判定：必须是 IPv4 RFC1918 私有地址（10/8 / 172.16/12 /
/// 192.168/16），且不是 loopback。
///
/// 故意没把 100.64.0.0/10（CGNAT）、169.254.0.0/16（链路本地）、Tailscale
/// fd7a:* ULA 算进来 —— 它们是 v1 SPEC §5 排除的；将来按 `NetworkSettings`
/// 切换时改这一个函数。
fn is_rfc1918_lan_candidate(iface: &LanInterface) -> bool {
    if iface.is_loopback {
        return false;
    }
    let octets = iface.ipv4.octets();
    match octets {
        [10, _, _, _] => true,
        [172, b, _, _] if (16..=31).contains(&b) => true,
        [192, 168, _, _] => true,
        _ => false,
    }
}

/// 排序桶：10.x = 0，172.16.x = 1，192.168.x = 2，其它 = 3（理论上经
/// `is_rfc1918_lan_candidate` 过滤后不会有）。
fn rfc1918_bucket(ipv4_str: &str) -> u8 {
    if ipv4_str.starts_with("10.") {
        0
    } else if ipv4_str.starts_with("172.") {
        1
    } else if ipv4_str.starts_with("192.168.") {
        2
    } else {
        3
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
    async fn drops_non_rfc1918_addresses() {
        let uc = ListLanInterfacesUseCase::new(Arc::new(FixedProbe(vec![
            make("en1", [100, 64, 1, 5], false),  // CGNAT
            make("en2", [169, 254, 1, 5], false), // link-local
            make("en3", [8, 8, 8, 8], false),     // public
            make("en4", [172, 32, 1, 5], false),  // 172.32 不在 16-31 范围内 → 不是 RFC1918
        ])));
        let out = uc.execute().await.expect("ok");
        assert!(out.is_empty(), "non-RFC1918 must be filtered: {out:?}");
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
