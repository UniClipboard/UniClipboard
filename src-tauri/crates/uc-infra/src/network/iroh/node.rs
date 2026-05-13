//! Process-wide iroh node shared by every Slice 1+ transport.
//!
//! A single [`iroh::Endpoint`] per process owns the Ed25519 identity, the
//! UDP socket, and the NAT-traversal / relay state. Every business
//! transport (pairing, clipboard sync, blob transfer) registers its own
//! ALPN on the same [`iroh::protocol::Router`] instead of binding a new
//! endpoint — see `uc-infra/AGENTS.md` §4.2 (technical detail stays
//! contained) and the Slice 1 decision log on shared endpoint ownership.
//!
//! The builder pattern is deliberate: each `install_*` method is where a
//! new transport slices in. Slice 1 ships [`install_pairing`]; Slice 2
//! Phase 1 adds [`install_presence`]; Slice 2 Phase 2 adds
//! [`install_clipboard`]; Slice 3 will add `install_blobs` on the same
//! builder.
//!
//! [`install_pairing`]: IrohNodeBuilder::install_pairing
//! [`install_presence`]: IrohNodeBuilder::install_presence
//! [`install_clipboard`]: IrohNodeBuilder::install_clipboard

#[cfg(not(any(test, feature = "test-util")))]
use std::sync::OnceLock;
use std::{path::PathBuf, sync::Arc, time::Duration};

use iroh::address_lookup::mdns::MdnsAddressLookup;
use iroh::address_lookup::AddrFilter;
use iroh::endpoint::{presets, QuicTransportConfig, VarInt};
use iroh::protocol::{Router, RouterBuilder};
use iroh::{Endpoint, RelayMode, RelayUrl, TransportAddr};
use noq_proto::congestion::BbrConfig;
use tracing::{debug, info, instrument, warn};

use uc_core::file_transfer::OutboundProgressReporterPort;
use uc_core::membership::MemberRepositoryPort;
use uc_core::ports::blob::BlobTransferPort;
use uc_core::ports::pairing::{PairingEventPort, PairingSessionPort};
use uc_core::ports::pairing_invitation::{
    PairingInvitationAddressQueryPort, PairingInvitationByAddressPort, PairingInvitationPort,
};
use uc_core::ports::security::IdentityFingerprintFactoryPort;
use uc_core::ports::{
    ClipboardDispatchPort, ClipboardReceiverPort, ClockPort, ConnectionChannelPort,
    DeviceIdentityPort, LocalIdentityError, PeerAddressRepositoryPort, PresencePort, SettingsPort,
};

use crate::pairing::{IrohPairingSessionAdapter, PAIRING_ALPN};
use crate::rendezvous::{RendezvousClient, RendezvousPairingInvitationAdapter};

use super::addr_filter::apply_addr_filter;
use super::blobs::{IrohBlobTransferAdapter, BLOBS_ALPN};
use super::clipboard_dispatch_adapter::{IrohClipboardDispatchAdapter, CLIPBOARD_ALPN};
use super::clipboard_receiver_adapter::IrohClipboardReceiverAdapter;
use super::connection_channel_adapter::IrohConnectionChannelAdapter;
use super::identity_store::IrohIdentityStore;
use super::presence_adapter::{IrohPresenceAdapter, PRESENCE_ALPN};
use super::transfer_progress_adapter::{
    InboundProgressEvent, IrohTransferProgressAdapter, TRANSFER_PROGRESS_ALPN,
};

/// The pairing ports produced by [`IrohNodeBuilder::install_pairing`].
///
/// `session` and `events` share the same underlying
/// [`IrohPairingSessionAdapter`] — both trait objects point at one Arc so
/// sponsor-side inbound events and the outbound dial/send path use the same
/// session map. `invitation`, `invitation_addresses`, and
/// `invitation_by_address` are three trait-object views over a single
/// rendezvous HTTP adapter (they read the same endpoint's
/// [`iroh::EndpointAddr`] — the split is purely a port-surface CQS
/// concern, not a runtime cost).
pub struct PairingHandlers {
    pub session: Arc<dyn PairingSessionPort>,
    pub events: Arc<dyn PairingEventPort>,
    pub invitation: Arc<dyn PairingInvitationPort>,
    pub invitation_addresses: Arc<dyn PairingInvitationAddressQueryPort>,
    /// Dev-only: issue an invitation pinned to a single local IP. Not
    /// part of the standard sponsor lifecycle — wired into the CLI's
    /// hidden `dev pairing` subcommand for multi-NIC diagnosis.
    pub invitation_by_address: Arc<dyn PairingInvitationByAddressPort>,
}

/// The two clipboard ports produced by [`IrohNodeBuilder::install_clipboard`].
///
/// `dispatch` opens a fresh bi-stream per message; `receiver` exposes the
/// broadcast of inbound payloads. Both share the endpoint — the receiver
/// handler is also registered under [`CLIPBOARD_ALPN`] on the same
/// `RouterBuilder`.
pub struct ClipboardHandlers {
    pub dispatch: Arc<dyn ClipboardDispatchPort>,
    pub receiver: Arc<dyn ClipboardReceiverPort>,
}

/// [`IrohNodeBuilder::install_blobs`] 产出的 blob port。
pub struct BlobHandlers {
    pub blob_transfer: Arc<dyn BlobTransferPort>,
}

/// [`IrohNodeBuilder::install_transfer_progress`] 产出的进度反向通道句柄。
///
/// `reporter` 给接收端 fetch sink 用,把字节进度推回 sender;
/// `inbound_events` 给应用层 worker 订阅,翻译为前端 host event。
/// 同一进程同时持两个角色 —— 谁是 sender / receiver 由当次传输方向决定。
pub struct TransferProgressHandlers {
    pub reporter: Arc<dyn OutboundProgressReporterPort>,
    pub inbound_events: tokio::sync::broadcast::Receiver<InboundProgressEvent>,
}

/// Live iroh node with a spawned [`Router`].
///
/// Owns the [`Router`] so shutdown runs through a single call site; Slice 2 /
/// 3 add handlers by extending [`IrohNodeBuilder`], not by adding shutdown
/// paths here.
pub struct IrohNode {
    endpoint: Arc<Endpoint>,
    router: Router,
}

impl IrohNode {
    /// 优雅关闭 iroh 节点。两步序列均为信号驱动,不再用外层 timeout 与 iroh
    /// 内部状态机 race。
    ///
    /// 1. [`Endpoint::close`] —— 显式跑完 iroh 自带的关闭状态机:cancel
    ///    `at_close_start` token、`address_lookup().clear()` 同步停掉 mDNS /
    ///    pkarr 子任务、发送 QUIC `CONNECTION_CLOSE`、`wait_idle` 等 ack
    ///    (自带 ~3s probe timeout)、cancel actors、shutdown runtime。**整条
    ///    链路本身就是有界的**;再叠一层更短的外层 timeout 只会把 ack 阶段
    ///    从"事件驱动地等到 OK 或自然超时"退化成"中途砍断 → `EndpointInner::drop`
    ///    走 ungraceful abort 喷 ERROR + 留下 mDNS 残留任务"。
    /// 2. [`Router::shutdown`] —— endpoint 已 closed 后,router 的 accept loop
    ///    在 `endpoint.accept()` 处自然返回 None 退出,这一步主要是 join 已
    ///    spawn 的 protocol handler shutdown(例如 iroh-blobs 的 store 关闭)
    ///    并 abort 残留 accept 任务。理应很快,但保留一个比 endpoint.close
    ///    自带预算更大的 watchdog 兜底已知 upstream bug n0-computer/iroh#3875
    ///    (router task 偶发不返回);触发时 endpoint 已 closed,task drop 不会
    ///    再喷 socket ERROR。
    ///
    /// 上层 GUI 退出路径再有 `DAEMON_SHUTDOWN_TIMEOUT = 15s` 兜底,所以这里不
    /// 需要也不应该用激进的硬截断。
    #[instrument(skip_all)]
    pub async fn shutdown(self) {
        // Step 1:跑完 iroh 自带的事件驱动关闭。无外层 timeout —— iroh 内部
        // 已经层层有界(详见上面 doc)。
        self.endpoint.close().await;

        // Step 2:join router cleanup。watchdog 仅用于规避 iroh#3875。
        const ROUTER_WATCHDOG: Duration = Duration::from_secs(5);
        match tokio::time::timeout(ROUTER_WATCHDOG, self.router.shutdown()).await {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                tracing::warn!(error = %err, "iroh router task joined with error");
            }
            Err(_) => {
                tracing::warn!(
                    budget_ms = ROUTER_WATCHDOG.as_millis() as u64,
                    "iroh router shutdown didn't return in budget (iroh#3875 watchdog tripped); endpoint already closed so socket cleanup is safe",
                );
            }
        }
        debug!("iroh node shut down");
    }
}

