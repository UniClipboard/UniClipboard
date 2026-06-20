//! Shared outbound send of active-clipboard state (0xC3).
//!
//! Single implementation of "send this converged active-clipboard state to a
//! peer under the full outbound gate", reused by every 0xC3 origination path:
//!
//! * inbound re-broadcast (after an inbound observation is honoured — the
//!   core invariant "register advanced ⟺ OS write succeeded ⟺ re-broadcast"),
//! * restore broadcast (after a local history restore advances the register),
//! * peer-online resync (after a peer comes online — send our current
//!   register to it so both ends converge via LWW).
//!
//! Two entry points share the same gate + dispatch step so they cannot drift:
//!
//! * [`send_active_state_to`] — single-target gated send.
//! * [`fan_out_active_state`] — multi-target fan-out built on top of it.
//!
//! The full outbound gate (issue #1017 D2) is applied in the single-target
//! step: `send_enabled` ∧ `send_content_types`, threaded via the activation's
//! content category set.

use std::sync::Arc;

use tracing::{debug, warn};

use uc_core::clipboard::{ActiveClipboardState, ClipboardContentCategorySet};
use uc_core::ids::DeviceId;
use uc_core::ports::clipboard::ActiveClipboardDispatchPort;
use uc_core::ports::{PeerAddressRepositoryPort, PresencePort, ReachabilityState};

use super::super::send_gate::MemberSendGate;

/// Send `state` to a single `target` under the full outbound gate (issue
/// #1017 D2): `send_enabled` ∧ `send_content_types` (the latter via
/// `categories`). A gate-rejected target is silently skipped; a dispatch
/// failure is isolated and logged at `debug` (the register is convergent, so
/// a missed send is recovered by a later advance or another peer-online
/// resync).
///
/// This is the single gate+dispatch step every 0xC3 origination path shares.
/// It does *not* consult the peer-address roster or the echo-suppression rule
/// (`state.activated_by`); that selection is the caller's concern —
/// [`fan_out_active_state`] applies both before delegating here.
pub(crate) async fn send_active_state_to(
    dispatch: &Arc<dyn ActiveClipboardDispatchPort>,
    send_gate: &MemberSendGate,
    target: &DeviceId,
    state: &ActiveClipboardState,
    categories: &ClipboardContentCategorySet,
) {
    if !send_gate.is_send_allowed(target, categories).await {
        return;
    }
    if let Err(err) = dispatch.dispatch(target, state).await {
        debug!(
            device = %target.as_str(),
            error = %err,
            "active state send: per-peer dispatch failed (isolated)"
        );
    }
}

/// Fan `state` out to every allowed peer.
///
/// The roster is the set of peers we hold an address for
/// (`peer_addr_repo.list()`), so a peer with no address is silently skipped
/// (offline / never reachable). The device that activated the state
/// (`state.activated_by`) is never echoed back to, and a peer the presence
/// tracker already reports `Offline` is skipped without dialing. Each surviving
/// target is gated by the full outbound gate (`send_enabled` ∧
/// `send_content_types`, the latter via `categories`). Per-peer dispatch
/// failures are isolated and logged — the register is convergent, so a missed
/// send is recovered by a later advance or a peer-online resync.
pub(crate) async fn fan_out_active_state(
    dispatch: &Arc<dyn ActiveClipboardDispatchPort>,
    peer_addr_repo: &Arc<dyn PeerAddressRepositoryPort>,
    presence: &Arc<dyn PresencePort>,
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
        // Skip peers the presence tracker already knows are offline (mirrors the
        // 0xC1 dispatch preflight): the roster can carry stale/ghost members,
        // and dialing each costs a multi-second connect timeout. `Unknown` is
        // deliberately NOT pre-filtered — the dispatch adapter marks peers
        // offline on its own dial failures, so an unprobed peer still gets one
        // real attempt rather than being silently dropped.
        if matches!(
            presence.current_state(&target).await,
            ReachabilityState::Offline
        ) {
            debug!(
                device = %target.as_str(),
                "active state fan-out: skipping peer known offline (deferred)"
            );
            continue;
        }
        send_active_state_to(dispatch, send_gate, &target, state, categories).await;
    }
}
