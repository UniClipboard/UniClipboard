//! `RestoreBroadcastTrigger` — the application-internal seam that lets a
//! successful local history restore hand off to the active-clipboard
//! broadcast subsystem without the restore use case knowing how broadcasting
//! works.
//!
//! A restore use case advances the active-clipboard register on a successful
//! OS write (keeping "register advanced ⟺ OS write succeeded"). When the
//! trigger is wired, it then offers the just-activated state to a downstream
//! debounced broadcaster. The trigger itself is fire-and-forget: it never
//! blocks the restore and never fails it — a closed channel (broadcaster gone)
//! is a silent no-op.
//!
//! The gate decision (whether `sync_on_restore` is on, and which peers are
//! eligible) lives entirely in the downstream broadcaster; the trigger only
//! transports the activation + its content categories so the broadcaster can
//! apply the per-device `send_content_types` filter without re-deriving them.

use tokio::sync::mpsc::UnboundedSender;
use tracing::trace;

use uc_core::clipboard::{ActiveClipboardState, ClipboardContentCategorySet};

/// One restore activation offered to the broadcast subsystem: the activated
/// state plus the content category set of what was put on the clipboard.
#[derive(Debug, Clone)]
pub struct RestoreBroadcastRequest {
    pub state: ActiveClipboardState,
    pub categories: ClipboardContentCategorySet,
}

/// Fire-and-forget handle a restore use case calls after it advances the
/// register. Cloning is cheap (the inner sender is an `Arc`-backed channel
/// handle), so the same trigger can be shared across restore paths.
#[derive(Clone)]
pub struct RestoreBroadcastTrigger {
    tx: UnboundedSender<RestoreBroadcastRequest>,
}

impl RestoreBroadcastTrigger {
    pub fn new(tx: UnboundedSender<RestoreBroadcastRequest>) -> Self {
        Self { tx }
    }

    /// Offer a restored activation to the broadcaster. Non-blocking; a send to
    /// a dropped receiver is ignored (the broadcaster having shut down must
    /// not surface as a restore failure).
    pub fn offer(&self, state: ActiveClipboardState, categories: ClipboardContentCategorySet) {
        let request = RestoreBroadcastRequest { state, categories };
        if self.tx.send(request).is_err() {
            trace!("restore broadcast trigger: receiver dropped; skipping offer");
        }
    }
}
