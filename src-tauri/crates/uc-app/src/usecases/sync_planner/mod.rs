//! OutboundSyncPlanner module.
//!
//! Consolidates all outbound sync eligibility decisions (settings load, file size filtering,
//! transfer_id generation, and all_files_excluded guard) into a single `plan()` call.

mod planner;
mod types;

pub use planner::OutboundSyncPlanner;
pub use types::{ClipboardSyncIntent, FileCandidate, FileSyncIntent, OutboundSyncPlan};
