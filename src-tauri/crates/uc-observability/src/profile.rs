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
/// 2. `UC_LOG_PROFILE` env var (`dev`, `prod`, `debug_clipboard`)
/// 3. Build-type default: debug builds -> `Dev`, release builds -> `Prod`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogProfile {
    /// Development profile: debug-level base, verbose uc_platform/uc_infra
    Dev,
    /// Production profile: info-level base
    Prod,
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
    "opentelemetry_sdk=warn",
    // iroh 0.97 forked quinn into noq; the old quinn=info directives no
    // longer match. noq_proto::connection in particular emits ~40k DEBUG
    // events per peer-hour without this cap.
    "noq=info",
    "noq_proto=info",
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
    // Catch-all for libraries that emit through the `log` crate (forwarded
    // into tracing). Without this they default to TRACE.
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

    /// Build the `EnvFilter` for the OTLP export layer.
    ///
    /// Always INFO-and-above regardless of profile. Debug/trace spans must
    /// never leave the machine via OTLP telemetry.
    ///
    /// `UC_OTLP_EXTRA` (comma-separated directives) appends to the default
    /// filter at runtime — used for time-bounded remote diagnostics (e.g.
    /// raising `iroh_quinn=debug` during a blob-fetch incident) without
    /// rebuilding the daemon. Directives are appended last, so they take
    /// precedence over both the base level and `NOISE_FILTERS`.
    pub fn otlp_filter(&self) -> EnvFilter {
        let mut directives = vec!["info".to_string()];
        for &filter in NOISE_FILTERS {
            directives.push(filter.to_string());
        }
        if let Ok(extra) = std::env::var("UC_OTLP_EXTRA") {
            for d in extra.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                directives.push(d.to_string());
            }
        }
        EnvFilter::new(directives.join(","))
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
            EnvFilter::try_from_default_env().ok()
        } else {
            None
        }
    }

    /// Build filter directives for this profile.
    fn build_filter(&self) -> EnvFilter {
        let base = match self {
            Self::Dev => "debug",
            Self::Prod | Self::Cli => "info",
            Self::DebugClipboard => "info",
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
            Self::DebugClipboard => write!(f, "debug_clipboard"),
            Self::Cli => write!(f, "cli"),
        }
    }
}
