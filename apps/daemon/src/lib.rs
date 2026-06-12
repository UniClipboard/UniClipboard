//! `uc-daemon` — UniClipboard GUI-agnostic daemon runtime library + `uniclipd`
//! binary.
//!
//! Hosts the full daemon runtime: run_mode, workers, assembly chain, main loop,
//! startup recovery, process bootstrap, and host entry points (`run` /
//! `start_in_process`). The `uniclipd` binary target is a thin wrapper that
//! delegates to [`daemon::host::run_standalone_from_env`].
//!
//! **Hard constraint**: no GUI / UI framework dependencies, and no reverse
//! dependency on `uc-desktop`. As of the ADR-008 dependency-edge cleanup the
//! GUI no longer depends on this crate at all (the lone shared type,
//! `DaemonOwnership`, was sunk into `uc-desktop`), so building the GUI no
//! longer compiles the daemon runtime tree.
//!
//! See `docs/architecture/adr-008-uniclipd-split-gui-as-client.md`.

pub mod daemon;

pub use daemon::host::{
    run_standalone_from_env, ProcessRuntimeHandles, RUN_MODE_ENV, RUN_MODE_ONESHOT, RUN_MODE_SERVER,
};
pub use daemon::process_bootstrap::{build_process_runtime, ProcessRuntimeContext};
pub use daemon::run_mode;
pub use daemon::DaemonHandle;
