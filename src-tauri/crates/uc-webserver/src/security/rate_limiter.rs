//! Sliding-window rate limiter for daemon HTTP API.
//!
//! Tracks request timestamps per client identifier and enforces a maximum
//! request count within a fixed time window. The per-request budget is supplied
//! by the caller (see [`PREAUTH_MAX_REQUESTS`] / [`AUTHENTICATED_MAX_REQUESTS`])
//! so one shared limiter can apply a tight pre-auth cap keyed by IP and a loose
//! authenticated backstop keyed by PID.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Window duration in seconds.
const WINDOW_SECS: u64 = 60;

/// Request budget per window for PRE-AUTH clients, keyed by source IP.
///
/// Applies to `/auth/connect` — the one untrusted, pre-authentication surface,
/// where a caller has not yet proven a whitelisted PID. Kept deliberately tight.
pub const PREAUTH_MAX_REQUESTS: u32 = 100;

/// Request budget per window for AUTHENTICATED local clients, keyed by PID.
///
/// Anything reaching this limiter has already passed L2 (valid JWT + whitelisted
/// PID), i.e. it is a trusted same-host process. This ceiling is therefore only a
/// runaway-loop backstop (~50 req/s), NOT a fine-grained throttle: a steady
/// interactive GUI polling status/clipboard/presence — even while the user
/// rapidly switches clipboard content — must never hit it.
pub const AUTHENTICATED_MAX_REQUESTS: u32 = 3000;

/// Outcome of a rate-limit check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RateLimitDecision {
    /// Within budget; the request has been recorded against the window.
    Allowed,
    /// Over budget. `retry_after_secs` is computed from when the oldest in-window
    /// request expires and a slot frees up — not a fixed constant — so a client
    /// that is only slightly over backs off by only a second or two instead of a
    /// full window.
    Limited { retry_after_secs: u64 },
}

struct RateLimiterInner {
    /// Maps client_id -> sorted list of request timestamps (newest last).
    entries: HashMap<String, Vec<tokio::time::Instant>>,
}

/// Sliding-window rate limiter using `tokio::time::Instant` for testable time control.
///
/// Each client has their own request history within a [`WINDOW_SECS`]-second
/// sliding window. The maximum request count is passed per call so different
/// surfaces (pre-auth by IP, authenticated by PID) enforce different budgets
/// against one shared limiter instance.
#[derive(Clone)]
pub struct SlidingWindowRateLimiter {
    inner: Arc<RwLock<RateLimiterInner>>,
    window_secs: u64,
}

