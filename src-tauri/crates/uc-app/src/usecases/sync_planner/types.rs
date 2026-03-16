//! Types for outbound sync planning.

use std::path::PathBuf;

use uc_core::{network::protocol::FileTransferMapping, SystemClipboardSnapshot};

/// A file candidate pre-computed by the runtime.
///
/// The runtime extracts file paths from the clipboard snapshot, resolves any platform-specific
/// file references (e.g., APFS), and retrieves file sizes via `std::fs::metadata()`. The planner
/// receives these pre-computed candidates and performs PURE LOGIC filtering — no filesystem I/O.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileCandidate {
    /// Resolved absolute path to the file.
    pub path: PathBuf,
    /// File size in bytes, pre-computed by the runtime via `std::fs::metadata()`.
    pub size: u64,
}

/// Intent to sync clipboard content to peers.
///
/// Includes the snapshot to broadcast and any associated file transfer mappings
/// so the receiver can pre-compute local cache paths before files arrive.
#[derive(Debug, Clone)]
pub struct ClipboardSyncIntent {
    /// The clipboard snapshot to broadcast.
    pub snapshot: SystemClipboardSnapshot,
    /// File transfer mappings carried inside the clipboard message.
    pub file_transfers: Vec<FileTransferMapping>,
}

/// Intent to sync a single file to peers.
#[derive(Debug, Clone)]
pub struct FileSyncIntent {
    /// Resolved absolute path to the file to transfer.
    pub path: PathBuf,
    /// Unique identifier for this file transfer.
    pub transfer_id: String,
    /// The original filename (used for naming the local cache entry on the receiver).
    pub filename: String,
}

/// The result of `OutboundSyncPlanner::plan()`.
///
/// Describes what should be synced outbound for a given clipboard change event.
///
/// - `clipboard: None` means clipboard sync is suppressed for this event.
/// - `files: []` means no file transfers should be initiated.
#[derive(Debug, Clone)]
pub struct OutboundSyncPlan {
    /// The clipboard sync intent, or `None` if clipboard sync is suppressed.
    pub clipboard: Option<ClipboardSyncIntent>,
    /// The list of file sync intents (may be empty).
    pub files: Vec<FileSyncIntent>,
}
