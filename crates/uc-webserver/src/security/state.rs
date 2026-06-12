//! Shared security state for the daemon HTTP API.
//!
//! Holds the JWT signing secret, PID whitelist, and rate limiter.
//! Cloned into the `DaemonApiState` at server startup.

use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::rate_limiter::SlidingWindowRateLimiter;

/// Shared security state for all daemon HTTP API requests.
///
/// Contains:
/// - `jwt_secret`: 32-byte secret used for HS256 JWT signing/verification.
///   Generated once at startup, never persisted to disk.
/// - `allowed_pids`: Set of client PIDs that have registered via `/auth/connect`.
/// - `rate_limiter`: Sliding-window rate limiter per client.
#[derive(Clone)]
pub struct SecurityState {
    /// HMAC secret for HS256 JWT signing and verification.
    pub jwt_secret: Arc<[u8; 32]>,
    /// Set of allowed client process IDs (registered via auth/connect).
    pub allowed_pids: Arc<RwLock<HashSet<u32>>>,
    /// Per-client sliding-window rate limiter.
    pub rate_limiter: Arc<SlidingWindowRateLimiter>,
}

impl SecurityState {
    /// Create a new SecurityState with a randomly generated JWT secret.
    pub fn new() -> Self {
        let mut secret = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut secret);
        Self {
            jwt_secret: Arc::new(secret),
            allowed_pids: Arc::new(RwLock::new(HashSet::new())),
            rate_limiter: Arc::new(SlidingWindowRateLimiter::new()),
        }
    }

    /// Create a new SecurityState with a single PID pre-registered in the whitelist.
    ///
    /// This is useful in test fixtures where async PID registration is not available
    /// (e.g., synchronous test setup functions). The PID is directly inserted into
    /// the `allowed_pids` set without requiring an async context.
    pub fn new_with_pid(pid: u32) -> Self {
        let state = Self::new();
        let mut pids = state
            .allowed_pids
            .try_write()
            .expect("SecurityState::new_with_pid: RwLock write contention at construction time is unexpected");
        pids.insert(pid);
        drop(pids);
        state
    }

    /// Generate a JWT session token signed with this state's jwt_secret.
    ///
    /// Used in test fixtures to pre-obtain a session token for use in
    /// authenticated requests without requiring an async `/auth/connect` call.
    ///
    /// # Panics
    /// Panics if JWT signing fails (should not happen with a valid secret).
    pub fn make_session_token_for_pid(&self, pid: u32) -> String {
        use super::claims::{SessionTokenClaims, LEVEL_L2};
        let claims = SessionTokenClaims::new(pid, "test".to_string(), LEVEL_L2, false);
        claims
            .sign(self.jwt_secret.as_ref())
            .expect("SecurityState::make_session_token_for_pid: JWT signing failed")
    }

    /// Register a client PID in the whitelist.
    ///
    /// Called during `/auth/connect` when a client successfully authenticates.
    /// Once registered, the PID can pass PID whitelist verification in JWT middleware.
    pub async fn register_pid(&self, pid: u32) {
        let mut pids = self.allowed_pids.write().await;
        pids.insert(pid);
    }

    /// Check if a PID is in the allowed whitelist.
    ///
    /// Called during JWT authentication to verify the client PID is registered.
    pub async fn is_pid_allowed(&self, pid: u32) -> bool {
        let pids = self.allowed_pids.read().await;
        pids.contains(&pid)
    }

    /// Remove a PID from the allowed whitelist.
    pub async fn unregister_pid(&self, pid: u32) {
        let mut pids = self.allowed_pids.write().await;
        pids.remove(&pid);
    }

    /// Clean up stale entries from the rate limiter.
    ///
    /// Should be called periodically (e.g., by a background task).
    pub async fn cleanup_rate_limiter(&self) {
        self.rate_limiter.cleanup_stale().await;
    }
}

impl Default for SecurityState {
    fn default() -> Self {
        Self::new()
    }
}
