//! Permission level definitions for daemon HTTP API routes.
//!
//! Phase 76 extends with L3 (sensitive) and L4 (dangerous) permission levels
//! to enforce granular access control based on encryption state and route sensitivity.

/// Permission level for daemon HTTP API routes.
///
/// Phase 76 scope:
/// - L1: Public endpoint — no authentication required (e.g., `/health`).
/// - L2: Authenticated endpoint — valid session token + PID whitelist required.
/// - L3: Sensitive endpoint — requires encryption layer to be initialized.
/// - L4: Dangerous endpoint — requires explicit admin capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionLevel {
    /// L1 — Public endpoint. No authentication required.
    L1Public = 1,
    /// L2 — Authenticated endpoint. Valid session token + PID whitelist required.
    L2Authenticated = 2,
    /// L3 — Sensitive endpoint. Encryption layer must be initialized.
    L3Sensitive = 3,
    /// L4 — Dangerous endpoint. Explicit admin capability required.
    L4Dangerous = 4,
}

impl PermissionLevel {
    /// Convert a raw u8 value to a PermissionLevel.
    ///
    /// Returns `Some` for L1 (1), L2 (2), L3 (3), and L4 (4).
    /// Returns `None` for all other values.
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::L1Public),
            2 => Some(Self::L2Authenticated),
            3 => Some(Self::L3Sensitive),
            4 => Some(Self::L4Dangerous),
            _ => None,
        }
    }
}
