//! Black-box E2E test harness for UniClipboard.
//!
//! Provides `TestDaemon` (lifecycle management for `uniclipd`) and `TestCli`
//! (ergonomic command builder for `uniclip`) — both profile-isolated so tests
//! can run in parallel without interference.

mod auth;
mod cli;
mod daemon;
mod pairing;
mod profile;

pub use auth::{get_session_token, read_daemon_file_token};
pub use cli::{CapturedOutput, TestCli};
pub use daemon::TestDaemon;
pub use pairing::{
    invite_join_round, invite_switch_round, pair_two_nodes, setup_initialized_node, InviteSession,
};
pub use profile::TestProfile;
