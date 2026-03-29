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

#[cfg(test)]
mod tests;
