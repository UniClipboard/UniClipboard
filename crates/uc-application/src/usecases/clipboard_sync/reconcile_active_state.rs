//! `ReconcileActiveClipboardStateUseCase` — reconcile the persisted
//! active-clipboard register against the *actual* OS clipboard once at process
//! start (issue #1017 §6.6, D8).
//!
//! The persisted register row is only an **untrusted baseline**. Between two
//! runs the OS clipboard can change behind our back (another app, the user, a
//! reboot), so the stored row may no longer describe the content the OS
//! clipboard actually holds. The register's core invariant is `register ==
//! current OS clipboard` (D1); a row that violates it is dangerous, because its
//! `activated_at_ms` could still win the LWW order and suppress a real later
//! activation, and the outbound resync would propagate a state that does not
//! match this device's clipboard.
//!
//! Policy (D8): read the OS clipboard, compare its content hash to the stored
//! row, and
//!
//! * **keep** the row when it still matches the OS clipboard (the invariant
//!   holds — nothing to do); otherwise
//! * **clear** the register (reset to no value).
//!
//! Clearing — rather than re-stamping the row to the observed OS content — is
//! deliberate. A cleared register makes no claim about the active clipboard, so
//! it can never suppress a future legitimate state (with no row, any observed
//! state supersedes), and the outbound resync sends nothing. Re-stamping the
//! row to the observed content with a fresh local timestamp would instead
//! fabricate an activation this device never performed and let it win LWW on
//! peers — the exact regression this reconcile exists to prevent.
//!
//! Hard invariants (D8): this **never writes the OS clipboard** and **never
//! broadcasts**. It only repairs the local register's relationship to reality.
//! It is best-effort: any failure (OS read error, register I/O error) is logged
//! and leaves startup to proceed. When the OS clipboard cannot be trusted
//! (unreadable), the register is cleared rather than left asserting a possibly
//! stale row.

use std::sync::Arc;

use tracing::{debug, info, instrument, warn};

use uc_core::ports::clipboard::{
    LoadActiveClipboardPort, ResetActiveClipboardPort, SystemClipboardPort,
};

/// Outcome of a reconcile pass. Returned for observability / testing; callers
/// drive reconcile for its side effect on the register, not this value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconcileOutcome {
    /// The register was empty — nothing to reconcile.
    AlreadyEmpty,
    /// The stored row still matches the OS clipboard; the register was kept.
    Kept,
    /// The stored row did not match the OS clipboard (or the OS clipboard was
    /// unreadable / empty); the register was cleared.
    Cleared,
}

/// Reconciles the persisted active-clipboard register against the live OS
/// clipboard at startup. Holds only the three ports it needs (read OS, load
/// register, reset register) — no dispatch, no OS-write port — so the
/// no-broadcast / no-OS-write invariants are structural, not merely observed.
pub(crate) struct ReconcileActiveClipboardStateUseCase {
    system_clipboard: Arc<dyn SystemClipboardPort>,
    load_register: Arc<dyn LoadActiveClipboardPort>,
    reset_register: Arc<dyn ResetActiveClipboardPort>,
}

impl ReconcileActiveClipboardStateUseCase {
    pub(crate) fn new(
        system_clipboard: Arc<dyn SystemClipboardPort>,
        load_register: Arc<dyn LoadActiveClipboardPort>,
        reset_register: Arc<dyn ResetActiveClipboardPort>,
    ) -> Self {
        Self {
            system_clipboard,
            load_register,
            reset_register,
        }
    }

