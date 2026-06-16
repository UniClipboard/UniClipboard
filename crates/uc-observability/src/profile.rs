//! Log profile selection and filter construction for UniClipboard.
//!
//! Provides the `LogProfile` enum for selecting logging verbosity profiles
//! via the `UC_LOG_PROFILE` environment variable, with build-type defaults.

use std::fmt;
use tracing_subscriber::EnvFilter;

/// Logging profile that controls filter verbosity for both console and JSON outputs.
///
/// # Profile Selection Precedence
///
/// 1. `RUST_LOG` env var (overrides everything when set)
/// 2. `UC_LOG_PROFILE` env var (`dev`, `prod`, `debug`, `debug_clipboard`)
/// 3. Build-type default: debug builds -> `Dev`, release builds -> `Prod`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogProfile {
    /// Development profile: debug-level base, verbose uc_platform/uc_infra
    Dev,
    /// Production profile: info-level base
    Prod,
    /// User-facing debug profile: production base with UniClipboard targets raised to debug.
    Debug,
    /// Clipboard debugging profile: info-level base with clipboard targets raised to debug/trace
    DebugClipboard,
    /// CLI profile: console output disabled (no noise), JSON file logging at info level
    Cli,
}

/// Common noise filter directives applied to all profiles.
const NOISE_FILTERS: &[&str] = &[
    "libp2p_mdns=info",
    "libp2p_mdns::behaviour::iface=off",
    "tauri=warn",
    "wry=off",
    "ipc::request=off",
    "hyper_util=info",
    "hyper=info",
    "quinn=info",
    "quinn_proto=info",
    "quinn_udp=info",
    "Connection::poll=warn",
    "Pool::poll=warn",
    "Swarm::poll=warn",
    // iroh 0.97 forked quinn into noq; the old quinn=info directives no
    // longer match. noq_proto::connection in particular emits ~40k DEBUG
    // events per peer-hour without this cap.
    "noq=info",
    "noq_proto=info",
    // noq-udp is a separate crate from `noq` so the directive above does
    // not cover it. On dual-stack hosts where the VPN / Clash TUN / stale
    // virtual NIC has no route to a remote IPv6 destination, the udp send
    // path emits a WARN per transmit (`sendmsg error: No route to host`).
    // Same EHOSTUNREACH pattern as `swarm_discovery::socket` above —
    // harmless because reachability still works over other interfaces,
    // but high-frequency, so cap at ERROR to keep Sentry Logs quiet.
    "noq_udp=error",
    // netwatch 0.16/0.17/main 在 `UdpSocket::poll_recv_noq` 的 trace! 字段里
    // 直接做 `meta.len / meta.stride`,而 `noq_udp::RecvMeta.stride` 在 GRO/GSO
    // 边界(空 datagram、内核回退到非分段路径)允许为 0,触发 divide-by-zero
    // panic —— 因为 panic 在第三方 crate 的 trace! 求值阶段,我们栈上没有任何
    // uc_* 帧,只能在拿到 trace event 前就把该 target 截掉。上游跟踪
    // n0-computer/net-tools#148;在上游 release 修复前用 EnvFilter 硬上限堵
    // trace。受影响 Sentry: UNICLIPBOARD-RUST-18 (Windows) / -S (macOS),
    // 同根因被按 OS 拆组。
    "netwatch::udp=debug",
    // QUIC connection state machine internals. Cap at WARN: silences the
    // ~40k DEBUG/INFO events per peer-hour of steady-state churn but keeps
    // the WARN/ERROR signals visible. The earlier `=off` here masked the
    // exact symptoms ("PTO expired while unset", "failed closing path
    // err=MultipathNotNegotiated") of upstream noq#512, which made a
    // multipath-pairing failure invisible at the prod log level — every
    // diagnosis had to override RUST_LOG to even see the root cause.
    // Higher layers do re-emit *some* real failures, but for QUIC state
    // machine races they only re-emit a generic "router.accept timed out"
    // 60s after the fact, with no path/PTO context. Keeping WARN/ERROR
    // through preserves that context cheaply.
    "noq_proto::connection=warn",
    // magicsock multipath state machine. The remote_state submodule is
    // also where iroh#4124 spams `Opening path failed` on every event
    // once the per-connection PathId budget is exhausted; cap that one
    // at ERROR until upstream lands a fix (PR pending against v1.0.0-rc).
    "iroh::socket=info",
    "iroh::socket::remote_map::remote_state=error",
    // swarm-discovery is the mDNS backend pulled in by the
    // `address-lookup-mdns` feature; very chatty at INFO/DEBUG.
    "swarm_discovery=warn",
    // The socket actor logs `error sending mDNS: No route to host` on every
    // tick when a bound interface (VPN/Clash TUN, stale virtual NIC, Wi-Fi
    // mid-reassoc) returns EHOSTUNREACH. The condition is harmless — peer
    // discovery still works on the other interfaces — but the actor's send
    // cadence is fixed inside the crate, so we suppress the per-tick WARN.
    "swarm_discovery::socket=error",
    // hickory-dns resolver used by pkarr + relay URL resolution.
    "hickory=warn",
    // === log-bridge noise ===
    //
    // Crates that emit through the `log` crate (forwarded via tracing-log).
    // The catch-all `log=warn` is *not reliable* on its own: tracing-log 0.2
    // dispatches each record in two steps —
    //   1. `dispatch.enabled(filter_meta)` where `filter_meta.target()` is the
    //      record's real target (e.g. `rustls::client::hs`)
    //   2. `dispatch.event(Event::new(static_meta, ...))` where
    //      `static_meta.target()` is the literal `"log"`
    // EnvFilter caches Interest by static callsite. When the filter set has
    // both a base level (`debug`) and a target rule (`log=warn`), the cached
    // Interest collapses to `sometimes`, and the runtime check falls back on
    // the dynamic `filter_meta` whose target is the real module path. At that
    // point `log=warn` no longer matches and the base `debug` lets it through
    // — which is why we measured ~23k DEBUG events leaking under target="log"
    // (rustls handshake spam dominated).
    //
    // Fix: cap the actual log-bridged crates by their real prefix. These
    // catch the high-volume offenders observed in production:
    //   rustls::client::hs / tls13 / common_state  — TLS handshake DEBUG x10
    //                                                 per iroh connect attempt
    //   reqwest::connect                           — DEBUG per HTTP connect
    //   igd_next::aio::tokio                       — UPnP/IGD discovery loop
    "rustls=warn",
    "reqwest=warn",
    "igd_next=warn",
    // Keep the literal-"log" catch-all as a backstop for any other log-only
    // crate that slips in. It works for the subset of cases where EnvFilter's
    // callsite Interest resolves statically (no `sometimes` fallback), and is
    // harmless when it doesn't.
    "log=warn",
];

