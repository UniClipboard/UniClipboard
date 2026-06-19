//! `MemberReceiveGate` — the per-device inbound receive gate.
//!
//! Single source of truth for "should the local device accept clipboard
//! state from `peer`", shared by every inbound path (bulk content ingest and
//! the active-clipboard state handler). Two independent stages:
//!
//! 1. **Device-level kill switch** — `receive_enabled`. Cheap; check first.
//! 2. **Content-type filter** — `receive_content_types`, AND-of-allowed
//!    across the snapshot's category set (see `uc-core` `category.rs`). An
//!    empty set passes (raw / unrecognised payload); a non-empty set passes
//!    only when every category in it is allowed.
//!
//! Both stages **fail open** on a member-repo miss or error: a transient
//! roster/repo glitch must not silently kill incoming sync.

use std::sync::Arc;

use tracing::{info, warn};

use uc_core::clipboard::ClipboardContentCategorySet;
use uc_core::ids::DeviceId;
use uc_core::MemberRepositoryPort;

/// Reads a peer's per-device sync preferences to decide whether inbound
/// clipboard data from it should be accepted.
pub(crate) struct MemberReceiveGate {
    member_repo: Arc<dyn MemberRepositoryPort>,
}

impl MemberReceiveGate {
    pub(crate) fn new(member_repo: Arc<dyn MemberRepositoryPort>) -> Self {
        Self { member_repo }
    }

    /// Stage 1: device-level kill switch. Returns `true` when the local
    /// device should accept clipboard data from `peer` at all. Reads
    /// `SpaceMember.sync_preferences.receive_enabled`; fails open on lookup
    /// error or missing record so a transient repo glitch can't silently
    /// kill incoming sync.
    pub(crate) async fn is_receive_allowed(&self, peer: &DeviceId) -> bool {
        match self.member_repo.get(peer).await {
            Ok(Some(member)) => {
                if !member.sync_preferences.receive_enabled {
                    info!(
                        peer = %peer.as_str(),
                        reason = "receive_disabled_by_user",
                        "receive gate: dropping inbound per per-device sync preferences"
                    );
                    return false;
                }
                true
            }
            Ok(None) => {
                warn!(
                    peer = %peer.as_str(),
                    "receive gate: inbound from peer missing in member repo; failing open"
                );
                true
            }
            Err(err) => {
                warn!(
                    peer = %peer.as_str(),
                    error = %err,
                    "receive gate: member repo lookup failed; failing open"
                );
                true
            }
        }
    }

    /// Stage 2: content-type filter, AND-of-allowed across the snapshot's
    /// category set. An empty set passes (fail open); a non-empty set passes
    /// only when every category in it is allowed by `receive_content_types`.
    /// Same fail-open posture as stage 1 on lookup errors (logged once by
    /// stage 1, so these branches stay quiet to avoid log spam).
    pub(crate) async fn is_receive_category_allowed(
        &self,
        peer: &DeviceId,
        categories: &ClipboardContentCategorySet,
    ) -> bool {
        match self.member_repo.get(peer).await {
            Ok(Some(member)) => {
                if !categories.allowed_by(&member.sync_preferences.receive_content_types) {
                    info!(
                        peer = %peer.as_str(),
                        categories = %categories.labels(),
                        denied = %categories
                            .denied_labels(&member.sync_preferences.receive_content_types),
                        reason = "content_type_disabled_by_user",
                        "receive gate: dropping inbound per per-device content_types filter"
                    );
                    return false;
                }
                true
            }
            Ok(None) | Err(_) => true,
        }
    }
}
