//! `uc-daemon` — UniClipboard GUI-agnostic daemon runtime library + `uniclipd`
//! binary.
//!
//! Hosts the full daemon runtime: run_mode, workers, assembly chain, main loop,
//! startup recovery, process bootstrap, and host entry points (`run` /
//! `start_in_process`). The `uniclipd` binary target is a thin wrapper that
//! delegates to [`daemon::host::run_standalone_from_env`].
//!
//! **Hard constraint**: no GUI / UI framework dependencies. `uc-desktop`
//! depends on this crate (forward dep), not the other way around.
//!
//! See `docs/architecture/adr-008-uniclipd-split-gui-as-client.md`.

pub mod daemon;

pub use daemon::host::{
    run_standalone_from_env, ProcessRuntimeHandles, RUN_MODE_ENV, RUN_MODE_SERVER,
};
pub use daemon::process_bootstrap::{build_process_runtime, ProcessRuntimeContext};
pub use daemon::run_mode;
pub use daemon::DaemonHandle;
pub use daemon::DaemonOwnership;
