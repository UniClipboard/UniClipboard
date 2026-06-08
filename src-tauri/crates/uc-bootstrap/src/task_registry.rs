//! Re-export of [`uc_core::task_registry`] for backwards compatibility.
//!
//! The canonical implementation now lives in `uc-core` (feature `task-registry`).
//! This module re-exports everything so existing consumers of `uc_bootstrap::TaskRegistry`
//! continue to compile without changes.

pub use uc_core::task_registry::*;