/// Bootstrap-time configuration for [`IrohNodeBuilder`]. Defaults cover
/// production; integration tests override the rendezvous URL (pointing at
/// a mock server) and usually disable relays (loopback-only handshake).
#[derive(Debug, Clone, Default)]
pub struct IrohNodeConfig {
    /// Override rendezvous base URL. `None` → use
    /// [`crate::rendezvous::RENDEZVOUS_BASE_URL`].
    pub rendezvous_base_url: Option<String>,
    /// If true, bind the endpoint in **strict LAN-only mode**:
    ///
    /// 1. `RelayMode::Disabled` —— 不连任何 iroh relay；
    /// 2. **同时清掉 `presets::N0` 默认注入的 `PkarrPublisher` /
    ///    `DnsAddressLookup`**，避免向 `dns.iroh.link` 发布 endpoint info 或
    ///    通过 n0 公共 DNS 解析对端 NodeId。LAN-only 用户的合理预期是"只靠
    ///    mDNS 在局域网内发现对方"，而不是"relay 关掉但仍向公共 DNS 广播
    ///    自己的 NodeId"（见 `.planning/research/PITFALLS.md` Pitfall 5）。
    ///
    /// 副作用（**这是 LAN-only 的设计意图，不是 bug**）：
    /// - 跨网段已配对设备无法被发现（关 relay 已经如此，这里把发现路径也
    ///   收紧到 mDNS 同子网）；
    /// - 没有 mDNS 的环境（部分 Docker / NAS / 未装 avahi 的 Linux）会完全
    ///   不可达，这是用户启用 LAN-only 时承担的取舍。
    ///
    /// 集成测试也走 `disable_relays = true` 路径（loopback 走 mDNS 或显式
    /// `EndpointAddr` 注入）。production `false` 保持完整 N0 行为
    /// （pkarr publish + DNS lookup + relay fallback）。
    pub disable_relays: bool,
    /// 用户配置的 iroh relay URL 列表。空列表表示沿用 iroh 默认 relay；
    /// 非空时在 `disable_relays = false` 路径下翻译为 `RelayMode::Custom`。
    /// `disable_relays = true` 时该列表被保留但不参与 endpoint bind。
    pub custom_relay_urls: Vec<String>,
    /// If true, allow VPN / overlay-network virtual NIC addresses (CGNAT
    /// `100.64.0.0/10`, Tailscale ULA `fd7a:115c:a1e0::/48`) to flow through
    /// the address filter as direct-connection candidates. Default `false`
    /// (drop them). Power users set this when both peers share an overlay
    /// network and want iroh to leverage that path.
    ///
    /// Clash fake-ip (`198.18.0.0/15`) and IPv4 link-local (`169.254.0.0/16`)
    /// are unconditionally filtered regardless of this flag — they have no
    /// legitimate cross-host use case.
    pub allow_overlay_network_addrs: bool,
}

/// Snapshot the candidate set this endpoint is currently advertising and
/// log it at INFO. We compare these IPs against `connect.rs`'s
/// `selected path` log to spot when magicsock is publishing virtual-NIC
/// addresses (Clash TUN, WireGuard, Tailscale) that skew remote candidate
/// races. Two snapshots are taken — `post-bind` (just after the UDP
/// socket comes up) and `post-spawn` (after `install_*` finish, which
/// gives magicsock more time to enumerate interfaces). Refs
/// UniClipboard#486.
fn log_publish_addrs(endpoint: &Endpoint, stage: &'static str) {
    let addr = endpoint.addr();
    let ip_addrs: Vec<String> = addr
        .addrs
        .iter()
        .filter_map(|a| match a {
            TransportAddr::Ip(s) => Some(s.to_string()),
            _ => None,
        })
        .collect();
    let relay_urls: Vec<String> = addr
        .addrs
        .iter()
        .filter_map(|a| match a {
            TransportAddr::Relay(u) => Some(u.to_string()),
            _ => None,
        })
        .collect();
    info!(
        stage,
        endpoint_id = %endpoint.id().fmt_short(),
        ip_addr_count = ip_addrs.len(),
        relay_url_count = relay_urls.len(),
        ip_addrs = ?ip_addrs,
        relay_urls = ?relay_urls,
        "iroh endpoint publish snapshot (refs UniClipboard#486)"
    );
}

/// Build the QUIC `QuicTransportConfig` we attach to the shared endpoint.
///
/// Defaults are tuned assuming "internet"-shaped paths; on macOS the shared
/// Wi-Fi radio is periodically yanked away by AWDL (AirDrop / Handoff /
/// Continuity) and background SSID scans, producing 50–150ms RTT spikes
/// even on a quiet LAN with great signal. Left at defaults, CUBIC interprets
/// those spikes as persistent congestion after 3 PTOs and halves CWND, so a
/// 60 MB transfer drags to ~700 KB/s over a link capable of hundreds of
/// MB/s. These knobs give QUIC more slack for the jitter floor without
/// changing the congestion algorithm itself.
///
/// Any change here affects every transport (pairing / presence / clipboard
/// / blobs) because they share this one endpoint.
///
/// iroh 0.97 reshaped the API: the old quinn-style `&mut TransportConfig`
/// lives behind `iroh::endpoint::QuicTransportConfigBuilder` now, returned
/// by `QuicTransportConfig::builder()`. Setters are by-value chained
/// instead of `&mut self`, but the underlying knobs are the same noq
/// (the project's quinn fork) `TransportConfig` surface.
fn build_transport_config() -> QuicTransportConfig {
    QuicTransportConfig::builder()
        // BBR over CUBIC: we're observing iroh emit "Congestion controller
        // state reset" 3× per connection (path-validation churn) which slams
        // the CUBIC CWND back to 10 MSS each time, forcing slow-start. Even
        // once warmed up, on macOS Wi-Fi a single sporadic loss halves the
        // window — visible in our blob-fetch traces as 1–3s stalls every
        // 4 MB chunk after the first ~22 MB/s burst. BBR models bandwidth ×
        // RTT directly instead of treating loss as a congestion signal, so
        // it recovers from those stalls without giving back the rate it
        // earned. The trade-off is BBR can be unfair to CUBIC flows on a
        // shared bottleneck; that's a non-issue for our P2P single-flow
        // direct UDP path.
        .congestion_controller_factory(Arc::new(BbrConfig::default()))
        // QUIC flow-control sized for hole-punched cross-WAN BDP. iroh-blobs
        // opens a single bidi stream per blob fetch (`open_bi`), so the
        // stream window — not the connection window — is the per-transfer
        // ceiling: max throughput ≈ stream_receive_window / RTT. Quinn's
        // default 1.25 MB is sized for a 100ms × 100 Mbps internet path; on
        // the relay fallback (~360ms RTT) it caps a single blob at ~28 Mbps,
        // and even on a successful hole-punch across continents (~200ms RTT)
        // it caps at ~50 Mbps. 32 MB supports 1 Gbps over 256ms RTT with
        // headroom — well beyond any realistic peer link. Memory cost is
        // bounded: each in-flight blob fetch can stage up to 32 MB of
        // unread chunks, but iroh-blobs reads continuously so the buffer
        // rarely fills.
        .stream_receive_window(VarInt::from_u32(32 * 1024 * 1024))
        // Mirror on the send side. Default `send_window = 8 × 1.25 MB = 10
        // MB` caps connection-total in-flight bytes at the same WAN-hostile
        // value. 64 MB keeps both directions of a sync from saturating
        // their own backpressure on long paths.
        .send_window(64 * 1024 * 1024)
        // Default 3 → 5 PTOs before declaring persistent congestion, so
        // isolated 100–150ms AWDL spikes don't collapse CWND.
        .persistent_congestion_threshold(5)
        // Default 30s idle timeout drops connections between bursty user
        // copies, forcing a fresh hole-punch on every resume. 60s keeps
        // the ConnectionPool entry warm across typical copy gaps.
        .max_idle_timeout(Some(
            Duration::from_secs(60)
                .try_into()
                .expect("60s is well within QUIC's idle-timeout encoding"),
        ))
        // QUIC-level keepalive, complementary to PeerKeepAliveWorker's
        // app-level pings: keeps the magicsock path's last-activity stamp
        // fresh so iroh doesn't tear the path down between transfers.
        // Note: iroh 0.98 changed the signature from `Option<Duration>`
        // (where `None` disabled) to bare `Duration` (always enabled),
        // because the iroh `QuicTransportConfigBuilder` already pre-sets a
        // sane default — calling this setter just overrides it.
        .keep_alive_interval(Duration::from_secs(15))
        // Multipath QUIC extension is *not* enabled — we deliberately leave
        // `max_concurrent_multipath_paths` at the noq-proto default of
        // `None` (i.e. don't call the setter). Rationale:
        //
        // 1. Our shape is 2-peer P2P. Multipath QUIC is designed for hosts
        //    with multiple ISPs simultaneously; we have one functional
        //    path between any two devices.
        //
        // 2. Empirically, enabling multipath (any value ≥ 1) triggers
        //    upstream bug noq#512 ("PTO fires when unset for an abandoned
        //    path") whenever the cross-peer link has more than one
        //    candidate (Tailscale + LAN + vmnet etc) and one of those
        //    candidates gets abandoned during handshake. The race
        //    manifests as: incoming connection arrives at the iroh
        //    endpoint, ALPN routes to `/uniclipboard/pairing/1`,
        //    `noq_proto::connection` silently swallows the PTO timer,
        //    handshake never makes progress, and `iroh::protocol`
        //    eventually 60s-timeouts at the router.accept ALPN deadline.
        //    We observed this at ~50% under release build with multipath
        //    enabled across 10 mac↔fedora rounds, but 0% on fedora
        //    self-pair (localhost UDP, no path abandonment).
        //
        // 3. iroh#4124 (the old reason this setter existed at all —
        //    PathId-budget exhaustion at 13) was about ramping the budget
        //    up. Since we're not enabling multipath, there is no PathId
        //    budget to exhaust.
        //
        // Revisit only after upstream noq#512 is fixed AND we have a
        // real product need for multipath (e.g. mobile multi-radio).
        .build()
}

