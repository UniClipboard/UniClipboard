//! Setup domain module.
//!
//! Only `SetupStatus` (the persistable completion flag, data contract for
//! `SetupStatusPort`) lives here. The rest of the setup flow — state
//! machine, events, actions, errors, event port — moved to
//! `uc-application::setup`, since `uc-core/AGENTS.md` §9.1 puts setup
//! flow orchestration outside core.

pub mod migration;
pub mod status;

pub use migration::{MigrationPhase, MigrationRunId};
pub use status::SetupStatus;
