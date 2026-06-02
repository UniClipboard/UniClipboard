//! Process-level runtime assembly — re-exported from [`uc_daemon`] (ADR-008 P2).
//!
//! The implementation migrated to `uc-daemon::daemon::process_bootstrap` in
//! Slice 2a. This module preserves the `uc_desktop::bootstrap::*` import paths
//! for backward compatibility with `uc-tauri` and other consumers.

pub use uc_daemon::daemon::process_bootstrap::{build_process_runtime, ProcessRuntimeContext};