/// Build the `AddrFilter` we hand to `Endpoint::builder().addr_filter(...)`.
/// The filter is applied at the `AddressLookupServices` layer (see
/// iroh#3960 / #4010), upstream of every individual lookup service, so a
/// single registration covers pkarr / mdns / static / DHT lookups in one
/// place — that's what makes this a viable replacement for "fork iroh and
/// patch magicsock" (issue #486 §三 A).
///
/// `allow_overlay` is captured by the closure so the predicate behaviour is
/// fixed at endpoint bind time — the iroh `Endpoint` does not support
/// runtime mutation of address filters; toggling
/// `Settings.network.allow_overlay_network_addrs` requires a daemon restart
/// (the same constraint as `disable_relays`).
fn build_addr_filter(allow_overlay: bool) -> AddrFilter {
    AddrFilter::new(move |addrs: &Vec<TransportAddr>| apply_addr_filter(addrs, allow_overlay))
}

fn relay_mode_from_config(config: &IrohNodeConfig) -> Result<RelayMode, IrohNodeError> {
    if config.disable_relays {
        return Ok(RelayMode::Disabled);
    }

    if config.custom_relay_urls.is_empty() {
        return Ok(RelayMode::Default);
    }

    let mut relay_urls = Vec::with_capacity(config.custom_relay_urls.len());
    for raw in &config.custom_relay_urls {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(IrohNodeError::InvalidRelayUrl {
                value: raw.clone(),
                message: "relay URL must not be empty".to_string(),
            });
        }
        let parsed = trimmed
            .parse::<RelayUrl>()
            .map_err(|err| IrohNodeError::InvalidRelayUrl {
                value: raw.clone(),
                message: err.to_string(),
            })?;
        if parsed.scheme() != "http" && parsed.scheme() != "https" {
            return Err(IrohNodeError::InvalidRelayUrl {
                value: raw.clone(),
                message: "relay URL scheme must be http or https".to_string(),
            });
        }
        if parsed.host_str().is_none() {
            return Err(IrohNodeError::InvalidRelayUrl {
                value: raw.clone(),
                message: "relay URL must include a host".to_string(),
            });
        }
        relay_urls.push(parsed);
    }

    Ok(RelayMode::custom(relay_urls))
}

/// Pitfall 3 结构性防御：进程级单次 bind 守护（**production-only**）。
///
/// `iroh::Endpoint::builder().relay_mode(...).bind()` 完成后 `RelayMode` 被冻结
/// 为 endpoint 的 bind-time 常量；任何 PR 试图实现"运行时热切换 LAN-only Mode"
/// 必须经过 `endpoint.close() + 重新 IrohNodeBuilder::bind`，第二次 `set` 会 panic
/// 让 production daemon 启动失败 / panic 进程级可见。
///
/// 双契约（**checker BLOCKER 2 — 修订版**）：
/// 1. **Production build（默认 — 无 `test-util` feature 且非 `cfg(test)`）** —
///    OnceCell 守护激活，进程级 single-shot；
/// 2. **Test build (`cfg(test)`)** 与 **下游 crate 启用 `uc-infra/test-util`
///    feature 时** — 守护 elided（不编译），允许同 binary 内多次 bind 支持现有
///    ≥9 处测试 binding 调用（uc-infra/uc-bootstrap pairing e2e 的 sponsor+joiner
///    双 endpoint 等）。
///
/// 注意：下游 crate（如 uc-bootstrap）的 e2e 测试编译时使用的是 uc-infra 的
/// production build —— `#[cfg(test)]` 只对**正在 `cargo test`** 的 crate 生效，
/// 不会传递到依赖。所以单独 `#[cfg(test)]` 不能 elide 守护。这里通过显式
/// cargo feature `test-util` 解决：uc-bootstrap dev-deps 中启用 `uc-infra/test-util`，
/// 当下游运行 e2e 测试时拿到 elided 版本。
///
/// 跨契约的 single-bind 保证由 `uc-bootstrap` 单 entrypoint（`builders.rs:178` /
/// `non_gui_runtime.rs:280` — 详见 plan 05）承担。**这是固有 CI 盲点：测试构建
/// （含下游 e2e）通过 `test-util` feature 永远 elided 守护，单元测试不能覆盖
/// production 守护**；任何修改本守护的 PR 必须用手工 production-build 验证：
/// `cargo build -p uc-bootstrap --release`（不带 `test-util` feature）后启动
/// daemon，断言无二次 bind。
///
/// 见：`.planning/research/PITFALLS.md` §Pitfall 3 + 094-06-PLAN.md must_haves.truths。
#[cfg(not(any(test, feature = "test-util")))]
static BIND_LOCK: OnceLock<()> = OnceLock::new();

/// Staged builder — bind endpoint, install transport handlers, then
/// [`spawn`](Self::spawn) the router.
pub struct IrohNodeBuilder {
    endpoint: Arc<Endpoint>,
    /// Held in `Option` so `install_*` methods can `take()` + reassign the
    /// builder (iroh's `RouterBuilder::accept` consumes `self`).
    router_builder: Option<RouterBuilder>,
    /// Retained so `install_*` methods can read the rendezvous override
    /// when constructing the per-transport adapters.
    config: IrohNodeConfig,
}

