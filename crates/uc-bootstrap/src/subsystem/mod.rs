//! Subsystem assembly fragments.
//!
//! Each module wires one self-contained engine the daemon runs on top of the
//! base infra/platform layers: the iroh sync engine, the file-transfer
//! lifecycle, blob-processing background tasks, and product analytics.

pub mod analytics;
pub mod blob_tasks;
pub mod file_transfer;
pub mod sync_engine;
