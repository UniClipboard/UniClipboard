//! # uc-daemon-process
//!
//! Thin, dependency-light primitives for managing the local `uniclipd` daemon
//! **process**: its PID-file metadata, loopback socket/token paths, detached
//! spawn, and the CLI↔daemon spawn contract.
//!
//! Extracted from `uc-daemon-local` (ADR-008 P5-0) so the daemon **client**
//! stack (`uc-daemon-client`, `uc-cli`) can depend on these primitives without
//! transitively pulling in `uc-application` → `uc-infra` → `iroh`/`diesel`.
//! Every module here depends on **only** lightweight crates (`uc-app-paths`,
//! `uc-daemon-contract`, `libc`, `which`, `serde`, `serde_json`, `anyhow`,
//! `thiserror`, `tokio`, `tracing`) plus `std`. The app
//! data-root resolution is delegated to `uc-app-paths`, the directory-layout
//! authority `uc-platform` also consumes, so the PID/token paths stay
//! byte-identical with no second copy (ADR-008 P5-0c).
//!
//! `uc-daemon-local` reverse-depends on this crate and re-exports these modules
//! (`pub use uc_daemon_process::{contract, handover, health_wait, process_metadata, socket, spawn, spawn_contract}`),
//! so every existing `uc_daemon_local::<module>::*` path keeps resolving
//! unchanged.
//!
//! - [`contract`]: probe outcome classifier, bootstrap/termination error types,
//!   and `terminate_local_daemon_pid` helper.
//! - [`handover`]: cross-process controlled-restart handover store.
//! - [`health_wait`]: async polling helpers that wait for daemon health or
//!   endpoint absence.
//! - [`process_metadata`]: PID-file read/write + `DaemonProcessMode`.
//! - [`socket`]: loopback HTTP address + daemon token path resolution.
//! - [`spawn`]: `uniclipd` detached spawn (`setsid` / `DETACHED_PROCESS`).
//! - [`spawn_contract`]: CLI→daemon run-mode / unattended-unlock env contract.
//! - [`timing`]: cross-process timing contract for the daemon stop → start
//!   handoff (base durations + derived wait budgets in one place).

pub mod contract;
pub mod handover;
pub mod health_wait;
pub mod process_metadata;
pub mod socket;
pub mod spawn;
pub mod spawn_contract;
pub mod timing;
#[cfg(windows)]
pub(crate) mod win_console;
#[cfg(windows)]
pub(crate) mod win_process;