    /// Run one reconcile pass. Best-effort: returns the outcome but never
    /// propagates an error — startup must proceed regardless.
    #[instrument(name = "active_state.reconcile", skip_all)]
    pub(crate) async fn run(&self) -> ReconcileOutcome {
        // Load the persisted baseline first. An empty register already satisfies
        // the invariant (no claim about the active clipboard), so there is
        // nothing to reconcile and no need to even touch the OS clipboard.
        let stored = match self.load_register.load().await {
            Ok(Some(state)) => state,
            Ok(None) => {
                debug!("active state reconcile: register empty; nothing to reconcile");
                return ReconcileOutcome::AlreadyEmpty;
            }
            Err(err) => {
                // Can't read the baseline → can't decide it's still valid.
                // Clear it so a possibly-stale row can't win LWW or be resynced.
                warn!(error = %err, "active state reconcile: register load failed; clearing as untrusted");
                self.clear().await;
                return ReconcileOutcome::Cleared;
            }
        };

        // Read the actual OS clipboard. An unreadable clipboard means we cannot
        // confirm the stored row still matches reality, so we treat the row as
        // untrusted and clear it (prefer clearing over trusting a stale row).
        let os_hash = match self.system_clipboard.read_snapshot() {
            Ok(snapshot) => snapshot.snapshot_hash().to_string(),
            Err(err) => {
                warn!(error = %err, "active state reconcile: OS clipboard read failed; clearing register as untrusted");
                self.clear().await;
                return ReconcileOutcome::Cleared;
            }
        };

        if stored.content_hash == os_hash {
            // The persisted row still matches the OS clipboard: the invariant
            // holds, keep the row as the baseline.
            debug!(
                content_hash = %stored.content_hash,
                "active state reconcile: stored register matches OS clipboard; kept"
            );
            ReconcileOutcome::Kept
        } else {
            // Stale/untrusted: the OS clipboard holds different content (or is
            // empty) than the row claims. Clear so the row can neither win LWW
            // against a real later activation nor be resynced to peers.
            info!(
                stored_hash = %stored.content_hash,
                os_hash = %os_hash,
                "active state reconcile: stored register does not match OS clipboard; clearing"
            );
            self.clear().await;
            ReconcileOutcome::Cleared
        }
    }