impl IrohNodeBuilder {
    /// Bind the iroh endpoint, reusing the Ed25519 secret persisted by
    /// [`IrohIdentityStore`] so the endpoint's on-wire identity matches the
    /// fingerprint `LocalIdentityPort` hands out to domain code.
    ///
    /// Registers [`PAIRING_ALPN`] up front — Slice 1 always has pairing. A
    /// future slice that wants to opt out would add a separate `bind_bare`
    /// constructor; there's no Slice 1 use case for that.
    #[instrument(skip_all)]
    pub async fn bind(
        identity_store: &IrohIdentityStore,
        config: IrohNodeConfig,
    ) -> Result<Self, IrohNodeError> {
        // Pitfall 3 防御（production-only — checker BLOCKER 2 双契约修订版）：
        // 单进程只允许 bind 一次。第二次调用 panic 阻断任何"运行时重建
        // endpoint"路径，迫使运行时热切换走独立 phase 立项。
        // test 配置或下游 crate 开启 `test-util` feature 下 elided ——
        // uc-infra/uc-bootstrap 测试 binary 内 ≥9 处 bind 调用必须正常工作。
        // 注意：`#[cfg(test)]` 只对正在 `cargo test` 的 crate 生效，不传递到
        // 下游依赖；所以下游 e2e 必须开 `uc-infra/test-util` feature。
        #[cfg(not(any(test, feature = "test-util")))]
        BIND_LOCK
            .set(())
            .expect("IrohNodeBuilder::bind called more than once in the same process — runtime hot-swap of LAN-only Mode is explicitly out of scope (Phase 94 / Pitfall 3); see .planning/research/PITFALLS.md");

        let secret = identity_store.ensure_secret_key()?;
        let relay_mode = relay_mode_from_config(&config)?;
        // Snapshot the overlay flag before consuming `config` into `Self`.
        let allow_overlay = config.allow_overlay_network_addrs;
        info!(
            target: "iroh.addr_filter",
            allow_overlay,
            "addr filter configured: overlay-network addresses {} (Tailscale 100.64/10 + fd7a:115c:a1e0::/48)",
            if allow_overlay { "ALLOWED" } else { "BLOCKED" },
        );
        let mut endpoint_builder = Endpoint::builder(presets::N0)
            .secret_key(secret)
            // Only PAIRING is declared at bind time; additional ALPNs are
            // added to the endpoint via `RouterBuilder::spawn`, which
            // rebuilds the ALPN set from every `accept()` handler. See
            // `install_presence` / `install_clipboard`.
            .alpns(vec![PAIRING_ALPN.to_vec()])
            .relay_mode(relay_mode)
            .transport_config(build_transport_config())
            // UniClipboard#486: drop Clash TUN / link-local IPs from every
            // address-lookup service in one shot, and drop CGNAT/Tailscale
            // overlay IPs unless the user opts in. See `build_addr_filter`.
            .addr_filter(build_addr_filter(allow_overlay));

        // LAN-only Mode 收紧（Pitfall 5 防御 + 与 `disable_relays` 字段 doc 一致）：
        // `presets::N0` 默认注入 `PkarrPublisher` (publish 到 dns.iroh.link) +
        // `DnsAddressLookup`，即便 `RelayMode::Disabled` 也会持续向 n0 公共 DNS
        // 广播自己的 NodeId 并解析对端。这与"LAN-only 仅靠 mDNS 在局域网内发现
        // 对方"的用户预期相悖。所以 `disable_relays = true` 路径下显式 clear
        // 掉 N0 注入的 lookup services，再单挂 mDNS。
        //
        // 同步把 LAN-only 状态固化到 `runtime_consts::LAN_ONLY` 进程常量 ——
        // `connect.rs` 出站 dial 时会读这个常量，从对端 `EndpointAddr` 中剥掉
        // `TransportAddr::Relay`。否则即便本端 `RelayMode::Disabled`，iroh 仍
        // 会用对端发布的 relay url 走中转（已在 dev 日志中观测到）。
        //
        // 取舍：跨网段已配对设备无法通过 NodeId 反查到，这是 LAN-only 的设计
        // 意图（不是 bug）。
        super::runtime_consts::install_lan_only(config.disable_relays);
        if config.disable_relays {
            endpoint_builder = endpoint_builder.clear_address_lookup();
            info!(
                target: "iroh.address_lookup",
                "LAN-only mode: cleared n0 pkarr/DNS lookup services; only mDNS will publish/resolve",
            );
        }

        let endpoint = endpoint_builder
            // UniClipboard#486 §三 B: enable mDNS LAN discovery. 在非 LAN-only
            // 路径下与 N0 preset 的 pkarr DHT lookup 并存（mDNS 同子网更快，
            // pkarr 兜底跨网段）；在 LAN-only 路径下是**唯一**的发现来源。
            // The `addr_filter` above also runs over what mDNS publishes,
            // so a Clash `198.18.0.1` won't leak into the LAN announcement
            // even if magicsock surfaces it locally.
            .address_lookup(MdnsAddressLookup::builder())
            .bind()
            .await
            .map_err(|err| IrohNodeError::Bind(err.to_string()))?;
        let endpoint = Arc::new(endpoint);
        let router_builder = Router::builder((*endpoint).clone());
        debug!(
            endpoint_id = %endpoint.id().fmt_short(),
            disable_relays = config.disable_relays,
            allow_overlay_network_addrs = config.allow_overlay_network_addrs,
            custom_relay_count = config.custom_relay_urls.len(),
            rendezvous_override = config.rendezvous_base_url.is_some(),
            "iroh node bound; ready to install transport handlers"
        );
        log_publish_addrs(&endpoint, "post-bind");
        Ok(Self {
            endpoint,
            router_builder: Some(router_builder),
            config,
        })
    }

    /// Install the pairing transport:
    ///
    /// * Registers [`IrohPairingSessionAdapter`] as the [`PAIRING_ALPN`]
    ///   [`iroh::protocol::ProtocolHandler`] so sponsor-side incoming
    ///   connections are accepted.
    /// * Returns the pairing session / event / invitation ports. The first
    ///   two are the same `Arc` cast to two trait objects.
    ///
    /// A single [`RendezvousClient`] is built here and shared between the
    /// session adapter (joiner `dial_by_invitation` → `/resolve`) and the
    /// invitation adapter (sponsor `/pairings` + `/consume`) so the
    /// whole process uses one reqwest connection pool, one timeout, and
    /// one user-agent.
    pub fn install_pairing(
        &mut self,
        device_identity: Arc<dyn DeviceIdentityPort>,
        settings: Arc<dyn SettingsPort>,
    ) -> PairingHandlers {
        let rendezvous = Arc::new(match &self.config.rendezvous_base_url {
            Some(url) => RendezvousClient::with_base_url(url.clone()),
            None => RendezvousClient::new(),
        });

        let adapter = Arc::new(IrohPairingSessionAdapter::new(
            Arc::clone(&self.endpoint),
            Arc::clone(&rendezvous),
        ));

        // `RouterBuilder::accept` consumes `self`; take + reassign so the
        // builder can be called again for a Slice 2 handler in the same
        // chain.
        let builder = self
            .router_builder
            .take()
            .expect("router_builder missing — install_* called after spawn");
        let builder = adapter.install_handler(builder);
        self.router_builder = Some(builder);

        let invitation_adapter = Arc::new(RendezvousPairingInvitationAdapter::new(
            Arc::clone(&self.endpoint),
            device_identity,
            settings,
            rendezvous,
        ));
        let invitation: Arc<dyn PairingInvitationPort> = invitation_adapter.clone();
        let invitation_addresses: Arc<dyn PairingInvitationAddressQueryPort> =
            invitation_adapter.clone();
        let invitation_by_address: Arc<dyn PairingInvitationByAddressPort> = invitation_adapter;

        PairingHandlers {
            session: adapter.clone(),
            events: adapter,
            invitation,
            invitation_addresses,
            invitation_by_address,
        }
    }

