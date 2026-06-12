//! Network session identifier shared across pairing and space access flows.

/// 网络会话的唯一标识符
///
/// Used by pairing and space access flows to correlate protocol messages,
/// timers, and state transitions within a single session.
pub type SessionId = String;