impl LogProfile {
    /// Select a profile from environment variables.
    ///
    /// Reads `UC_LOG_PROFILE` first. If unset or unrecognized, falls back to
    /// build-type default (`Dev` for debug builds, `Prod` for release builds).
    pub fn from_env() -> Self {
        match std::env::var("UC_LOG_PROFILE").as_deref() {
            Ok("dev") => Self::Dev,
            Ok("prod") => Self::Prod,
            Ok("debug") => Self::Debug,
            Ok("debug_clipboard") => Self::DebugClipboard,
            Ok("cli") => Self::Cli,
            _ => {
                if cfg!(debug_assertions) {
                    Self::Dev
                } else {
                    Self::Prod
                }
            }
        }
    }

    /// Build the `EnvFilter` for the console (pretty) layer.
    ///
    /// If `RUST_LOG` is set, returns that override filter instead.
    /// For the `Cli` profile, console output is completely disabled.
    pub fn console_filter(&self) -> EnvFilter {
        if let Some(filter) = Self::rust_log_override() {
            return filter;
        }
        if matches!(self, Self::Cli) {
            return EnvFilter::new("off");
        }
        self.build_filter()
    }

    /// Build the `EnvFilter` for the JSON file layer.
    ///
    /// Symmetric with `console_filter` per design decision, except for the
    /// `Cli` profile which still logs to JSON at info level for debugging.
    /// If `RUST_LOG` is set, returns that override filter instead.
    pub fn json_filter(&self) -> EnvFilter {
        if let Some(filter) = Self::rust_log_override() {
            return filter;
        }
        self.build_filter()
    }

    /// Check if `RUST_LOG` is set and return an override `EnvFilter`.
    fn rust_log_override() -> Option<EnvFilter> {
        if std::env::var("RUST_LOG").is_ok() {
            let filter = EnvFilter::try_from_default_env().ok()?;
            // 即使用户主动开了 RUST_LOG=trace 也必须把 netwatch::udp 压回 debug,
            // 否则会触发 netwatch udp.rs:436 的 divide-by-zero panic(详见
            // NOISE_FILTERS 同名条目)。`add_directive` 会覆盖同 target 的更
            // 宽松规则,放在用户 RUST_LOG 解析之后追加即可生效。
            let filter =
                filter.add_directive("netwatch::udp=debug".parse().expect("static directive"));
            Some(filter)
        } else {
            None
        }
    }

    /// Build filter directives for this profile.
    fn build_filter(&self) -> EnvFilter {
        let base = match self {
            Self::Dev => "debug",
            Self::Prod | Self::Cli => "info",
            Self::Debug | Self::DebugClipboard => "info",
        };

        let mut directives = vec![base.to_string()];

        // Common noise filters
        for &filter in NOISE_FILTERS {
            directives.push(filter.to_string());
        }

        // Profile-specific directives
        match self {
            Self::Dev => {
                directives.push("uc_platform=debug".to_string());
                directives.push("uc_infra=debug".to_string());
            }
            Self::DebugClipboard => {
                directives.push("uc_platform::adapters::clipboard=trace".to_string());
                directives.push("uc_app::usecases::clipboard=debug".to_string());
                directives.push("uc_core::clipboard=debug".to_string());
            }
            Self::Debug => {
                directives.push("uc_core=debug".to_string());
                directives.push("uc_application=debug".to_string());
                directives.push("uc_app=debug".to_string());
                directives.push("uc_infra=debug".to_string());
                directives.push("uc_platform=debug".to_string());
                directives.push("uc_bootstrap=debug".to_string());
                directives.push("uc_webserver=debug".to_string());
                directives.push("uc_daemon_client=debug".to_string());
                directives.push("uc_daemon_local=debug".to_string());
                directives.push("uc_daemon_process=debug".to_string());
                directives.push("uc_desktop=debug".to_string());
                directives.push("uc_tauri=debug".to_string());
                directives.push("uc_cli=debug".to_string());
            }
            Self::Prod | Self::Cli => {}
        }

        EnvFilter::new(directives.join(","))
    }
}

impl fmt::Display for LogProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dev => write!(f, "dev"),
            Self::Prod => write!(f, "prod"),
            Self::Debug => write!(f, "debug"),
            Self::DebugClipboard => write!(f, "debug_clipboard"),
            Self::Cli => write!(f, "cli"),
        }
    }
}