    /// Install the presence transport (Slice 2 Phase 1):
    ///
    /// * Registers [`IrohPresenceHandler`] as the [`PRESENCE_ALPN`]
    ///   [`iroh::protocol::ProtocolHandler`] so incoming "is this peer
    ///   reachable" probes are accepted and held open until the peer closes.
    /// * Builds [`IrohPresenceAdapter`] wired to the shared endpoint,
    ///   [`PeerAddressRepositoryPort`] for stored NodeAddr bytes, and
    ///   [`ClockPort`] for event timestamps. Returns it as
    ///   `Arc<dyn PresencePort>` so callers depend on the port, not the
    ///   concrete adapter (`uc-infra/AGENTS.md` §4.3).
    ///
    /// Must be called before [`spawn`](Self::spawn). Safe to call alongside
    /// [`install_pairing`] — the two ALPNs are disjoint so both handlers
    /// coexist on the same router.
    pub fn install_presence(
        &mut self,
        peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
        member_repo: Arc<dyn MemberRepositoryPort>,
        fingerprint_factory: Arc<dyn IdentityFingerprintFactoryPort>,
        clock: Arc<dyn ClockPort>,
    ) -> Arc<dyn PresencePort> {
        // Build the adapter first so the handler shares its `last_state`
        // and broadcast `Sender` — that's what makes inbound dials flip a
        // recovered peer to Online without needing our own keepalive to
        // dial them again.
        let adapter = IrohPresenceAdapter::new(
            Arc::clone(&self.endpoint),
            peer_addr_repo,
            member_repo,
            fingerprint_factory,
            clock,
        );
        let handler = adapter.handler();

        let builder = self
            .router_builder
            .take()
            .expect("router_builder missing — install_* called after spawn");
        let builder = builder.accept(PRESENCE_ALPN, handler);
        self.router_builder = Some(builder);

        Arc::new(adapter)
    }

    /// Build a [`ConnectionChannelPort`] (Phase 96 INDIC-01).
    ///
    /// Pure read adapter — does **not** register an ALPN handler, only wires
    /// the shared endpoint + `peer_addr_repo` so callers can ask
    /// `channel_for(device_id)` to derive Direct/Relay/Offline/Unknown from
    /// the current `Endpoint::remote_info` snapshot.
    ///
    /// Safe to call before or after [`spawn`](Self::spawn) in principle, but
    /// for consistency with the other `install_*` methods (and to keep
    /// bootstrap wiring linear) we expose it as part of the builder phase.
    /// No router mutation, so coexists trivially with every other install_*.
    pub fn install_connection_channel(
        &self,
        peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
    ) -> Arc<dyn ConnectionChannelPort> {
        Arc::new(IrohConnectionChannelAdapter::new(
            Arc::clone(&self.endpoint),
            peer_addr_repo,
        ))
    }

    /// Install the clipboard sync transport (Slice 2 Phase 2):
    ///
    /// * Registers [`IrohClipboardReceiverHandler`] as the
    ///   [`CLIPBOARD_ALPN`] `ProtocolHandler`. Unknown peers are rejected
    ///   at the ack boundary, not at bind time — see the receiver adapter
    ///   for identity resolution via `remote_id()` + fingerprint.
    /// * Returns both clipboard ports as trait objects. The dispatch
    ///   adapter shares the same `Endpoint` and `PeerAddressRepositoryPort`
    ///   as presence — reusing the stored `addr_blob` per peer so a
    ///   dispatch flows through the same NAT/relay mapping the presence
    ///   watchdog already established.
    ///
    /// Must be called before [`spawn`](Self::spawn). Coexists with
    /// [`install_pairing`] / [`install_presence`] — all three ALPNs share
    /// a single router.
    ///
    /// [`IrohClipboardReceiverHandler`]: super::clipboard_receiver_adapter::IrohClipboardReceiverHandler
    pub fn install_clipboard(
        &mut self,
        peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
        member_repo: Arc<dyn MemberRepositoryPort>,
        fingerprint_factory: Arc<dyn IdentityFingerprintFactoryPort>,
    ) -> ClipboardHandlers {
        let receiver = IrohClipboardReceiverAdapter::new(member_repo, fingerprint_factory);
        let handler = receiver.handler();

        let builder = self
            .router_builder
            .take()
            .expect("router_builder missing — install_* called after spawn");
        let builder = builder.accept(CLIPBOARD_ALPN, handler);
        self.router_builder = Some(builder);

        let dispatch = Arc::new(IrohClipboardDispatchAdapter::new(
            Arc::clone(&self.endpoint),
            peer_addr_repo,
        ));

        ClipboardHandlers {
            dispatch,
            receiver: Arc::new(receiver),
        }
    }

    /// 安装"反向传输进度"通道(receiver → sender)。
    ///
    /// * 注册 [`TRANSFER_PROGRESS_ALPN`] 的 `ProtocolHandler`,sender 端
    ///   接收 receiver 推送回来的字节级 fetch 进度帧。
    /// * 返回 [`TransferProgressHandlers`]:reporter 端口给 application
    ///   层在 `BlobProgressSink::report` 时旁路调用;inbound_events 给
    ///   application 层 worker 订阅以翻译成 host event。
    ///
    /// 必须在 [`spawn`](Self::spawn) 之前调用。和 install_clipboard 复用
    /// member_repo / fingerprint_factory 做对端身份验证,陌生 peer 推上
    /// 来的进度直接被丢弃。
    pub fn install_transfer_progress(
        &mut self,
        peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
        member_repo: Arc<dyn MemberRepositoryPort>,
        fingerprint_factory: Arc<dyn IdentityFingerprintFactoryPort>,
    ) -> TransferProgressHandlers {
        let adapter = IrohTransferProgressAdapter::new(
            Arc::clone(&self.endpoint),
            peer_addr_repo,
            member_repo,
            fingerprint_factory,
        );
        let handler = adapter.handler();
        let reporter = adapter.reporter();
        let inbound_events = adapter.subscribe();

        let builder = self
            .router_builder
            .take()
            .expect("router_builder missing — install_* called after spawn");
        let builder = builder.accept(TRANSFER_PROGRESS_ALPN, handler);
        self.router_builder = Some(builder);

        // adapter is dropped after this method returns; reporter and the
        // broadcast subscriber both hold strong refs into the inner state
        // so the handler keeps working.
        let _ = adapter;

        TransferProgressHandlers {
            reporter,
            inbound_events,
        }
    }

    /// 安装 iroh-blobs 传输能力。
    ///
    /// 这个方法把官方 iroh-blobs handler 注册到当前共享 Router 上,同时
    /// 返回实现 `BlobTransferPort` 的 adapter。sqlite 去重缓存不在这里
    /// 构造,它属于数据库装配链。
    pub async fn install_blobs(
        &mut self,
        store_dir: PathBuf,
    ) -> Result<BlobHandlers, IrohNodeError> {
        // Phase D1: enable iroh-blobs internal GC at fixed interval.
        // `FsStore::load_with_opts` is the only public entry that can
        // wire `GcConfig` — `gc::run_gc` / `gc_run_once` themselves are
        // crate-private in iroh-blobs 0.100. The store internally
        // `tokio::spawn`s `run_gc(store, config)` once at load time;
        // the loop lives until the store actor exits (i.e. effectively
        // the daemon's lifetime).
        //
        // `load_with_opts` 的第一个参数是 redb 数据库**文件**路径,不是 root
        // 目录。`Options::new(root)` 才接 root,内部派生出 `root/data`、
        // `root/temp`。直接传 `store_dir` 当 db_path 会让 redb 去把已经被
        // `Options::new` 当作 root 的目录当文件打开,actor 启动会卡死(macOS
        // 上 redb 在目录上的文件锁不会立即返回错误)。和 `FsStore::load`
        // 内部一样用 `root/blobs.db` 作为 db 文件路径。
        let mut options = iroh_blobs::store::fs::options::Options::new(&store_dir);
        options.gc = Some(iroh_blobs::store::GcConfig {
            interval: crate::network::iroh::blobs::BLOBS_GC_INTERVAL,
            add_protected: None,
        });
        let db_path = store_dir.join("blobs.db");
        let store = iroh_blobs::store::fs::FsStore::load_with_opts(db_path, options)
            .await
            .map_err(|err| IrohNodeError::BlobStoreInit(err.to_string()))?;

        // Phase E1 (transitional): sweep `auto-*` tags left behind by
        // pre-Phase-F daemons.
        //
        // iroh-blobs `AddProgress::with_tag` (the `IntoFuture` default for
        // `add_bytes` / `add_path_with_opts`) used to mint a persistent
        // `auto-<timestamp>` tag protecting every newly-published blob
        // from GC. Phase F (`IrohBlobTransferAdapter::publish*`) routes
        // through `with_named_tag` instead, so freshly written blobs no
        // longer carry an auto-tag. But upgrades from older builds inherit
        // the leftover auto-tags, and those still pin every old blob
        // against the Phase D1 GC even after the user deletes the owning
        // entry — sweeping them once at startup is what lets cache reclaim
        // recover for upgraded users.
        //
        // Sweeping is safe because no `add_*` request can land between
        // `FsStore::load_with_opts` returning and us re-acquiring the
        // router (single-threaded init). Phase F also means future
        // publishes never re-create an auto-tag, so post-upgrade this
        // sweep becomes a near-no-op (delete-prefix on an empty range);
        // we keep it for one or two release cycles to cover stragglers
        // and then can drop the call entirely.
        match store.tags().delete_prefix(b"auto-").await {
            Ok(removed) => {
                if removed > 0 {
                    info!(removed, "iroh blobs: swept stale auto-* tags");
                }
            }
            Err(err) => {
                warn!(error = %err, "iroh blobs: failed to sweep stale auto-* tags (non-fatal)");
            }
        }

        let protocol = iroh_blobs::BlobsProtocol::new(&store, None);

        let builder = self
            .router_builder
            .take()
            .expect("router_builder missing — install_* called after spawn");
        let builder = builder.accept(BLOBS_ALPN, protocol);
        self.router_builder = Some(builder);

        let adapter = Arc::new(IrohBlobTransferAdapter::new(
            Arc::clone(&self.endpoint),
            store,
        ));

        info!(
            store_dir = %store_dir.display(),
            alpn = %String::from_utf8_lossy(BLOBS_ALPN),
            endpoint_id = %self.endpoint.id().fmt_short(),
            gc_interval_secs = crate::network::iroh::blobs::BLOBS_GC_INTERVAL.as_secs(),
            "iroh blobs acceptor installed"
        );

        Ok(BlobHandlers {
            blob_transfer: adapter,
        })
    }

