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
