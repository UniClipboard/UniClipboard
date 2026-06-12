//! Local device identity implementation.
//!
//! This module provides a filesystem-based persistence layer for the device identity.
//! The device ID is stored as a plain text UUID in the application data directory.
//!
//! ## Architecture Notes
//!
//! - **No Repository pattern needed**: DeviceId is a singleton, not a collection
//! - **Port in core, implementation in infra**: `DeviceIdentityPort` is defined in uc-core
//! - **Fail-fast on init**: If we can't load/create the ID, the app should not start
//! - **Immutable once created**: DeviceId never changes for the lifetime of the installation

mod storage;

use anyhow::Result;
use std::path::PathBuf;
use uc_core::ids::DeviceId;
use uc_core::ports::DeviceIdentityPort;

/// Local filesystem-backed device identity.
///
/// This struct implements `DeviceIdentityPort` by storing the device ID
/// as a plain text file in the application data directory.
pub struct LocalDeviceIdentity {
    device_id: DeviceId,
}

impl LocalDeviceIdentity {
    /// Load existing device ID or create a new one.
    ///
    /// This is the primary entry point for obtaining the device identity.
    /// It will:
    /// 1. Try to load from disk
    /// 2. If not found, generate a new UUID v4 and persist it
    /// 3. Fail-fast on any I/O error (app should not start without valid identity)
    pub fn load_or_create(config_dir: PathBuf) -> Result<Self> {
        if let Some(id) = storage::load_from_disk(&config_dir)? {
            Ok(Self { device_id: id })
        } else {
            let id = DeviceId::new(uuid::Uuid::new_v4().to_string());
            storage::save_to_disk(&config_dir, &id)?;
            Ok(Self { device_id: id })
        }
    }
}

impl DeviceIdentityPort for LocalDeviceIdentity {
    fn current_device_id(&self) -> DeviceId {
        self.device_id.clone()
    }
}