    /// Finalize the builder: spawn the [`Router`]. After this point no more
    /// `install_*` calls are allowed.
    pub fn spawn(self) -> IrohNode {
        let router = self
            .router_builder
            .expect("router_builder missing — spawn called twice")
            .spawn();
        log_publish_addrs(&self.endpoint, "post-spawn");
        IrohNode {
            endpoint: self.endpoint,
            router,
        }
    }
}

/// Bootstrap-time failures binding the iroh endpoint. Kept small on
/// purpose — deeper iroh errors are summarised into a string rather than
/// threaded as typed variants per `uc-infra/AGENTS.md` §9.1 (infra error
/// types don't leak third-party error types upward).
#[derive(Debug, thiserror::Error)]
pub enum IrohNodeError {
    #[error("failed to bind iroh endpoint: {0}")]
    Bind(String),

    #[error("failed to initialize iroh blob store: {0}")]
    BlobStoreInit(String),

    #[error("invalid custom iroh relay URL `{value}`: {message}")]
    InvalidRelayUrl { value: String, message: String },

    #[error(transparent)]
    Identity(#[from] LocalIdentityError),
}

#[cfg(test)]
mod tests {
    use super::super::addr_filter::is_virtual_nic_ip;
    use super::*;

    use std::collections::HashMap;
    use std::sync::Mutex as StdMutex;

    use async_trait::async_trait;
    use uc_core::ids::DeviceId;
    use uc_core::ports::{SecureStorageError, SecureStoragePort};
    use uc_core::settings::model::Settings;

    use crate::security::Sha256IdentityFingerprintFactory;

    #[derive(Default)]
    struct InMemorySecureStorage {
        map: StdMutex<HashMap<String, Vec<u8>>>,
    }
    impl SecureStoragePort for InMemorySecureStorage {
        fn get(&self, key: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
            Ok(self.map.lock().unwrap().get(key).cloned())
        }
        fn set(&self, key: &str, value: &[u8]) -> Result<(), SecureStorageError> {
            self.map
                .lock()
                .unwrap()
                .insert(key.to_string(), value.to_vec());
            Ok(())
        }
        fn delete(&self, key: &str) -> Result<(), SecureStorageError> {
            self.map.lock().unwrap().remove(key);
            Ok(())
        }
    }

    struct FixedDeviceIdentity(DeviceId);
    impl DeviceIdentityPort for FixedDeviceIdentity {
        fn current_device_id(&self) -> DeviceId {
            self.0.clone()
        }
    }

    struct InMemorySettings(StdMutex<Settings>);
    #[async_trait]
    impl SettingsPort for InMemorySettings {
        async fn load(&self) -> anyhow::Result<Settings> {
            Ok(self.0.lock().unwrap().clone())
        }
        async fn save(&self, s: &Settings) -> anyhow::Result<()> {
            *self.0.lock().unwrap() = s.clone();
            Ok(())
        }
    }

    fn identity_store() -> Arc<IrohIdentityStore> {
        Arc::new(IrohIdentityStore::new(
            Arc::new(InMemorySecureStorage::default()),
            Arc::new(Sha256IdentityFingerprintFactory),
        ))
    }

    #[tokio::test]
    async fn bind_install_pairing_spawn_and_shutdown_cleanly() {
        let store = identity_store();
        let mut builder = IrohNodeBuilder::bind(&store, IrohNodeConfig::default())
            .await
            .expect("bind");
        let handlers = builder.install_pairing(
            Arc::new(FixedDeviceIdentity(DeviceId::new("device-1"))),
            Arc::new(InMemorySettings(StdMutex::new(Settings::default()))),
        );
        // Ports are handed out as trait objects so ownership (and hence
        // the session adapter) survives past the node's spawn.
        drop(handlers);
        let node = builder.spawn();
        // Clean shutdown exits without hanging; the test runner's default
        // timeout would catch a deadlock.
        node.shutdown().await;
    }

    #[tokio::test]
    async fn bind_is_idempotent_across_builds_for_same_store() {
        // The endpoint id is derived from the Ed25519 secret, so a second
        // bind against the same store must see the same id (rotating it
        // would break every peer that already remembered us).
        let store = identity_store();
        let first = IrohNodeBuilder::bind(&store, IrohNodeConfig::default())
            .await
            .expect("first bind");
        let first_id = first.endpoint.id();
        let first_node = first.spawn();
        first_node.shutdown().await;

        let second = IrohNodeBuilder::bind(&store, IrohNodeConfig::default())
            .await
            .expect("second bind");
        assert_eq!(second.endpoint.id(), first_id);
        second.spawn().shutdown().await;
    }

    #[derive(Default)]
    struct EmptyPeerAddressRepo;
    #[async_trait]
    impl PeerAddressRepositoryPort for EmptyPeerAddressRepo {
        async fn get(
            &self,
            _device: &DeviceId,
        ) -> Result<Option<uc_core::ports::PeerAddressRecord>, uc_core::ports::PeerAddressError>
        {
            Ok(None)
        }
        async fn upsert(
            &self,
            _record: &uc_core::ports::PeerAddressRecord,
        ) -> Result<(), uc_core::ports::PeerAddressError> {
            Ok(())
        }
        async fn list(
            &self,
        ) -> Result<Vec<uc_core::ports::PeerAddressRecord>, uc_core::ports::PeerAddressError>
        {
            Ok(Vec::new())
        }
        async fn remove(&self, _device: &DeviceId) -> Result<(), uc_core::ports::PeerAddressError> {
            Ok(())
        }
    }

    struct FixedClock(i64);
    impl ClockPort for FixedClock {
        fn now_ms(&self) -> i64 {
            self.0
        }
    }

