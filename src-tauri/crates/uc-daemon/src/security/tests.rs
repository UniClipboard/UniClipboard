//! Integration tests for daemon security middleware.
//!
//! These tests exercise the security primitives at the unit level.
//! HTTP-level integration tests live in `tests/` directory.

use std::sync::Arc;

use crate::security::{
    claims::{LEVEL_L1, LEVEL_L2},
    PermissionLevel, SecurityState, SessionTokenClaims, SlidingWindowRateLimiter,
};

#[tokio::test]
async fn rate_limit_rejects_after_100_requests() {
    let limiter = Arc::new(SlidingWindowRateLimiter::new());
    for i in 0..100 {
        assert!(limiter.check("client-a").await, "request {i} should be allowed");
    }
    assert!(
        !limiter.check("client-a").await,
        "101st request should be rejected"
    );
}

#[tokio::test]
async fn rate_limit_per_client_isolation() {
    let limiter = Arc::new(SlidingWindowRateLimiter::new());
    for _ in 0..100 {
        limiter.check("client-a").await;
    }
    // client-b should still be allowed
    assert!(limiter.check("client-b").await);
}

#[tokio::test]
async fn rate_limit_window_sliding() {
    tokio::time::pause();
    let limiter = Arc::new(SlidingWindowRateLimiter::new());

    // Exhaust the limit for client-a
    for _ in 0..100 {
        limiter.check("client-a").await;
    }
    assert!(!limiter.check("client-a").await);

    // Advance time past the window
    tokio::time::advance(std::time::Duration::from_secs(61)).await;
    assert!(
        limiter.check("client-a").await,
        "should allow after window expires"
    );
}

#[tokio::test]
async fn rate_limit_cleanup_removes_stale_entries() {
    tokio::time::pause();
    let limiter = Arc::new(SlidingWindowRateLimiter::new());

    limiter.check("stale-client").await;
    assert!(limiter.check("stale-client").await);

    tokio::time::advance(std::time::Duration::from_secs(61)).await;
    limiter.cleanup_stale().await;

    // After cleanup, stale-client's window should be gone
    assert!(
        limiter.check("stale-client").await,
        "should allow after cleanup"
    );
}

// PID Whitelist Tests

#[tokio::test]
async fn pid_whitelist_accepts_registered_pid() {
    let state = SecurityState::new();
    state.register_pid(12345).await;
    assert!(state.is_pid_allowed(12345).await);
}

#[tokio::test]
async fn pid_whitelist_rejects_unregistered_pid() {
    let state = SecurityState::new();
    state.register_pid(12345).await;
    assert!(!state.is_pid_allowed(99999).await);
}

#[tokio::test]
async fn pid_whitelist_allows_multiple_pids() {
    let state = SecurityState::new();
    state.register_pid(111).await;
    state.register_pid(222).await;
    state.register_pid(333).await;
    assert!(state.is_pid_allowed(111).await);
    assert!(state.is_pid_allowed(222).await);
    assert!(state.is_pid_allowed(333).await);
    assert!(!state.is_pid_allowed(444).await);
}

// JWT Tests

#[tokio::test]
async fn jwt_sign_and_verify_roundtrip() {
    let state = SecurityState::new();
    let claims = SessionTokenClaims::new(12345, "gui".into(), LEVEL_L2, false);
    let token = claims.sign(&state.jwt_secret).unwrap();
    let verified = SessionTokenClaims::verify(&token, &state.jwt_secret).unwrap();
    assert_eq!(verified.pid, 12345);
    assert_eq!(verified.client_type, "gui");
    assert_eq!(verified.access_level, LEVEL_L2);
    assert!(!verified.encryption_ready);
}

#[tokio::test]
async fn jwt_expired_token_rejected() {
    let mut claims = SessionTokenClaims::new(12345, "gui".into(), LEVEL_L2, false);
    // Manually set exp to the past (7 days ago)
    claims.exp = chrono::Utc::now().timestamp() - 86400 * 7;
    let state = SecurityState::new();
    let token = claims.sign(&state.jwt_secret).unwrap();
    let result = SessionTokenClaims::verify(&token, &state.jwt_secret);
    assert!(result.is_err());
}

#[tokio::test]
async fn jwt_wrong_secret_rejected() {
    let state_a = SecurityState::new();
    let state_b = SecurityState::new(); // different secret
    let claims = SessionTokenClaims::new(12345, "gui".into(), LEVEL_L2, false);
    let token = claims.sign(&state_a.jwt_secret).unwrap();
    let result = SessionTokenClaims::verify(&token, &state_b.jwt_secret);
    assert!(result.is_err());
}

#[tokio::test]
async fn jwt_fields_correct_after_verify() {
    let state = SecurityState::new();
    let claims = SessionTokenClaims::new(99999, "cli".into(), LEVEL_L2, true);
    let token = claims.sign(&state.jwt_secret).unwrap();
    let verified = SessionTokenClaims::verify(&token, &state.jwt_secret).unwrap();

    assert_eq!(verified.iss, "uniclipboard-daemon");
    assert_eq!(verified.sub, "frontend");
    assert_eq!(verified.pid, 99999);
    assert_eq!(verified.client_type, "cli");
    assert_eq!(verified.access_level, LEVEL_L2);
    assert!(verified.encryption_ready);
    assert!(!verified.jti.is_empty());
}

// Permission Level Tests

#[test]
fn permission_level_l1_maps_from_u8() {
    assert_eq!(
        PermissionLevel::from_u8(1),
        Some(PermissionLevel::L1Public)
    );
}

#[test]
fn permission_level_l2_maps_from_u8() {
    assert_eq!(
        PermissionLevel::from_u8(2),
        Some(PermissionLevel::L2Authenticated)
    );
}

#[test]
fn permission_level_l3_returns_l3_sensitive() {
    assert_eq!(
        PermissionLevel::from_u8(3),
        Some(PermissionLevel::L3Sensitive)
    );
}

#[test]
fn permission_level_l4_returns_l4_dangerous() {
    assert_eq!(
        PermissionLevel::from_u8(4),
        Some(PermissionLevel::L4Dangerous)
    );
}

#[test]
fn permission_level_other_values_return_none() {
    assert_eq!(PermissionLevel::from_u8(0), None);
    assert_eq!(PermissionLevel::from_u8(99), None);
}
