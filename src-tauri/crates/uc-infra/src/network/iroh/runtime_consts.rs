//! 进程级 iroh 运行时常量。
//!
//! 本模块承载"启动时设置一次、终生不变"的状态 —— 与 Rust `const` 的
//! 编译期常量不同，这些值在 daemon 启动后由 `IrohNodeBuilder::bind`
//! 注入，但**注入完成后等同于常量**：再无任何路径会改写它们，全工程
//! 任何地方读到的都是同一个值。
//!
//! ## 当前承载的常量
//!
//! - [`LAN_ONLY`] —— 当前进程是否处于 LAN-only Mode（即 iroh
//!   `RelayMode::Disabled` + 清掉 pkarr/DNS lookup）。`connect.rs` 在
//!   每次 dial 前读这个值，决定要不要从对端 `EndpointAddr` 中剥掉
//!   `TransportAddr::Relay` 项 —— 否则 iroh 仍会走对端发布的 relay
//!   完成中转（详见 `connect::strip_relay_if_lan_only`）。
//!
//! ## 双契约（与 `node.rs::BIND_LOCK` 同款）
//!
//! - **Production build**（默认 — 无 `test-util` feature 且非
//!   `cfg(test)`）：`OnceLock` 守护激活，`install_lan_only` 进程级
//!   single-shot；二次设置无声忽略（理论上不会发生，因为 `BIND_LOCK`
//!   已经强制单 bind）。
//! - **Test build (`cfg(test)`)** 与下游 crate 启用 `uc-infra/test-util`
//!   feature 时：`install_lan_only` no-op，`lan_only()` 永远返回 `false`。
//!   测试要验证 LAN-only 行为请直接调 [`strip_relay_if_lan_only`]
//!   并显式传 `true`，与 `lan_only_relay_mode.rs` Tier B 同款模式。
//!
//! [`strip_relay_if_lan_only`]: super::connect::strip_relay_if_lan_only

#[cfg(not(any(test, feature = "test-util")))]
use std::sync::OnceLock;

#[cfg(not(any(test, feature = "test-util")))]
static LAN_ONLY: OnceLock<bool> = OnceLock::new();

/// 由 [`super::node::IrohNodeBuilder::bind`] 在 production 路径调一次，
/// 把当前的 LAN-only 状态固化到进程常量。test/test-util build 下 no-op。
pub(super) fn install_lan_only(lan_only: bool) {
    #[cfg(not(any(test, feature = "test-util")))]
    {
        let _ = LAN_ONLY.set(lan_only);
    }
    #[cfg(any(test, feature = "test-util"))]
    {
        let _ = lan_only;
    }
}

/// 当前进程是否处于 LAN-only Mode。production 默认 `false`（在
/// `install_lan_only` 之前调用也安全）；test/test-util build 永远返回
/// `false`。
pub(super) fn lan_only() -> bool {
    #[cfg(not(any(test, feature = "test-util")))]
    {
        LAN_ONLY.get().copied().unwrap_or(false)
    }
    #[cfg(any(test, feature = "test-util"))]
    {
        false
    }
}
