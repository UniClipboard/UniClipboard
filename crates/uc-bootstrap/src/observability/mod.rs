//! Sentry-layer composition owned by the composition root.
//!
//! `uc-observability` stays sink-agnostic (console/json layers, profiles,
//! redaction); the Sentry tracing layer and its cross-device correlation
//! enrichment live here because they depend on `sentry::protocol::*` and on
//! the wired device identity. See `docs/architecture/uc-bootstrap-redesign.md`
//! §2.1 (Phase 3a decision).

pub mod tracing;

/// Sentry-sink correlation enrichment, consumed only by `tracing`'s
/// `event_mapper`.
mod correlation;

/// The two `uc_observability::telemetry_gate` attachment points that are not
/// payload hooks: the transaction sampler and the outbound envelope transport.
/// Both are consumed only by `tracing`'s `sentry::init` call.
mod sentry_gate;

/// Default host-event transport (`LoggingHostEventEmitter`) for non-GUI / CLI
/// processes, pre-registered on the shared host-event bus at wire time. Lives
/// here — below the entrypoint layer — so the common wiring root stays
/// independent of any specific scenario entrypoint.
pub(crate) mod host_event;
