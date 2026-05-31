//! Sliding-window rate limiter for daemon HTTP API.
//!
//! Tracks request timestamps per client identifier and enforces a maximum
//! request count within a configurable time window.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Maximum requests per client within the sliding window.
const MAX_REQUESTS: u32 = 100;

/// Window duration in seconds.
const WINDOW_SECS: u64 = 60;

struct RateLimiterInner {
    /// Maps client_id -> sorted list of request timestamps (newest last).
    entries: HashMap<String, Vec<tokio::time::Instant>>,
}

/// Sliding-window rate limiter using `tokio::time::Instant` for testable time control.
///
/// Each client has their own request history within a 60-second sliding window.
/// After 100 requests in the window, subsequent requests are rejected until
/// older requests expire from the window.
#[derive(Clone)]
pub struct SlidingWindowRateLimiter {
    inner: Arc<RwLock<RateLimiterInner>>,
    max_requests: u32,
    window_secs: u64,
}

impl SlidingWindowRateLimiter {
    /// Create a new rate limiter with default configuration (100 req/min).
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(RateLimiterInner {
                entries: HashMap::new(),
            })),
            max_requests: MAX_REQUESTS,
            window_secs: WINDOW_SECS,
        }
    }

    /// Check if a request from the given client should be allowed.
    ///
    /// Prunes stale entries (older than the window), then checks if the client
    /// is within the rate limit. If allowed, records the current timestamp.
    ///
    /// Returns `true` if the request is allowed, `false` if rate-limited.
    pub async fn check(&self, client_id: &str) -> bool {
        let now = tokio::time::Instant::now();
        let window = std::time::Duration::from_secs(self.window_secs);

        let mut inner = self.inner.write().await;
        let entries = &mut inner.entries;

        // Prune stale entries (older than window). On Windows `tokio::time::Instant`
        // is anchored at system boot, so within the first `window` after boot
        // `now - window` underflows and panics. In that case no entry can be stale
        // yet, so skip pruning instead of subtracting.
        if let Some(cutoff) = now.checked_sub(window) {
            if let Some(timestamps) = entries.get_mut(client_id) {
                timestamps.retain(|&t| t > cutoff);
            }
        }

        // Check current count
        let count = entries.get(client_id).map(|v| v.len()).unwrap_or(0) as u32;
        if count >= self.max_requests {
            return false;
        }

        // Record this request
        entries.entry(client_id.to_string()).or_default().push(now);
        true
    }

    /// Remove all stale entries (older than the window) across all clients.
    ///
    /// Should be called periodically to prevent unbounded memory growth.
    pub async fn cleanup_stale(&self) {
        let now = tokio::time::Instant::now();
        let window = std::time::Duration::from_secs(self.window_secs);
        // See `check`: within the first `window` after boot the subtraction
        // underflows on Windows and there is nothing to clean up yet.
        let Some(cutoff) = now.checked_sub(window) else {
            return;
        };

        let mut inner = self.inner.write().await;
        for timestamps in inner.entries.values_mut() {
            timestamps.retain(|&t| t > cutoff);
        }
    }
}

impl Default for SlidingWindowRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a limiter whose window is larger than any possible process uptime,
    /// so `now - window` underflows `Instant` deterministically. This reproduces
    /// the Windows boot-time crash (where `tokio::time::Instant::now()` is anchored
    /// at boot and is smaller than the window) without depending on real uptime.
    fn limiter_with_overflowing_window() -> SlidingWindowRateLimiter {
        SlidingWindowRateLimiter {
            inner: Arc::new(RwLock::new(RateLimiterInner {
                entries: HashMap::new(),
            })),
            max_requests: MAX_REQUESTS,
            window_secs: u64::MAX,
        }
    }

    #[tokio::test]
    async fn check_does_not_panic_when_window_exceeds_uptime() {
        let limiter = limiter_with_overflowing_window();
        // Must not panic on `now - window` underflow, and the request is allowed.
        assert!(limiter.check("client-a").await);
    }

    #[tokio::test]
    async fn cleanup_stale_does_not_panic_when_window_exceeds_uptime() {
        let limiter = limiter_with_overflowing_window();
        limiter.check("client-a").await;
        // Must not panic on `now - window` underflow.
        limiter.cleanup_stale().await;
    }
}
