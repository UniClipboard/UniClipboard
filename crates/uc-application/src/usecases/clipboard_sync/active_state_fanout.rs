//! Shared outbound fan-out for active-clipboard state (0xC3).
//!
//! Single implementation of "send this converged active-clipboard state to
//! every allowed peer", reused by both 0xC3 origination paths:
//!
//! * inbound re-broadcast (after an inbound observation is honoured — the
//!   core invariant "register advanced ⟺ OS write succeeded ⟺ re-broadcast"),
//! * restore broadcast (after a local history restore advances the register).
//!
//! Keeping one fan-out avoids parallel gate/skip/dispatch copies drifting
//! apart. The full outbound gate (issue #1017 D2) is applied here:
//! `send_enabled` ∧ `send_content_types`, threaded via the activation's
//! content category set.

use std::sync::Arc;

use tracing::{debug, warn};

use uc_core::clipboard::{ActiveClipboardState, ClipboardContentCategorySet};
use uc_core::ports::clipboard::ActiveClipboardDispatchPort;
use uc_core::ports::PeerAddressRepositoryPort;

use super::send_gate::MemberSendGate;

/// Fan `state` out to every allowed peer.
///
/// The roster is the set of peers we hold an address for
/// (`peer_addr_repo.list()`), so a peer with no address is silently skipped
/// (offline / never reachable). The device that activated the state
/// (`state.activated_by`) is never echoed back to. Each surviving target is
/// gated by the full outbound gate (`send_enabled` ∧ `send_content_types`,
/// the latter via `categories`). Per-peer dispatch failures are isolated and
/// logged — the register is convergent, so a missed send is recovered by a
/// later advance or a peer-online resync.
pub(crate) async fn fan_out_active_state(
    dispatch: &Arc<dyn ActiveClipboardDispatchPort>,
    peer_addr_repo: &Arc<dyn PeerAddressRepositoryPort>,
    send_gate: &MemberSendGate,
    state: &ActiveClipboardState,
    categories: &ClipboardContentCategorySet,
) {
    let records = match peer_addr_repo.list().await {
        Ok(r) => r,
        Err(err) => {
            warn!(error = %err, "active state fan-out skipped: peer_addr_repo.list failed");
            return;
        }
    };

    for record in records {
        let target = record.device_id;
        // Never echo the state back to the device that activated it.
        if target == state.activated_by {
            continue;
        }
        if !send_gate.is_send_allowed(&target, categories).await {
            continue;
        }
        if let Err(err) = dispatch.dispatch(&target, state).await {
            debug!(
                device = %target.as_str(),
                error = %err,
                "active state fan-out: per-peer dispatch failed (isolated)"
            );
        }
    }
}