    #[tokio::test]
    async fn install_pairing_and_install_presence_coexist_on_same_router() {
        // Two ALPNs on one router — proves Slice 2 Phase 1's presence
        // transport slices in without disturbing Slice 1's pairing wiring.
        let store = identity_store();
        let mut builder = IrohNodeBuilder::bind(&store, IrohNodeConfig::default())
            .await
            .expect("bind");

        let _pairing = builder.install_pairing(
            Arc::new(FixedDeviceIdentity(DeviceId::new("device-coexist"))),
            Arc::new(InMemorySettings(StdMutex::new(Settings::default()))),
        );

        let presence: Arc<dyn PresencePort> = builder.install_presence(
            Arc::new(EmptyPeerAddressRepo),
            Arc::new(EmptyMemberRepo),
            Arc::new(crate::security::Sha256IdentityFingerprintFactory),
            Arc::new(FixedClock(1_700_000_000_000)),
        );

        // current_state before any dial is Unknown — proves the adapter
        // survived the type-erasure round trip through install_presence.
        let unknown_state = presence.current_state(&DeviceId::new("never-dialed")).await;
        assert_eq!(unknown_state, uc_core::ports::ReachabilityState::Unknown,);

        let node = builder.spawn();
        node.shutdown().await;
    }

    #[derive(Default)]
    struct EmptyMemberRepo;
    #[async_trait]
    impl uc_core::membership::MemberRepositoryPort for EmptyMemberRepo {
        async fn get(
            &self,
            _device_id: &DeviceId,
        ) -> Result<Option<uc_core::membership::SpaceMember>, uc_core::membership::MembershipError>
        {
            Ok(None)
        }
        async fn list(
            &self,
        ) -> Result<Vec<uc_core::membership::SpaceMember>, uc_core::membership::MembershipError>
        {
            Ok(Vec::new())
        }
        async fn save(
            &self,
            _member: &uc_core::membership::SpaceMember,
        ) -> Result<(), uc_core::membership::MembershipError> {
            Ok(())
        }
        async fn remove(
            &self,
            _device_id: &DeviceId,
        ) -> Result<bool, uc_core::membership::MembershipError> {
            Ok(false)
        }
    }

    #[tokio::test]
    async fn install_pairing_presence_and_clipboard_coexist_on_same_router() {
        // Three ALPNs on one router — Slice 2 Phase 2 (clipboard) slices
        // in alongside pairing + presence. Verifies both clipboard ports
        // survive the trait-object round trip and the router spawns /
        // shuts down cleanly when all three transports are installed.
        let store = identity_store();
        let mut builder = IrohNodeBuilder::bind(&store, IrohNodeConfig::default())
            .await
            .expect("bind");

        let _pairing = builder.install_pairing(
            Arc::new(FixedDeviceIdentity(DeviceId::new("device-triple"))),
            Arc::new(InMemorySettings(StdMutex::new(Settings::default()))),
        );

        let peer_addr_repo: Arc<dyn PeerAddressRepositoryPort> = Arc::new(EmptyPeerAddressRepo);
        let _presence = builder.install_presence(
            Arc::clone(&peer_addr_repo),
            Arc::new(EmptyMemberRepo),
            Arc::new(crate::security::Sha256IdentityFingerprintFactory),
            Arc::new(FixedClock(1_700_000_000_000)),
        );

        let ClipboardHandlers { dispatch, receiver } = builder.install_clipboard(
            peer_addr_repo,
            Arc::new(EmptyMemberRepo),
            Arc::new(crate::security::Sha256IdentityFingerprintFactory),
        );

        // Dispatch against an unknown device — the repo is empty so we
        // hit the `Offline` short-circuit without touching the wire. This
        // verifies the trait object round-tripped and is usable.
        let offline_err = dispatch
            .dispatch(
                &DeviceId::new("never-paired"),
                &uc_core::ports::ClipboardHeader {
                    version: uc_core::ports::ClipboardHeader::CURRENT_VERSION,
                    content_hash: "0".repeat(64),
                    captured_at_ms: 0,
                    origin_device_id: "self".to_string(),
                    origin_device_name: "Self".to_string(),
                    payload_version: 3,
                    flow_id: None,
                },
                uc_core::ports::SyncPayload {
                    ciphertext: bytes::Bytes::from_static(b"x"),
                },
            )
            .await
            .expect_err("no peer_addr → Offline");
        assert!(
            matches!(offline_err, uc_core::ports::ClipboardDispatchError::Offline),
            "expected Offline, got {offline_err:?}"
        );

        // Receiver's subscribe handle is ready for the ingest use case.
        let _inbound_rx = receiver.subscribe();

        let node = builder.spawn();
        node.shutdown().await;
    }

    #[tokio::test]
    async fn install_pairing_presence_clipboard_and_blobs_coexist_on_same_router() {
        let store = identity_store();
        let mut builder = IrohNodeBuilder::bind(&store, IrohNodeConfig::default())
            .await
            .expect("bind");

        let _pairing = builder.install_pairing(
            Arc::new(FixedDeviceIdentity(DeviceId::new("device-quad"))),
            Arc::new(InMemorySettings(StdMutex::new(Settings::default()))),
        );

        let peer_addr_repo: Arc<dyn PeerAddressRepositoryPort> = Arc::new(EmptyPeerAddressRepo);
        let _presence = builder.install_presence(
            Arc::clone(&peer_addr_repo),
            Arc::new(EmptyMemberRepo),
            Arc::new(crate::security::Sha256IdentityFingerprintFactory),
            Arc::new(FixedClock(1_700_000_000_000)),
        );

        let _clipboard = builder.install_clipboard(
            peer_addr_repo,
            Arc::new(EmptyMemberRepo),
            Arc::new(crate::security::Sha256IdentityFingerprintFactory),
        );

        let tempdir = tempfile::tempdir().expect("tempdir");
        let BlobHandlers { blob_transfer } = builder
            .install_blobs(tempdir.path().join("iroh-blobs"))
            .await
            .expect("install blobs");

        let digest = blob_transfer
            .publish(
                bytes::Bytes::from_static(b"router-four-alpns"),
                uc_core::ports::blob::TagReason::ClipboardEntry(uc_core::ids::EntryId::from_str(
                    "router-blob-test",
                )),
            )
            .await
            .expect("publish through blob port");
        assert!(blob_transfer.has(&digest).await.expect("has digest"));

        let node = builder.spawn();
        node.shutdown().await;
    }

    // ──────────────────────────────────────────────────────────────────
    // is_virtual_nic_ip / build_addr_filter unit tests
    // ──────────────────────────────────────────────────────────────────

    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    use std::net::{SocketAddr, SocketAddrV4, SocketAddrV6};

    fn v4(ip: &str) -> IpAddr {
        IpAddr::V4(ip.parse::<Ipv4Addr>().unwrap())
    }
    fn v6(ip: &str) -> IpAddr {
        IpAddr::V6(ip.parse::<Ipv6Addr>().unwrap())
    }

    /// Always-filtered classes are filtered regardless of the overlay flag.
    #[test]
    fn always_filtered_classes_ignore_overlay_flag() {
        for &allow in &[false, true] {
            // 198.18.0.0/15 — Clash fake-ip
            assert!(
                is_virtual_nic_ip(v4("198.18.0.1"), allow),
                "198.18.0.1 must be filtered (allow_overlay={allow})"
            );
            assert!(
                is_virtual_nic_ip(v4("198.19.255.255"), allow),
                "198.19.255.255 must be filtered (allow_overlay={allow})"
            );
            // 169.254.0.0/16 — IPv4 link-local
            assert!(
                is_virtual_nic_ip(v4("169.254.1.1"), allow),
                "169.254.1.1 must be filtered (allow_overlay={allow})"
            );
        }
    }

    /// CGNAT/Tailscale 100.64/10 is filtered when overlay is denied,
    /// allowed when overlay is permitted.
    #[test]
    fn cgnat_v4_respects_overlay_flag() {
        // allow_overlay = false → filtered
        assert!(is_virtual_nic_ip(v4("100.64.0.1"), false));
        assert!(is_virtual_nic_ip(v4("100.100.100.100"), false));
        assert!(is_virtual_nic_ip(v4("100.127.255.255"), false));

        // allow_overlay = true → allowed (predicate returns false)
        assert!(!is_virtual_nic_ip(v4("100.64.0.1"), true));
        assert!(!is_virtual_nic_ip(v4("100.100.100.100"), true));
        assert!(!is_virtual_nic_ip(v4("100.127.255.255"), true));
    }

