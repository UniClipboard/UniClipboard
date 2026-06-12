//! Security middleware for daemon HTTP API.
//!
//! Phase 75 provides: JWT session tokens, PID whitelist, rate limiting (L2).
//! L3/L4 permission enforcement is reserved for future phases.

pub mod claims;
pub mod connect;
pub mod middleware;
pub mod permission;
pub mod rate_limiter;
pub mod state;

// Re-export commonly used types
pub use claims::SessionTokenClaims;
pub use middleware::{auth_extractor_middleware, rate_limit_middleware, ClientId};
pub use permission::PermissionLevel;
pub use rate_limiter::SlidingWindowRateLimiter;
pub use state::SecurityState;

use std::sync::Arc;

/// Spawn a background task that periodically cleans up stale entries from the rate limiter.
///
/// The task runs every 5 minutes and respects the `cancel` token for shutdown.
pub fn cleanup_rate_limiter_task(
    security: Arc<SecurityState>,
    cancel: tokio_util::sync::CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        const CLEANUP_INTERVAL_SECS: u64 = 300;
        let mut interval =
            tokio::time::interval(std::time::Duration::from_secs(CLEANUP_INTERVAL_SECS));
        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    tracing::debug!("rate limiter cleanup task shutting down");
                    break;
                }
                _ = interval.tick() => {
                    security.cleanup_rate_limiter().await;
                }
            }
        }
    })
}
