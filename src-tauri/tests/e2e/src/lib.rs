//! Black-box E2E test harness for UniClipboard.
//!
//! Provides `TestDaemon` (lifecycle management for `uniclipd`) and `TestCli`
//! (ergonomic command builder for `uniclip`) — both profile-isolated so tests
//! can run in parallel without interference.

mod cli;
mod daemon;
mod profile;

pub use cli::TestCli;
pub use daemon::TestDaemon;
pub use profile::TestProfile;
