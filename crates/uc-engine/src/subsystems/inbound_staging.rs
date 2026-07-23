//! Startup sweep of the hidden areas where inbound directories are assembled.
//!
//! A directory receive builds its tree in a hidden area beside where the roots
//! will land, then publishes them in one atomic step. A process that dies
//! mid-receive never reaches that step, so the area survives with a partial
//! tree in it and no entry referring to it.
//!
//! Debris can therefore sit in two places, and both are scanned: the user's
//! configured save directory, and the managed per-entry cache parents used
//! when there is no save directory (or its volume cannot publish atomically).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use uc_core::ports::inbound_file_target::ResolveInboundSaveDirPort;

/// Remove assembly areas left behind by an interrupted receive.
///
/// Governance only: nothing here is fatal, because a leftover area costs disk
/// space rather than correctness — it is hidden, and unreachable from any
/// entry.
pub(crate) async fn sweep_inbound_staging(
    save_dir_resolver: Arc<dyn ResolveInboundSaveDirPort>,
    file_cache_dir: &Path,
) {
    let mut scan_dirs: Vec<PathBuf> = Vec::new();

    if let Some(save_dir) = save_dir_resolver.resolve_save_dir().await {
        scan_dirs.push(save_dir);
    }

    // The managed layout puts each entry's roots in its own directory, so the
    // areas are one level down rather than directly here.
    let managed = file_cache_dir.join("iroh-blobs");
    match tokio::fs::read_dir(&managed).await {
        Ok(mut entries) => loop {
            match entries.next_entry().await {
                Ok(Some(entry)) => {
                    if entry
                        .file_type()
                        .await
                        .map(|kind| kind.is_dir())
                        .unwrap_or(false)
                    {
                        scan_dirs.push(entry.path());
                    }
                }
                Ok(None) => break,
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        "could not enumerate managed cache while sweeping staging areas"
                    );
                    break;
                }
            }
        },
        // No cache dir yet (first run) is the normal case, not a problem.
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            tracing::warn!(error = %err, "could not open managed cache while sweeping staging areas");
        }
    }

    uc_application::facade::ClipboardSyncFacade::sweep_orphaned_inbound_staging(&scan_dirs).await;
}
