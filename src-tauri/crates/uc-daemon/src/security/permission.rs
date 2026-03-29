//! Permission level definitions for daemon HTTP API routes.
//!
//! Phase 75 defines only L1 (public) and L2 (authenticated) permission levels.
//! L3/L4 values are reserved for future phases that will enforce granular
//! access control based on encryption state and route sensitivity.

/// Permission level for daemon HTTP API routes.
///
/// Phase 75 scope:
/// - L1: Public endpoint — no authentication required (e.g., `/health`).
/// - L2: Authenticated endpoint — valid JWT + PID whitelist required.
///
/// L3 and L4 are intentionally absent from Phase 75. Future phases will
/// add L3 (sensitive — requires encryption initialized) and L4 (dangerous —
/// requires explicit admin capability).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionLevel {
    /// L1 — Public endpoint. No authentication required.
    L1Public = 1,
    /// L2 — Authenticated endpoint. Valid session token + PID whitelist required.
    L2Authenticated = 2,
}

impl PermissionLevel {
    /// Convert a raw u8 value to a PermissionLevel.
    ///
    /// Returns `Some` for L1 (1) and L2 (2). Returns `None` for L3/L4 (3-4)
    /// and all other values since Phase 75 does not define them.
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::L1Public),
            2 => Some(Self::L2Authenticated),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_u8_l1() {
        assert_eq!(PermissionLevel::from_u8(1), Some(PermissionLevel::L1Public));
    }

    #[test]
    fn from_u8_l2() {
        assert_eq!(
            PermissionLevel::from_u8(2),
            Some(PermissionLevel::L2Authenticated)
        );
    }

    #[test]
    fn from_u8_l3_returns_none() {
        assert_eq!(PermissionLevel::from_u8(3), None);
    }

    #[test]
    fn from_u8_l4_returns_none() {
        assert_eq!(PermissionLevel::from_u8(4), None);
    }

    #[test]
    fn from_u8_invalid_values() {
        assert_eq!(PermissionLevel::from_u8(0), None);
        assert_eq!(PermissionLevel::from_u8(5), None);
        assert_eq!(PermissionLevel::from_u8(99), None);
    }
}