impl SlidingWindowRateLimiter {
    /// Create a new rate limiter over a 60-second window.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(RateLimiterInner {
                entries: HashMap::new(),
            })),
            window_secs: WINDOW_SECS,
        }
    }

    /// Check whether a request from `client_id` is allowed under a budget of
    /// `max_requests` within the window.
    ///
    /// Prunes stale entries (older than the window), then checks the client's
    /// in-window count. If allowed, records the current timestamp. When rejected,
    /// returns [`RateLimitDecision::Limited`] carrying the number of seconds until
    /// the oldest in-window request expires and a slot frees up.
    pub async fn check(&self, client_id: &str, max_requests: u32) -> RateLimitDecision {
        let now = tokio::time::Instant::now();
        let window = std::time::Duration::from_secs(self.window_secs);

        let mut inner = self.inner.write().await;
        let timestamps = inner.entries.entry(client_id.to_string()).or_default();

        // Prune stale entries (older than window). On Windows `tokio::time::Instant`
        // is anchored at system boot, so within the first `window` after boot
        // `now - window` underflows and panics. In that case no entry can be stale
        // yet, so skip pruning instead of subtracting.
        if let Some(cutoff) = now.checked_sub(window) {
            timestamps.retain(|&t| t > cutoff);
        }

        if timestamps.len() as u32 >= max_requests {
            // A slot frees up when the oldest in-window request ages out. Round up
            // so the advertised retry is never short of when capacity returns, and
            // floor at 1 so we never advertise an immediate (0s) retry.
            let retry_after_secs = timestamps
                .first()
                .map(|&oldest| {
                    let remaining = window.saturating_sub(now.saturating_duration_since(oldest));
                    let mut secs = remaining.as_secs();
                    if remaining.subsec_nanos() > 0 {
                        secs += 1;
                    }
                    secs.max(1)
                })
                .unwrap_or(self.window_secs);
            return RateLimitDecision::Limited { retry_after_secs };
        }

        // Record this request
        timestamps.push(now);
        RateLimitDecision::Allowed
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
    use std::time::Duration;

    /// Build a limiter whose window is larger than any possible process uptime,
    /// so `now - window` underflows `Instant` deterministically. This reproduces
    /// the Windows boot-time crash (where `tokio::time::Instant::now()` is anchored
    /// at boot and is smaller than the window) without depending on real uptime.
    fn limiter_with_overflowing_window() -> SlidingWindowRateLimiter {
        SlidingWindowRateLimiter {
            inner: Arc::new(RwLock::new(RateLimiterInner {
                entries: HashMap::new(),
            })),
            window_secs: u64::MAX,
        }
    }

    #[tokio::test]
    async fn check_does_not_panic_when_window_exceeds_uptime() {
        let limiter = limiter_with_overflowing_window();
        // Must not panic on `now - window` underflow, and the request is allowed.
        assert_eq!(
            limiter.check("client-a", AUTHENTICATED_MAX_REQUESTS).await,
            RateLimitDecision::Allowed
        );
    }

    #[tokio::test]
    async fn cleanup_stale_does_not_panic_when_window_exceeds_uptime() {
        let limiter = limiter_with_overflowing_window();
        limiter.check("client-a", AUTHENTICATED_MAX_REQUESTS).await;
        // Must not panic on `now - window` underflow.
        limiter.cleanup_stale().await;
    }

    #[tokio::test(start_paused = true)]
    async fn blocks_after_budget_and_reports_computed_retry_after() {
        let limiter = SlidingWindowRateLimiter::new();
        let max = 5;

        // Fill the budget exactly.
        for _ in 0..max {
            assert_eq!(
                limiter.check("pid-1", max).await,
                RateLimitDecision::Allowed
            );
        }

        // One over budget: rejected. The oldest request was just made, so the
        // computed retry is the full window.
        assert_eq!(
            limiter.check("pid-1", max).await,
            RateLimitDecision::Limited {
                retry_after_secs: WINDOW_SECS
            }
        );

        // Once the window elapses the budget frees up again.
        tokio::time::advance(Duration::from_secs(WINDOW_SECS + 1)).await;
        assert_eq!(
            limiter.check("pid-1", max).await,
            RateLimitDecision::Allowed
        );
    }

    #[tokio::test(start_paused = true)]
    async fn retry_after_shrinks_as_oldest_request_ages() {
        let limiter = SlidingWindowRateLimiter::new();
        let max = 2;

        assert_eq!(
            limiter.check("pid-1", max).await,
            RateLimitDecision::Allowed
        );
        // Age the first request halfway through the window before filling up.
        tokio::time::advance(Duration::from_secs(WINDOW_SECS - 10)).await;
        assert_eq!(
            limiter.check("pid-1", max).await,
            RateLimitDecision::Allowed
        );

        // Now over budget: the oldest request expires in ~10s, not a full window.
        assert_eq!(
            limiter.check("pid-1", max).await,
            RateLimitDecision::Limited {
                retry_after_secs: 10
            }
        );
    }

    #[tokio::test(start_paused = true)]
    async fn budgets_are_independent_per_client() {
        let limiter = SlidingWindowRateLimiter::new();
        for _ in 0..3 {
            assert_eq!(limiter.check("a", 3).await, RateLimitDecision::Allowed);
        }
        assert!(matches!(
            limiter.check("a", 3).await,
            RateLimitDecision::Limited { .. }
        ));
        // A different client keyed separately is unaffected.
        assert_eq!(limiter.check("b", 3).await, RateLimitDecision::Allowed);
    }
}
