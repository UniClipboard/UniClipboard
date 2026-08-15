//! Black-box E2E test harness for UniClipboard.
//!
//! Provides `TestDaemon` (lifecycle management for `uniclipd`) and `TestCli`
//! (ergonomic command builder for `uniclip`) — both profile-isolated so tests
//! can run in parallel without interference.

mod auth;
mod binaries;
mod cli;
mod daemon;
#[cfg(test)]
mod member_removal;
mod pairing;
mod profile;
mod releases;
mod rendezvous;

pub use auth::{get_session_token, read_daemon_file_token};
pub use binaries::NodeBinarySet;
pub use cli::{CapturedOutput, TestCli};
pub use daemon::TestDaemon;
pub use pairing::{
    invite_join_round, invite_switch_round, pair_two_nodes, setup_initialized_node, InviteSession,
};
pub use profile::TestProfile;
pub use releases::{
    checksum_for_asset, extract_release_archive, fixed_legacy_release_asset,
    prepare_fixed_legacy_release_from, prepare_v0_19_1_release_from, v0_19_1_release_asset,
    verify_release_payload, ArchiveFormat, ReleaseAsset, LEGACY_RELEASE_BASE_URL,
    LEGACY_RELEASE_TAG, LEGACY_RELEASE_VERSION, LEGACY_SHA256SUMS_SHA256, V0_19_1_RELEASE_TAG,
    V0_19_1_RELEASE_VERSION, V0_19_1_SHA256SUMS_SHA256,
};
pub use rendezvous::LocalRendezvous;
