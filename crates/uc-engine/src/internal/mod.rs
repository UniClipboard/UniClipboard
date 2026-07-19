//! Internal assembly modules exposed temporarily while desktop consumers move
//! to the stable `Engine` interface.

pub mod blob_tasks;
pub mod clipboard;
pub mod deps;
pub mod file_transfer;
pub(crate) mod inbound_staging;
pub mod lifecycle;
pub mod network_policy;
pub mod platform;
pub mod reconcile;
pub mod sync_engine;
pub mod wire;
