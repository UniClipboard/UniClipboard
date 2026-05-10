//! LAN-only Mode bind-time 行为断言（Tier B 自动化覆盖）：
//!
//! 验证 `IrohNodeConfig.disable_relays` 通过 iroh
//! `Endpoint::builder().relay_mode(...).bind()` 路径正确翻译为
//! `RelayMode::Disabled` / `RelayMode::Default`，效果体现在
//! `endpoint.addr().addrs` 是否含 `TransportAddr::Relay(_)` 项。
//!
//! ## 测试分层（Pitfall 8）
//!
//! - **Tier A（unit）**：`uc-bootstrap/src/network_policy.rs` truth-table 单测
//!   覆盖配置传递（plan 05 task 1）
//! - **Tier B（integration — 本文件）**：bind 后断 endpoint 的候选地址中是否
//!   含 Relay 项；强不等式覆盖 Disabled，弱不等式覆盖 Default
//! - **Tier C（manual）**：跨平台 Wireshark / tcpdump 抓包验证（D-C1 锁定为
//!   手工流程，不在本 phase 自动化）
//!
//! ## 反向用例约束（D-C1 / PATTERNS.md §11 critical finding 3）
//!
//! `RelayMode::Default` 不一定立刻发布 Relay 地址（取决于 iroh 与公网 relay
//! mesh 的连通性，CI 环境可能没有公网或被 firewall 限制），所以反向断言
//! 只断"bind 不 panic"这一条**弱**不等式；具体 Relay 候选行为留给 Tier C
//! 抓包验证。
//!
//! 见：`.planning/research/PITFALLS.md` Pitfall 8 + 094-CONTEXT.md D-C1。

use std::time::Duration;

use iroh::address_lookup::mdns::MdnsAddressLookup;
use iroh::{Endpoint, RelayMode, TransportAddr};

const TEST_ALPN: &[u8] = b"uniclipboard/lan-only-test/0";

/// loopback bind helper —— 与 `iroh_presence_probe.rs:17-29` 同模式。
async fn bind_with_relay_mode(mode: RelayMode) -> Endpoint {
    Endpoint::builder(iroh::endpoint::presets::N0)
        .alpns(vec![TEST_ALPN.to_vec()])
        .relay_mode(mode)
        .bind()
        .await
        .expect("bind endpoint")
}

/// LAN-only 生产链路 fixture —— 镜像 `uc-infra/src/network/iroh/node.rs`
/// `IrohNodeBuilder::bind` 在 `disable_relays = true` 时的 builder 形状：
/// 从 `presets::N0` 出发，先 `clear_address_lookup()` 清掉 pkarr/DNS，再
/// 单挂 mDNS。用来验证"LAN-only 路径下 address_lookup 只剩 mDNS"这条
/// 防回归不变量（Pitfall 5 防御）。
async fn bind_lan_only_production_shape() -> Endpoint {
    Endpoint::builder(iroh::endpoint::presets::N0)
        .alpns(vec![TEST_ALPN.to_vec()])
        .relay_mode(RelayMode::Disabled)
        .clear_address_lookup()
        .address_lookup(MdnsAddressLookup::builder())
        .bind()
        .await
        .expect("bind endpoint")
}

