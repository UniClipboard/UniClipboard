//! Whether the current GUI launch spawned the daemon or attached to one that
//! was already running (GUI-framework agnostic).
//!
//! [`DaemonOwnership`](super::DaemonOwnership) records *that* we are attached
//! to an external daemon, but it collapses two launch shapes that Lightweight
//! Mode (issue #1169) must tell apart:
//!
//! - **SpawnedThisLaunch** — this process found no daemon (or an incompatible
//!   one) and spawned it. This is a genuine cold start: the shell runs its
//!   cold-launch actions (auto-unlock, restore-last-entry) and, under
//!   Lightweight Mode, hands off to background-only running.
//! - **AlreadyRunning** — a compatible daemon was already up (e.g. a previous
//!   Lightweight-Mode session left it running, or the CLI started it), so this
//!   process only connected. This launch is a *reopen*: it must NOT re-run the
//!   cold-launch restore (that would clobber the OS clipboard on every window
//!   reopen), and under Lightweight Mode it shows the window instead of exiting.
//!
//! The daemon-probe path records the origin here before publishing the daemon
//! connection, so any task that waits on the connection observes a settled
//! origin. Kept separate from `DaemonOwnership` to avoid overloading that
//! type's deliberately minimal attach-state semantics.

use std::sync::{Arc, Mutex};

/// How the daemon came to be running for the current GUI launch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum LaunchOriginState {
    /// Not yet determined — the probe has not classified the daemon yet.
    #[default]
    Unknown,
    /// This launch spawned the daemon (it was absent or incompatible).
    SpawnedThisLaunch,
    /// A compatible daemon was already running; this launch only connected.
    AlreadyRunning,
}

/// Shareable handle to the current launch's daemon origin.
///
/// `Clone` + inner `Arc<Mutex<...>>` — the shell stores it in Tauri `manage`
/// state and/or clones it into the probe and cold-launch tasks; all clones
/// observe the same state.
#[derive(Clone, Default)]
pub struct DaemonLaunchOrigin(Arc<Mutex<LaunchOriginState>>);

impl DaemonLaunchOrigin {
    fn set(&self, state: LaunchOriginState) {
        let mut guard = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = state;
    }

    /// Record that this launch spawned the daemon (absent / incompatible).
    pub fn mark_spawned(&self) {
        self.set(LaunchOriginState::SpawnedThisLaunch);
    }

    /// Record that a compatible daemon was already running and this launch
    /// only connected to it.
    pub fn mark_already_running(&self) {
        self.set(LaunchOriginState::AlreadyRunning);
    }

    /// Whether this launch spawned the daemon (a genuine cold start). `false`
    /// for both `AlreadyRunning` and the `Unknown` default, so a caller that
    /// races ahead of the probe conservatively treats the launch as a reopen
    /// (skips the clipboard-clobbering restore) rather than a cold start.
    pub fn spawned_this_launch(&self) -> bool {
        let guard = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        matches!(*guard, LaunchOriginState::SpawnedThisLaunch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_origin_is_not_a_spawn() {
        assert!(!DaemonLaunchOrigin::default().spawned_this_launch());
    }

    #[test]
    fn mark_spawned_reports_cold_start() {
        let origin = DaemonLaunchOrigin::default();
        origin.mark_spawned();
        assert!(origin.spawned_this_launch());
    }

    #[test]
    fn mark_already_running_reports_reopen() {
        let origin = DaemonLaunchOrigin::default();
        origin.mark_already_running();
        assert!(!origin.spawned_this_launch());
    }

    #[test]
    fn clone_shares_underlying_state() {
        // The shell clones the handle into the probe task and the cold-launch
        // task; a clone must observe the origin the probe recorded.
        let probe_side = DaemonLaunchOrigin::default();
        let cold_launch_side = probe_side.clone();
        probe_side.mark_spawned();
        assert!(
            cold_launch_side.spawned_this_launch(),
            "clone must observe mark_spawned via the shared Arc"
        );
    }
}
