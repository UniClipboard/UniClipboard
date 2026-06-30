//! Single-layer adapter assembly.
//!
//! Each module constructs one adapter layer the composition root wires
//! together: infrastructure (DB pool / repos / encryption / fs / timers),
//! platform (clipboard / secure storage), and directory-path resolution.

pub mod paths;
pub mod platform;
