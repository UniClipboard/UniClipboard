//! 出站同步规划。

mod planner;
mod types;

pub use planner::OutboundSyncPlanner;
pub use types::{ClipboardSyncIntent, FileCandidate, FileSyncIntent, OutboundSyncPlan};