/// 等候 endpoint 发布候选地址（loopback 通常 < 100ms；与 sibling fixture 同
/// 重试策略）。**不 panic** —— 即使没有任何候选也允许后续断言；本 helper 只
/// 是给 magicsock 一点时间枚举接口。
async fn wait_for_addrs(endpoint: &Endpoint) {
    for _ in 0..100 {
        if !endpoint.addr().addrs.is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// D-C1 用例 (a) — 强不等式：`RelayMode::Disabled` bind 后候选地址中
/// **不应**含 `TransportAddr::Relay` 项。这是 LAN-only Mode 在 endpoint 层面
/// 的可观察事实（与 ROADMAP success criterion #1 对齐）。
#[tokio::test]
async fn relay_disabled_publishes_no_relay_addrs() {
    let endpoint = bind_with_relay_mode(RelayMode::Disabled).await;
    wait_for_addrs(&endpoint).await;

    let addrs = endpoint.addr().addrs.clone();
    let has_relay = addrs.iter().any(|a| matches!(a, TransportAddr::Relay(_)));

    assert!(
        !has_relay,
        "RelayMode::Disabled MUST NOT publish Relay addrs; got: {:?}",
        addrs
    );

    endpoint.close().await;
}

/// D-C1 用例 (b) — 弱不等式：`RelayMode::Default` bind 不应 panic / 抛错。
/// 不强断"必须含 Relay 候选"（CI 环境 / Relay mesh 不可靠）；这是 PATTERNS.md
/// §11 critical finding 3 与 D-C1 共同确认的 Tier B 边界。
///
/// 真正"Relay 路径活跃"的验证留给 Tier C 手工抓包（D-C1）。
#[tokio::test]
async fn relay_default_binds_without_panic() {
    let endpoint = bind_with_relay_mode(RelayMode::Default).await;
    wait_for_addrs(&endpoint).await;

    // 【checker WARNING 4】显式探一下 addr().addrs 字段以确认 endpoint 处于
    // 可读状态（不 panic 即可），但**不**对内容做断言 —— 这就是 Tier B 弱不
    // 等式的设计：bind 成功 + endpoint 状态可读 = 通过；具体 Relay/Direct
    // 候选行为留给 Tier C 抓包（D-C1）。
    let _addrs = endpoint.addr().addrs;

    endpoint.close().await;
}

/// Pitfall 5 防回归 — 强不等式：LAN-only 生产链路下 `address_lookup`
/// 只剩 1 个 service（mDNS）。
///
/// `presets::N0` 默认会注入 `PkarrPublisher` + `DnsAddressLookup`；
/// `IrohNodeBuilder::bind` 在 `disable_relays = true` 时通过
/// `clear_address_lookup()` 清掉它们再单挂 `MdnsAddressLookup`。任何
/// 后续修改如果不小心保留了 N0 默认的 lookup（比如把 `clear_address_lookup`
/// 删了），本断言立即翻车 —— 哪怕日志看起来"正常"。
///
/// 与 [`relay_default_publishes_three_address_lookup_services`] 形成对比测试，
/// 锁定"3 个（默认）↔ 1 个（LAN-only）"这条结构差。
#[tokio::test]
async fn lan_only_publishes_only_mdns_address_lookup() {
    let endpoint = bind_lan_only_production_shape().await;

    let services = endpoint.address_lookup().expect("endpoint not closed");
    assert_eq!(
        services.len(),
        1,
        "LAN-only path MUST register exactly 1 address lookup (mDNS); \
         saw {} — did clear_address_lookup() get removed from IrohNodeBuilder::bind?",
        services.len(),
    );

    endpoint.close().await;
}

/// 对照测试：`presets::N0` + 后挂 mDNS 的"默认"链路应注册 3 个 service
/// （`PkarrPublisher` + `DnsAddressLookup` + `MdnsAddressLookup`）。
/// 与 [`lan_only_publishes_only_mdns_address_lookup`] 共同锁定结构差。
#[tokio::test]
async fn relay_default_publishes_three_address_lookup_services() {
    let endpoint = Endpoint::builder(iroh::endpoint::presets::N0)
        .alpns(vec![TEST_ALPN.to_vec()])
        .relay_mode(RelayMode::Default)
        .address_lookup(MdnsAddressLookup::builder())
        .bind()
        .await
        .expect("bind endpoint");

    let services = endpoint.address_lookup().expect("endpoint not closed");
    assert_eq!(
        services.len(),
        3,
        "Default path MUST register 3 address lookups (pkarr publisher + dns + mdns); \
         saw {} — `presets::N0` injection contract changed?",
        services.len(),
    );

    endpoint.close().await;
}