    /// Tailscale IPv6 ULA fd7a:115c:a1e0::/48 mirrors the CGNAT v4 case.
    #[test]
    fn tailscale_ula_v6_respects_overlay_flag() {
        // First three 16-bit segments must match: fd7a:115c:a1e0
        assert!(is_virtual_nic_ip(v6("fd7a:115c:a1e0::1"), false));
        assert!(is_virtual_nic_ip(v6("fd7a:115c:a1e0:ab12:cd34::"), false));
        assert!(!is_virtual_nic_ip(v6("fd7a:115c:a1e0::1"), true));
        assert!(!is_virtual_nic_ip(v6("fd7a:115c:a1e0:ab12:cd34::"), true));
    }

    /// Real-world LAN/WAN IPs are never filtered, regardless of flag.
    #[test]
    fn real_world_addresses_pass_through() {
        for &allow in &[false, true] {
            for ip in [
                v4("10.0.0.1"),
                v4("192.168.1.42"),
                v4("172.16.0.5"),
                v4("8.8.8.8"),
                v4("100.63.255.255"), // just below 100.64/10
                v4("100.128.0.0"),    // just above 100.64/10
                v4("198.17.255.255"), // just below 198.18/15
                v4("198.20.0.0"),     // just above 198.18/15
                v4("169.253.255.255"),
                v4("169.255.0.0"),
                v6("2001:db8::1"),
                v6("fe80::1"),
                v6("fd7a:115c:a1e1::1"), // off-by-one outside Tailscale ULA
                v6("fc00::1"),           // generic ULA, not Tailscale
            ] {
                assert!(
                    !is_virtual_nic_ip(ip, allow),
                    "{ip} must NOT be filtered (allow_overlay={allow})"
                );
            }
        }
    }

    /// apply_addr_filter: no virtual addresses present → returns Borrowed
    /// (untouched).
    #[test]
    fn addr_filter_passes_through_clean_set() {
        let addrs = vec![
            TransportAddr::Ip(SocketAddr::V4(SocketAddrV4::new(
                "192.168.1.1".parse().unwrap(),
                4242,
            ))),
            TransportAddr::Ip(SocketAddr::V4(SocketAddrV4::new(
                "10.0.0.5".parse().unwrap(),
                4242,
            ))),
        ];
        let kept = apply_addr_filter(&addrs, false);
        assert_eq!(
            kept.len(),
            addrs.len(),
            "clean set must be passed through unchanged"
        );
    }

    /// apply_addr_filter: drops virtual NIC addrs when allow_overlay=false.
    #[test]
    fn addr_filter_drops_overlay_when_disallowed() {
        let addrs = vec![
            TransportAddr::Ip(SocketAddr::V4(SocketAddrV4::new(
                "192.168.1.1".parse().unwrap(),
                4242,
            ))),
            TransportAddr::Ip(SocketAddr::V4(SocketAddrV4::new(
                "100.64.0.7".parse().unwrap(),
                4242,
            ))), // Tailscale 100.x — must be dropped
            TransportAddr::Ip(SocketAddr::V6(SocketAddrV6::new(
                "fd7a:115c:a1e0::1".parse().unwrap(),
                4242,
                0,
                0,
            ))), // Tailscale ULA — must be dropped
            TransportAddr::Ip(SocketAddr::V4(SocketAddrV4::new(
                "198.18.0.1".parse().unwrap(),
                4242,
            ))), // Clash — always dropped
        ];
        let kept: Vec<TransportAddr> = apply_addr_filter(&addrs, false).into_owned();
        assert_eq!(kept.len(), 1, "only the real LAN IP should survive");
        match &kept[0] {
            TransportAddr::Ip(s) => assert_eq!(s.ip().to_string(), "192.168.1.1"),
            _ => panic!("expected Ip variant"),
        }
    }

    /// apply_addr_filter: keeps overlay addrs when allow_overlay=true,
    /// but still drops always-filtered classes.
    #[test]
    fn addr_filter_keeps_overlay_when_allowed_but_still_drops_clash() {
        let addrs = vec![
            TransportAddr::Ip(SocketAddr::V4(SocketAddrV4::new(
                "192.168.1.1".parse().unwrap(),
                4242,
            ))),
            TransportAddr::Ip(SocketAddr::V4(SocketAddrV4::new(
                "100.64.0.7".parse().unwrap(),
                4242,
            ))), // overlay — kept now
            TransportAddr::Ip(SocketAddr::V6(SocketAddrV6::new(
                "fd7a:115c:a1e0::1".parse().unwrap(),
                4242,
                0,
                0,
            ))), // overlay — kept now
            TransportAddr::Ip(SocketAddr::V4(SocketAddrV4::new(
                "198.18.0.1".parse().unwrap(),
                4242,
            ))), // Clash — always dropped
            TransportAddr::Ip(SocketAddr::V4(SocketAddrV4::new(
                "169.254.0.5".parse().unwrap(),
                4242,
            ))), // link-local — always dropped
        ];
        let kept: Vec<TransportAddr> = apply_addr_filter(&addrs, true).into_owned();
        assert_eq!(kept.len(), 3, "real LAN + 2 overlay candidates kept");
        let ips: Vec<String> = kept
            .iter()
            .filter_map(|a| match a {
                TransportAddr::Ip(s) => Some(s.ip().to_string()),
                _ => None,
            })
            .collect();
        assert!(ips.contains(&"192.168.1.1".to_string()));
        assert!(ips.contains(&"100.64.0.7".to_string()));
        assert!(ips.iter().any(|s| s == "fd7a:115c:a1e0::1"));
        assert!(!ips.iter().any(|s| s == "198.18.0.1"));
        assert!(!ips.iter().any(|s| s == "169.254.0.5"));
    }

    #[test]
    fn relay_mode_empty_custom_urls_uses_default_when_enabled() {
        let cfg = IrohNodeConfig {
            disable_relays: false,
            custom_relay_urls: Vec::new(),
            ..Default::default()
        };
        let mode = relay_mode_from_config(&cfg).expect("relay mode");
        assert!(matches!(mode, RelayMode::Default));
    }

    #[test]
    fn relay_mode_custom_urls_builds_custom_map() {
        let cfg = IrohNodeConfig {
            disable_relays: false,
            custom_relay_urls: vec![
                "https://relay-a.example.com.".to_string(),
                "https://relay-b.example.com.".to_string(),
            ],
            ..Default::default()
        };
        let mode = relay_mode_from_config(&cfg).expect("relay mode");
        match mode {
            RelayMode::Custom(map) => {
                let urls = map.urls::<Vec<_>>();
                assert_eq!(urls.len(), 2);
                assert!(urls
                    .iter()
                    .any(|url| url.as_str() == "https://relay-a.example.com./"));
                assert!(urls
                    .iter()
                    .any(|url| url.as_str() == "https://relay-b.example.com./"));
            }
            other => panic!("expected custom relay mode, got {other:?}"),
        }
    }

    #[test]
    fn relay_mode_lan_only_ignores_custom_urls() {
        let cfg = IrohNodeConfig {
            disable_relays: true,
            custom_relay_urls: vec!["https://relay.example.com.".to_string()],
            ..Default::default()
        };
        let mode = relay_mode_from_config(&cfg).expect("relay mode");
        assert!(matches!(mode, RelayMode::Disabled));
    }

    #[test]
    fn relay_mode_rejects_invalid_custom_url() {
        let cfg = IrohNodeConfig {
            disable_relays: false,
            custom_relay_urls: vec!["not a url".to_string()],
            ..Default::default()
        };
        let err = relay_mode_from_config(&cfg).expect_err("invalid relay url");
        assert!(matches!(err, IrohNodeError::InvalidRelayUrl { .. }));
    }

    #[test]
    fn relay_mode_rejects_custom_url_with_unsupported_scheme() {
        let cfg = IrohNodeConfig {
            disable_relays: false,
            custom_relay_urls: vec!["ftp://relay.example.com".to_string()],
            ..Default::default()
        };
        let err = relay_mode_from_config(&cfg).expect_err("invalid relay scheme");
        assert!(matches!(err, IrohNodeError::InvalidRelayUrl { .. }));
    }
}
