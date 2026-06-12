//! `SetupStatusFacade` — setup-status query + command entry point.
//!
//! Thin aggregator over the `SetupStatusPort`-backed use cases:
//!
//! * [`SetupStatusFacade::is_complete`] — "has Space setup completed?"
//! * [`SetupStatusFacade::mark_complete`] — flip the completion flag.
//!
//! Per `uc-application/AGENTS.md` §11.4 external consumers (bootstrap,
//! CLI, future GUI) may only reach the underlying use cases through this
//! facade; the use case types themselves stay `pub(crate)`.

mod facade;

pub use facade::SetupStatusFacade;
