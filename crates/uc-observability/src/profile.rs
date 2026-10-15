//! Log profile selection and filter construction for UniClipboard.
//!
//! Provides the `LogProfile` enum for selecting logging verbosity profiles
//! via the `UC_LOG_PROFILE` environment variable, with build-type defaults.

use std::fmt;
use std::sync::OnceLock;

use tracing_subscriber::EnvFilter;

static INSTALLED_PROFILE: OnceLock<String> = OnceLock::new();

/// Environment variable that switches the console log layer off when set to `off`.
///
/// A process whose standard output is a file nobody rotates (the macOS launchd
/// service) sets it so that file cannot grow without bound; the JSON file layer
/// keeps its own daily rotation and retention.
pub const CONSOLE_LOG_ENV: &str = "UC_CONSOLE_LOG";

/// Whether a [`CONSOLE_LOG_ENV`] value turns the console layer off.
pub(crate) fn console_logging_disabled(value: Option<&str>) -> bool {
    value == Some("off")
}

/// Record the profile this process installed its host log layers with.
///
/// Returns `false` when a profile was already recorded; the first one stays.
pub fn set_installed_log_profile(profile: &LogProfile) -> bool {
    INSTALLED_PROFILE.set(profile.to_string()).is_ok()
}

/// Profile this process runs with, `None` before host logging is installed.
///
/// This is what the host log files actually record at. The persisted Debug Mode
/// setting only becomes the installed profile at the next start, and
/// `UC_LOG_PROFILE` / `RUST_LOG` can override it entirely.
pub fn installed_log_profile() -> Option<&'static str> {
    INSTALLED_PROFILE.get().map(String::as_str)
}

/// Logging profile that controls filter verbosity for both console and JSON outputs.
///
/// # Profile Selection Precedence
///
/// 1. `RUST_LOG` env var (subject to mandatory privacy and safety caps)
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
///
/// Host log layers only receive host-process events: the Engine keeps its own
/// records, and the network stack it runs (iroh, noq, quinn, hickory, pkarr,
/// swarm-discovery, portmapper, netdev), away from them. Directives for those
/// targets would never match and are deliberately absent. `netwatch::udp` is
/// the one exception, kept as a guard against a panic described below.
const NOISE_FILTERS: &[&str] = &[
    "hyper_util=info",
    "hyper=info",
    // netwatch 0.16/0.17/main panics with a divide-by-zero while evaluating a
    // `trace!` field in `UdpSocket::poll_recv_noq` (`noq_udp::RecvMeta.stride`
    // may be 0 at GRO/GSO boundaries). The panic happens in third-party code
    // before any `uc_*` frame exists, so the target is capped here as well as
    // in the `RUST_LOG` override. Upstream: n0-computer/net-tools#148. Remove
    // once a fixed netwatch is released.
    "netwatch::udp=debug",
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
    // — which is why ~23k DEBUG events leaked under target="log" (rustls
    // handshake spam dominated).
    //
    // Fix: cap the actual log-bridged crates by their real prefix:
    //   rustls::client::hs / tls13 / common_state  — TLS handshake DEBUG
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

/// Mandatory privacy caps that no log profile or `RUST_LOG` override may relax.
///
/// OpenMLS can emit cryptographic keys, plaintext, and storage values through
/// its `log`-based debug paths. Those records must never reach a persistent or
/// remote sink, even when a user explicitly requests verbose diagnostics.
const SENSITIVE_TARGET_FILTERS: &[&str] = &["openmls=off", "openmls_memory_storage=off"];

fn enforce_sensitive_target_filters(mut filter: EnvFilter) -> EnvFilter {
    for &directive in SENSITIVE_TARGET_FILTERS {
        let Ok(directive) = directive.parse() else {
            return EnvFilter::new("off");
        };
        filter = filter.add_directive(directive);
    }
    filter
}

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
    /// If `RUST_LOG` is set, returns that override with mandatory caps applied.
    /// Console output is completely disabled for the `Cli` profile and when
    /// [`CONSOLE_LOG_ENV`] is `off`.
    pub fn console_filter(&self) -> EnvFilter {
        if console_logging_disabled(std::env::var(CONSOLE_LOG_ENV).ok().as_deref()) {
            return EnvFilter::new("off");
        }
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
    /// If `RUST_LOG` is set, returns that override with mandatory caps applied.
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
            let filter = match "netwatch::udp=debug".parse() {
                Ok(directive) => filter.add_directive(directive),
                Err(_) => EnvFilter::new("off"),
            };
            Some(enforce_sensitive_target_filters(filter))
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
            }
            Self::DebugClipboard => {
                directives.push("uc_platform::adapters::clipboard=trace".to_string());
            }
            Self::Debug => {
                directives.push("uc_platform=debug".to_string());
                directives.push("uc_bootstrap=debug".to_string());
                directives.push("uc_webserver=debug".to_string());
                directives.push("uc_daemon=debug".to_string());
                directives.push("uc_daemon_client=debug".to_string());
                directives.push("uc_daemon_local=debug".to_string());
                directives.push("uc_daemon_process=debug".to_string());
                directives.push("uc_dev_cli=debug".to_string());
            }
            Self::Prod | Self::Cli => {}
        }

        enforce_sensitive_target_filters(EnvFilter::new(directives.join(",")))
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

