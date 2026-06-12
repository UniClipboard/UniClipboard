//! Private storage implementation for device identity.
//!
//! This module handles the low-level file I/O for persisting the device ID.
//! It is not part of the public API of the device module.

use anyhow::{Context, Result};
use std::path::PathBuf;
use uc_core::ids::DeviceId;

const DEVICE_ID_FILE: &str = "device_id.txt";

/// Load device ID from disk, returning None if file doesn't exist.
///
/// This is a private implementation detail of the device module.
pub(crate) fn load_from_disk(config_dir: &PathBuf) -> Result<Option<DeviceId>> {
    let path = config_dir.join(DEVICE_ID_FILE);

    if !path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("read device_id file failed: {}", path.display()))?;

    let id_str = content.trim();
    if id_str.is_empty() {
        return Ok(None);
    }

    // Validate UUID format
    uuid::Uuid::parse_str(id_str)
        .with_context(|| format!("invalid device_id UUID in file: {}", path.display()))?;

    Ok(Some(DeviceId::new(id_str.to_string())))
}

/// Save device ID to disk, creating parent directory if needed.
///
/// This is a private implementation detail of the device module.
pub(crate) fn save_to_disk(config_dir: &PathBuf, id: &DeviceId) -> Result<()> {
    // Ensure parent directory exists
    std::fs::create_dir_all(config_dir)
        .with_context(|| format!("create config dir failed: {}", config_dir.display()))?;

    let path = config_dir.join(DEVICE_ID_FILE);

    // Try atomic write using temp file + rename first
    // If rename fails (e.g., cross-device link in CI environments), fall back to direct write
    let tmp_path = path.with_extension("txt.tmp");
    std::fs::write(&tmp_path, id.as_str())
        .with_context(|| format!("write temp device_id failed: {}", tmp_path.display()))?;

    match std::fs::rename(&tmp_path, &path) {
        Ok(_) => Ok(()),
        Err(rename_err) => {
            // Rename failed - likely cross-device link or permission issue
            // Fall back to direct write (non-atomic but better than failing)
            std::fs::write(&path, id.as_str()).with_context(|| {
                format!(
                    "direct write device_id failed after rename error ({}): {}",
                    rename_err,
                    path.display()
                )
            })?;
            // Clean up temp file if it still exists
            let _ = std::fs::remove_file(&tmp_path);
            Ok(())
        }
    }
}
