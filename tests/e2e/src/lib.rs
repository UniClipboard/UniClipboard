//! Black-box E2E test harness for UniClipboard.
//!
//! Provides `TestDaemon` (lifecycle management for `uniclipd`) and `TestCli`
//! (ergonomic command builder for `uniclip`) — both profile-isolated so tests
//! can run in parallel without interference.

mod auth;
mod binaries;
mod cli;
mod daemon;
mod pairing;
mod profile;
mod releases;
mod rendezvous;
mod upgrade_userdata;
mod upgrade_userdata_builder;

pub use auth::{get_session_token, read_daemon_file_token};
pub use binaries::{DaemonEndpointDiscovery, NodeBinarySet};
pub use cli::{CapturedOutput, TestCli};
pub use daemon::TestDaemon;
pub use pairing::{
    invite_join_round, invite_switch_round, pair_two_nodes, setup_initialized_node, InviteSession,
};
pub use profile::TestProfile;
pub use releases::{
    checksum_for_asset, extract_release_archive, fixed_legacy_release_asset,
    prepare_fixed_legacy_release_from, prepare_selected_upgrade_release_from,
    prepare_v0_19_1_release_from, selected_upgrade_release, v0_19_1_release_asset,
    verify_release_payload, ArchiveFormat, ReleaseAsset, UpgradeRelease, LEGACY_RELEASE_BASE_URL,
    LEGACY_RELEASE_TAG, LEGACY_RELEASE_VERSION, LEGACY_SHA256SUMS_SHA256, UPGRADE_RELEASES,
    V0_19_1_RELEASE_TAG, V0_19_1_RELEASE_VERSION, V0_19_1_SHA256SUMS_SHA256,
    V0_19_1_UPGRADE_RELEASE,
};
pub use rendezvous::LocalRendezvous;
pub use upgrade_userdata::{
    verify_upgrade_userdata_archive, UpgradeUserdataFixture, UpgradeUserdataManifest,
};
pub use upgrade_userdata_builder::build_single_node_upgrade_fixture;
