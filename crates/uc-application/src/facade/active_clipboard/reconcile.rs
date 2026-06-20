//! `ActiveClipboardReconcileFacade` — startup entry point for reconciling the
//! persisted active-clipboard register against the live OS clipboard (issue
//! #1017 §6.6, D8).
//!
//! This is a separate, lightweight facade from [`ActiveClipboardFacade`]
//! (`super`): reconcile must run **once at startup, before** the inbound and
//! peer-online resync workers are spawned, so that a stale persisted row is
//! never read or broadcast. Those workers are created together with
//! `ActiveClipboardFacade` deeper in assembly; reconcile is therefore wired and
//! driven earlier and on its own.

use std::sync::Arc;

use uc_core::ports::clipboard::{
    LoadActiveClipboardPort, ResetActiveClipboardPort, SystemClipboardPort,
};

use super::ClipboardSnapshotDeps;
use crate::usecases::clipboard_sync::active_state::reconcile::{
    ReconcileActiveClipboardStateUseCase, ReconcileOutcome,
};

pub use crate::usecases::clipboard_sync::active_state::reconcile::ReconcileOutcome as ActiveClipboardReconcileOutcome;

/// Dependencies for [`ActiveClipboardReconcileFacade`].
pub struct ActiveClipboardReconcileDeps {
    pub system_clipboard: Arc<dyn SystemClipboardPort>,
    pub load_register: Arc<dyn LoadActiveClipboardPort>,
    pub reset_register: Arc<dyn ResetActiveClipboardPort>,
    /// Snapshot reconstruction ports (shared with restore / resend). The
    /// reconcile rebuilds the stored row's entry into the snapshot a restore
    /// would place on the OS clipboard, so it can compare like-for-like against
    /// the live OS read rather than against the row's persisted (cross-device)
    /// `snapshot_hash`, which diverges for file entries.
    pub snapshot: ClipboardSnapshotDeps,
}

/// Thin facade over the startup reconcile use case.
pub struct ActiveClipboardReconcileFacade {
    uc: ReconcileActiveClipboardStateUseCase,
}

impl ActiveClipboardReconcileFacade {
    pub fn new(deps: ActiveClipboardReconcileDeps) -> Self {
        Self {
            uc: ReconcileActiveClipboardStateUseCase::new(
                deps.system_clipboard,
                deps.load_register,
                deps.reset_register,
                deps.snapshot.into_reconstructor(),
            ),
        }
    }

    /// Reconcile the persisted register against the live OS clipboard once.
    ///
    /// Keeps the persisted row when it still matches the OS clipboard, clears
    /// it otherwise (an untrusted/stale row must not act as the active
    /// baseline). Never writes the OS clipboard and never broadcasts;
    /// best-effort, so it returns the outcome but cannot fail the caller.
    /// Drive this once at startup, before any worker reads or broadcasts the
    /// register.
    pub async fn reconcile(&self) -> ReconcileOutcome {
        self.uc.run().await
    }
}
