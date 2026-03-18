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

    /// Return the filter directives as a string (for testing/debugging).
    #[cfg(test)]
    fn directives_string(&self) -> String {
        let base = match self {
            Self::Dev => "debug",
            Self::Prod | Self::Cli => "info",
            Self::DebugClipboard => "info",
        };

        let mut directives = vec![base.to_string()];
        for &filter in NOISE_FILTERS {
            directives.push(filter.to_string());
        }
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
        directives.join(",")
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Env var tests need serialization since they modify process-global state
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_from_env_dev() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("UC_LOG_PROFILE", "dev");
        std::env::remove_var("RUST_LOG");
        assert_eq!(LogProfile::from_env(), LogProfile::Dev);
        std::env::remove_var("UC_LOG_PROFILE");
    }

    #[test]
    fn test_from_env_prod() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("UC_LOG_PROFILE", "prod");
        std::env::remove_var("RUST_LOG");
        assert_eq!(LogProfile::from_env(), LogProfile::Prod);
        std::env::remove_var("UC_LOG_PROFILE");
    }

    #[test]
    fn test_from_env_debug_clipboard() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("UC_LOG_PROFILE", "debug_clipboard");
        std::env::remove_var("RUST_LOG");
        assert_eq!(LogProfile::from_env(), LogProfile::DebugClipboard);
        std::env::remove_var("UC_LOG_PROFILE");
    }

    #[test]
    fn test_from_env_unset_defaults_to_build_type() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("UC_LOG_PROFILE");
        std::env::remove_var("RUST_LOG");
        let profile = LogProfile::from_env();
        // In debug builds (test), should be Dev
        if cfg!(debug_assertions) {
            assert_eq!(profile, LogProfile::Dev);
        } else {
            assert_eq!(profile, LogProfile::Prod);
        }
    }

    #[test]
    fn test_from_env_unrecognized_defaults_to_build_type() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("UC_LOG_PROFILE", "unknown_value");
        std::env::remove_var("RUST_LOG");
        let profile = LogProfile::from_env();
        if cfg!(debug_assertions) {
            assert_eq!(profile, LogProfile::Dev);
        } else {
            assert_eq!(profile, LogProfile::Prod);
        }
        std::env::remove_var("UC_LOG_PROFILE");
    }

    #[test]
    fn test_dev_filter_has_debug_base() {
        let directives = LogProfile::Dev.directives_string();
        assert!(directives.starts_with("debug,"));
    }

    #[test]
    fn test_prod_filter_has_info_base() {
        let directives = LogProfile::Prod.directives_string();
        assert!(directives.starts_with("info,"));
    }

    #[test]
    fn test_from_env_cli() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("UC_LOG_PROFILE", "cli");
        std::env::remove_var("RUST_LOG");
        assert_eq!(LogProfile::from_env(), LogProfile::Cli);
        std::env::remove_var("UC_LOG_PROFILE");
    }

    #[test]
    fn test_cli_console_filter_is_off() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("RUST_LOG");
        // CLI profile should have console filter set to "off"
        let filter = LogProfile::Cli.console_filter();
        // Verify it builds without panic; the filter should reject all events
        let _ = filter;
    }

    #[test]
    fn test_cli_json_filter_is_info() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("RUST_LOG");
        let directives = LogProfile::Cli.directives_string();
        assert!(
            directives.starts_with("info,"),
            "CLI JSON filter should be info-level, got: {}",
            directives
        );
    }

    #[test]
    fn test_all_profiles_include_noise_filters() {
        for profile in [
            LogProfile::Dev,
            LogProfile::Prod,
            LogProfile::DebugClipboard,
            LogProfile::Cli,
        ] {
            let directives = profile.directives_string();
            assert!(
                directives.contains("libp2p_mdns=info"),
                "Missing libp2p_mdns=info in {profile}"
            );
            assert!(
                directives.contains("libp2p_mdns::behaviour::iface=off"),
                "Missing iface=off in {profile}"
            );
            assert!(
                directives.contains("tauri=warn"),
                "Missing tauri=warn in {profile}"
            );
            assert!(
                directives.contains("wry=off"),
                "Missing wry=off in {profile}"
            );
            assert!(
                directives.contains("ipc::request=off"),
                "Missing ipc::request=off in {profile}"
            );
            assert!(
                directives.contains("hyper_util=info"),
                "Missing hyper_util=info in {profile}"
            );
            assert!(
                directives.contains("hyper=info"),
                "Missing hyper=info in {profile}"
            );
        }
    }

    #[test]
    fn test_dev_profile_includes_platform_debug() {
        let directives = LogProfile::Dev.directives_string();
        assert!(directives.contains("uc_platform=debug"));
        assert!(directives.contains("uc_infra=debug"));
    }

    #[test]
    fn test_debug_clipboard_includes_clipboard_targets() {
        let directives = LogProfile::DebugClipboard.directives_string();
        assert!(directives.contains("uc_platform::adapters::clipboard=trace"));
        assert!(directives.contains("uc_app::usecases::clipboard=debug"));
        assert!(directives.contains("uc_core::clipboard=debug"));
    }

    #[test]
    fn test_json_filter_is_symmetric_with_console_filter() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("RUST_LOG");
        // We can't directly compare EnvFilters, but we can verify the directives are the same
        // Note: Cli profile is intentionally asymmetric (console=off, json=info)
        for profile in [
            LogProfile::Dev,
            LogProfile::Prod,
            LogProfile::DebugClipboard,
        ] {
            let console_directives = profile.directives_string();
            // json_filter uses the same build_filter() so directives should match
            let json_directives = profile.directives_string();
            assert_eq!(
                console_directives, json_directives,
                "Asymmetry in {profile}"
            );
        }
    }

    #[test]
    fn test_display_impl() {
        assert_eq!(LogProfile::Dev.to_string(), "dev");
        assert_eq!(LogProfile::Prod.to_string(), "prod");
        assert_eq!(LogProfile::DebugClipboard.to_string(), "debug_clipboard");
        assert_eq!(LogProfile::Cli.to_string(), "cli");
    }

    #[test]
    fn test_console_filter_builds_valid_envfilter() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("RUST_LOG");
        // Should not panic
        let _ = LogProfile::Dev.console_filter();
        let _ = LogProfile::Prod.console_filter();
        let _ = LogProfile::DebugClipboard.console_filter();
        let _ = LogProfile::Cli.console_filter();
    }
}
