//! Startup orchestration.
//!
//! One-shot coordination that runs *after* dependency wiring, driving the
//! already-wired ports to reconcile persisted state and apply deferred
//! imports. It sits between pure wiring and use cases; by decision it stays
//! in the composition root as a pragmatic extension (see
//! `docs/architecture/uc-bootstrap-redesign.md` §2.1), not as use cases.

/// Idempotent reconciliation of persisted peer/trust state on each daemon
/// start. Crate-internal: invoked once from the composition-root entry.
pub(crate) mod reconcile;

/// Deferred config-import application staged by a prior `.ucbundle` import.
pub mod pending_import;