    /// Best-effort unconditional clear; a reset failure is logged, not
    /// propagated (startup proceeds either way).
    async fn clear(&self) {
        if let Err(err) = self.reset_register.reset().await {
            warn!(error = %err, "active state reconcile: register reset failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;

    use uc_core::clipboard::{
        ActiveClipboardState, ObservedClipboardRepresentation, SystemClipboardSnapshot,
    };
    use uc_core::ids::{DeviceId, EntryId, FormatId, RepresentationId};
    use uc_core::ports::clipboard::ActiveClipboardRegisterError;
    use uc_core::MimeType;

    // ---- fakes / spies ------------------------------------------------------

    /// Build a single-text-rep snapshot whose content hash is deterministic in
    /// the text.
    fn text_snapshot(text: &str) -> SystemClipboardSnapshot {
        SystemClipboardSnapshot {
            ts_ms: 0,
            representations: vec![ObservedClipboardRepresentation::new(
                RepresentationId::from("rep-text"),
                FormatId::from("text"),
                Some(MimeType("text/plain".to_string())),
                text.as_bytes().to_vec(),
            )],
        }
    }

    /// OS clipboard whose read returns a fixed snapshot, an empty snapshot, or
    /// an error. `write_snapshot` panics — it must never be reached.
    enum FakeClipboard {
        Text(&'static str),
        Empty,
        ReadError,
    }
    impl SystemClipboardPort for FakeClipboard {
        fn read_snapshot(&self) -> anyhow::Result<SystemClipboardSnapshot> {
            match self {
                FakeClipboard::Text(t) => Ok(text_snapshot(t)),
                FakeClipboard::Empty => Ok(SystemClipboardSnapshot {
                    ts_ms: 0,
                    representations: vec![],
                }),
                FakeClipboard::ReadError => Err(anyhow::anyhow!("clipboard unreadable")),
            }
        }
        fn write_snapshot(&self, _snapshot: SystemClipboardSnapshot) -> anyhow::Result<()> {
            panic!("reconcile must never write the OS clipboard");
        }
    }

    struct FixedRegister(Option<ActiveClipboardState>);
    #[async_trait]
    impl LoadActiveClipboardPort for FixedRegister {
        async fn load(&self) -> Result<Option<ActiveClipboardState>, ActiveClipboardRegisterError> {
            Ok(self.0.clone())
        }
    }

    struct LoadErrors;
    #[async_trait]
    impl LoadActiveClipboardPort for LoadErrors {
        async fn load(&self) -> Result<Option<ActiveClipboardState>, ActiveClipboardRegisterError> {
            Err(ActiveClipboardRegisterError::Storage("load boom".into()))
        }
    }

    #[derive(Default)]
    struct ResetSpy {
        calls: AtomicUsize,
    }
    #[async_trait]
    impl ResetActiveClipboardPort for ResetSpy {
        async fn reset(&self) -> Result<(), ActiveClipboardRegisterError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    /// Build a state whose `content_hash` equals the snapshot hash of the given
    /// text — so a `FakeClipboard::Text(t)` reconcile sees a match.
    fn state_matching(text: &str) -> ActiveClipboardState {
        let hash = text_snapshot(text).snapshot_hash().to_string();
        ActiveClipboardState::new(hash, EntryId::new(), 1_000, DeviceId::new("self"))
    }

    fn build(
        clipboard: FakeClipboard,
        register: Option<ActiveClipboardState>,
    ) -> (ReconcileActiveClipboardStateUseCase, Arc<ResetSpy>) {
        let reset = Arc::new(ResetSpy::default());
        let uc = ReconcileActiveClipboardStateUseCase::new(
            Arc::new(clipboard),
            Arc::new(FixedRegister(register)),
            Arc::clone(&reset) as Arc<dyn ResetActiveClipboardPort>,
        );
        (uc, reset)
    }

    #[tokio::test]
    async fn empty_register_is_already_empty_and_does_not_reset() {
        let (uc, reset) = build(FakeClipboard::Text("anything"), None);
        assert_eq!(uc.run().await, ReconcileOutcome::AlreadyEmpty);
        assert_eq!(
            reset.calls.load(Ordering::SeqCst),
            0,
            "an empty register needs no reset"
        );
    }

    #[tokio::test]
    async fn matching_register_is_kept_and_not_reset() {
        let text = "hello world";
        let (uc, reset) = build(FakeClipboard::Text(text), Some(state_matching(text)));
        assert_eq!(uc.run().await, ReconcileOutcome::Kept);
        assert_eq!(
            reset.calls.load(Ordering::SeqCst),
            0,
            "a matching register must be kept, not cleared"
        );
    }

    #[tokio::test]
    async fn mismatched_register_is_cleared() {
        // Register claims content for "stored", OS clipboard holds "different".
        let (uc, reset) = build(
            FakeClipboard::Text("different"),
            Some(state_matching("stored")),
        );
        assert_eq!(uc.run().await, ReconcileOutcome::Cleared);
        assert_eq!(
            reset.calls.load(Ordering::SeqCst),
            1,
            "a stale register must be cleared exactly once"
        );
    }

    #[tokio::test]
    async fn empty_os_clipboard_clears_a_nonempty_register() {
        // OS clipboard is empty (its hash can't match any content hash) → the
        // row is stale → clear.
        let (uc, reset) = build(FakeClipboard::Empty, Some(state_matching("stored")));
        assert_eq!(uc.run().await, ReconcileOutcome::Cleared);
        assert_eq!(reset.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn unreadable_os_clipboard_clears_register_as_untrusted() {
        let (uc, reset) = build(FakeClipboard::ReadError, Some(state_matching("stored")));
        assert_eq!(uc.run().await, ReconcileOutcome::Cleared);
        assert_eq!(
            reset.calls.load(Ordering::SeqCst),
            1,
            "an unreadable OS clipboard means the row can't be trusted; clear it"
        );
    }

    #[tokio::test]
    async fn register_load_error_clears_as_untrusted() {
        let reset = Arc::new(ResetSpy::default());
        let uc = ReconcileActiveClipboardStateUseCase::new(
            Arc::new(FakeClipboard::Text("x")),
            Arc::new(LoadErrors),
            Arc::clone(&reset) as Arc<dyn ResetActiveClipboardPort>,
        );
        assert_eq!(uc.run().await, ReconcileOutcome::Cleared);
        assert_eq!(reset.calls.load(Ordering::SeqCst), 1);
    }
}
