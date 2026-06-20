//! Active-clipboard state use cases.
//!
//! The cross-device last-writer-wins clipboard register and the workers
//! that drive it: inbound apply, fan-out, peer-online resync, restore
//! broadcast, pull serving, and reconciliation. Grouped under one module
//! so the active-state subsystem stays together instead of being
//! flattened across `clipboard_sync` with an `active_state_` name prefix.
//!
//! The shared gating / snapshot helpers (`send_gate`, `receive_gate`,
//! `snapshot_from_entry`) deliberately stay in the parent module: they are
//! also used by the generic inbound / resend use cases, so they are not
//! part of this subsystem.

pub(crate) mod apply_inbound;
pub(crate) mod fanout;
pub(crate) mod peer_online_resync_worker;
pub(crate) mod reconcile;
pub(crate) mod restore_broadcast_worker;
pub(crate) mod serve_pull;