#[cfg(test)]
mod tests {
    use tracing_subscriber::prelude::*;

    use super::*;

    /// Targets owned by the Engine or by the network stack it runs. The Engine
    /// keeps them away from host log layers, so a host profile directive for
    /// them can never match an event.
    const ENGINE_AND_NETWORK_TARGETS: [&str; 16] = [
        "uc_core",
        "uc_application",
        "uc_infra",
        "uc_app=",
        "uc_app::",
        "uc_tauri",
        "libp2p",
        "tauri=",
        "wry=",
        "ipc::request",
        "quinn",
        "noq",
        "iroh",
        "swarm_discovery",
        "hickory",
        "Swarm::poll",
    ];

    #[test]
    fn profiles_only_name_targets_a_host_layer_can_receive() {
        for profile in [
            LogProfile::Dev,
            LogProfile::Prod,
            LogProfile::Debug,
            LogProfile::DebugClipboard,
            LogProfile::Cli,
        ] {
            let directives = profile.build_filter().to_string();
            for target in ENGINE_AND_NETWORK_TARGETS {
                assert!(
                    !directives.contains(target),
                    "{profile} names {target}: {directives}"
                );
            }
        }
    }

    #[test]
    fn the_console_switch_only_accepts_an_explicit_off() {
        assert!(console_logging_disabled(Some("off")));
        for value in [None, Some(""), Some("on"), Some("OFF "), Some("0")] {
            assert!(!console_logging_disabled(value), "{value:?}");
        }
    }

    #[test]
    fn debug_profile_raises_the_daemon_host_itself() {
        let directives = LogProfile::Debug.build_filter().to_string();
        assert!(directives.contains("uc_daemon=debug"), "{directives}");
        assert!(directives.contains("uc_webserver=debug"), "{directives}");
    }

    #[test]
    fn development_profile_never_enables_openmls_crypto_logs() {
        let subscriber = tracing_subscriber::registry().with(LogProfile::Dev.json_filter());
        let dispatch = tracing::Dispatch::new(subscriber);

        tracing::dispatcher::with_default(&dispatch, || {
            assert!(!tracing::enabled!(
                target: "openmls::ciphersuite::hpke",
                tracing::Level::DEBUG
            ));
            assert!(!tracing::enabled!(
                target: "openmls_memory_storage",
                tracing::Level::DEBUG
            ));
        });
    }

    #[test]
    fn explicit_trace_override_cannot_enable_openmls_crypto_logs() {
        let subscriber = tracing_subscriber::registry().with(enforce_sensitive_target_filters(
            EnvFilter::new("trace,openmls=trace,openmls_memory_storage=trace"),
        ));
        let dispatch = tracing::Dispatch::new(subscriber);

        tracing::dispatcher::with_default(&dispatch, || {
            assert!(!tracing::enabled!(
                target: "openmls::ciphersuite::hpke",
                tracing::Level::ERROR
            ));
            assert!(!tracing::enabled!(
                target: "openmls_memory_storage",
                tracing::Level::ERROR
            ));
        });
    }
}
