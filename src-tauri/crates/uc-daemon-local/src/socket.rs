//! Shared daemon HTTP address resolution.

use std::path::PathBuf;

use anyhow::Result;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use uc_app::app_paths::AppPaths;

pub const DEFAULT_HTTP_HOST: &str = "127.0.0.1";
pub const DEFAULT_HTTP_PORT: u16 = 42715;
const PROFILE_A_HTTP_PORT: u16 = 42716;
const PROFILE_B_HTTP_PORT: u16 = 42717;
// `package.json` runs `tauri:dev` with `UC_PROFILE=dev`.
// Keep that common profile on a stable reserved port instead of hashing it
// into the general high-port space, which can collide with unrelated local services.
const PROFILE_DEV_HTTP_PORT: u16 = 42718;
const PROFILE_HTTP_PORT_START: u16 = 42719;

/// Resolve the loopback-only daemon HTTP listen address.
pub fn resolve_daemon_http_addr() -> SocketAddr {
    try_resolve_daemon_http_addr()
        .expect("daemon http address resolution should stay within loopback port range")
}

/// Resolve the loopback-only daemon HTTP listen address with explicit error propagation.
pub fn try_resolve_daemon_http_addr() -> Result<SocketAddr> {
    Ok(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
        resolve_daemon_http_port()?,
    ))
}

fn resolve_daemon_http_port() -> Result<u16> {
    match std::env::var("UC_PROFILE") {
        Ok(profile) if profile.trim().is_empty() => Ok(DEFAULT_HTTP_PORT),
        Ok(profile) if profile.eq_ignore_ascii_case("a") => Ok(PROFILE_A_HTTP_PORT),
        Ok(profile) if profile.eq_ignore_ascii_case("b") => Ok(PROFILE_B_HTTP_PORT),
        Ok(profile) if profile.eq_ignore_ascii_case("dev") => Ok(PROFILE_DEV_HTTP_PORT),
        Ok(profile) => resolve_hashed_profile_http_port(&profile),
        Err(_) => Ok(DEFAULT_HTTP_PORT),
    }
}

fn resolve_hashed_profile_http_port(profile: &str) -> Result<u16> {
    let slot_count = u32::from(u16::MAX) - u32::from(PROFILE_HTTP_PORT_START) + 1;
    let hash = stable_profile_hash(profile);
    let offset = (hash % u64::from(slot_count)) as u16;

    PROFILE_HTTP_PORT_START.checked_add(offset).ok_or_else(|| {
        anyhow::anyhow!(
            "profile-derived daemon HTTP port overflowed reserved range for UC_PROFILE={profile}"
        )
    })
}

fn stable_profile_hash(profile: &str) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    profile.as_bytes().iter().fold(FNV_OFFSET, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME)
    })
}

/// Resolve the daemon auth token path using AppPaths.
pub fn resolve_daemon_token_path() -> Result<PathBuf> {
    let dirs = uc_platform::app_dirs::default_app_dirs()?;
    Ok(AppPaths::from_app_dirs(&dirs).daemon_token_path())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_uc_profile<T>(value: Option<&str>, f: impl FnOnce() -> T) -> T {
        let _guard = crate::test_env::lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = std::env::var("UC_PROFILE").ok();

        match value {
            Some(profile) => std::env::set_var("UC_PROFILE", profile),
            None => std::env::remove_var("UC_PROFILE"),
        }

        let result = f();

        match previous {
            Some(profile) => std::env::set_var("UC_PROFILE", profile),
            None => std::env::remove_var("UC_PROFILE"),
        }

        result
    }

    #[test]
    fn test_http_addr_is_loopback() {
        let addr = with_uc_profile(None, resolve_daemon_http_addr);
        assert_eq!(addr.ip().to_string(), DEFAULT_HTTP_HOST);
        assert_eq!(addr.port(), DEFAULT_HTTP_PORT);
    }

    #[test]
    fn test_profiled_http_addr_uses_stable_distinct_ports() {
        let default_addr = with_uc_profile(None, resolve_daemon_http_addr);
        let addr_a = with_uc_profile(Some("a"), resolve_daemon_http_addr);
        let addr_b = with_uc_profile(Some("b"), resolve_daemon_http_addr);
        let addr_dev = with_uc_profile(Some("dev"), resolve_daemon_http_addr);
        let addr_team = with_uc_profile(Some("team-alpha"), resolve_daemon_http_addr);
        let addr_team_repeat = with_uc_profile(Some("team-alpha"), resolve_daemon_http_addr);

        assert_eq!(default_addr.port(), 42715);
        assert_eq!(addr_a.port(), 42716);
        assert_eq!(addr_b.port(), 42717);
        assert_eq!(addr_dev.port(), 42718);
        assert_ne!(addr_team.port(), default_addr.port());
        assert_ne!(addr_team.port(), addr_a.port());
        assert_ne!(addr_team.port(), addr_b.port());
        assert_ne!(addr_team.port(), addr_dev.port());
        assert_eq!(addr_team.port(), addr_team_repeat.port());
    }
}