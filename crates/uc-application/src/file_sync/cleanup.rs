//! `check_device_quota` is the only surviving artefact in this module.
//! The expired-files cleanup use case moved to
//! `usecases::clipboard_history::cleanup` so it can route through the
//! entry-aware delete path.
//!
//! `check_device_quota` is currently uncalled — it computed a per-peer
//! cache footprint based on a wrong path layout
//! (`cache_dir/<source_device_id>` vs the real
//! `cache_dir/iroh-blobs/<entry_id>`) and was never re-wired after the
//! file-cache layout changed. Phase A made the layout question even
//! more divergent (file_cache_dir moved to `~/Library/Application Support`).
//! Kept here under `#[allow(dead_code)]` so an eventual quota feature
//! has a starting point; remove if/when a follow-up rewrites it.

#![allow(dead_code)]

use std::path::Path;

use uc_core::ports::SettingsPort;

/// Check if accepting a file would exceed the per-device cache quota.
///
/// Currently dead — see module docs.
pub async fn check_device_quota(
    settings: &dyn SettingsPort,
    cache_dir: &Path,
    source_device_id: &str,
    incoming_file_size: u64,
) -> std::result::Result<(), QuotaExceededError> {
    let s = settings
        .load()
        .await
        .map_err(|e| QuotaExceededError::Internal(e.to_string()))?;

    let quota_bytes = s.file_sync.file_cache_quota_per_device;

    let peer_cache_dir = cache_dir.join(source_device_id);
    let current_usage = if peer_cache_dir.exists() {
        dir_size(&peer_cache_dir).unwrap_or(0)
    } else {
        0
    };

    if current_usage.saturating_add(incoming_file_size) > quota_bytes {
        return Err(QuotaExceededError::Exceeded {
            device_id: source_device_id.to_string(),
            current_usage,
            quota: quota_bytes,
            requested: incoming_file_size,
        });
    }

    Ok(())
}

fn dir_size(path: &Path) -> anyhow::Result<u64> {
    let mut total = 0u64;
    if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let meta = entry.metadata()?;
            if meta.is_file() {
                total += meta.len();
            } else if meta.is_dir() {
                total += dir_size(&entry.path()).unwrap_or(0);
            }
        }
    }
    Ok(total)
}

#[derive(Debug, thiserror::Error)]
pub enum QuotaExceededError {
    #[error("Cache quota exceeded for device {device_id}: {current_usage}/{quota} bytes used, requested {requested} bytes")]
    Exceeded {
        device_id: String,
        current_usage: u64,
        quota: u64,
        requested: u64,
    },
    #[error("Internal error checking quota: {0}")]
    Internal(String),
}
