//! 本机 LAN 网卡的领域视图。
//!
//! 仅描述"daemon 自我观察到的本机 IPv4 地址"——故意不收 IPv6：iPhone 的
//! Apple Shortcuts HTTP 请求在 LAN 场景几乎都走 IPv4，IPv6 ULA / 链路本地
//! 地址在配置上反而是常见的"看起来连得上但实际不可达"陷阱来源；v1 把它
//! 整个排除，简化二维码 URL 与排错心智。
//!
//! 该类型与平台细节解耦：adapter 在 `uc-platform` 用 `network-interface`
//! crate 探测，但本类型不携带 mac 地址、index、prefix 长度等运维信息——
//! 那些在 v1 没有用户可感知的对应行为，按 `uc-core/AGENTS.md` §4.2 留在
//! 实现细节里即可。

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

/// 本机一张网卡上的一条 IPv4 配置。
///
/// 一张物理网卡可能对应多条 `LanInterface`（多个 IPv4 alias），调用方按
/// `name + ipv4` 去重即可。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LanInterface {
    /// 系统给出的接口名（macOS: `en0` / Linux: `eth0` / Windows:
    /// `Ethernet`），用于 UI 让用户区分"哪张网卡的地址"。
    pub name: String,
    /// 当前 IPv4 地址。
    pub ipv4: Ipv4Addr,
    /// 是否是回环（127.0.0.0/8）。回环地址显然不能给 iPhone 用，但适配器
    /// 仍把它包含进列表 —— 过滤逻辑由 application 层 use case 决定，便于
    /// 单元测试穷举。
    pub is_loopback: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lan_interface_round_trips_through_serde() {
        let iface = LanInterface {
            name: "en0".into(),
            ipv4: Ipv4Addr::new(192, 168, 1, 5),
            is_loopback: false,
        };
        let json = serde_json::to_string(&iface).unwrap();
        let back: LanInterface = serde_json::from_str(&json).unwrap();
        assert_eq!(iface, back);
    }
}
