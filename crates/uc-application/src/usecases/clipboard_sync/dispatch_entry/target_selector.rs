//! `TargetSelector` — resolves the eligible fan-out roster for one
//! dispatch pass: the peers we hold an address for, minus self, narrowed
//! by the optional resend `target_filter`, gated by each peer's per-device
//! send preferences.
//!
//! ## Invariants preserved from the inlined logic
//!
//! - Iteration source is `peer_addr_repo.list()` (members we have an
//!   address blob for), NOT `member_repo` — avoids iterating ghost
//!   half-paired rows.
//! - Presence is intentionally NOT consulted here; the per-target preflight
//!   happens later in `PerPeerDispatcher`.
//! - `Some(vec![])` `target_filter` ⇒ empty intersection ⇒ zero targets
//!   (NOT a passthrough); `ResendEntryUseCase` already short-circuits the
//!   empty-difference case before it ever reaches dispatch.
//! - The send gate fails OPEN on a member-repo miss / error — a transient
//!   glitch must not silently kill sync.

use std::sync::Arc;

use tracing::{info, warn};
use uc_core::clipboard::ClipboardContentCategorySet;
use uc_core::ids::DeviceId;
use uc_core::ports::PeerAddressRepositoryPort;
use uc_core::MemberRepositoryPort;

use super::{DispatchClipboardEntryInput, DispatchSyncError};

pub(crate) struct TargetSelector {
    peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
    member_repo: Arc<dyn MemberRepositoryPort>,
}

impl TargetSelector {
    pub(crate) fn new(
        peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
        member_repo: Arc<dyn MemberRepositoryPort>,
    ) -> Self {
        Self {
            peer_addr_repo,
            member_repo,
        }
    }

    /// Enumerate eligible fan-out targets for this pass. `local_device` is
    /// resolved once by the caller (it also stamps the wire header origin)
    /// and passed in so device identity is read a single time per dispatch.
    pub(crate) async fn select(
        &self,
        input: &DispatchClipboardEntryInput,
        local_device: &DeviceId,
    ) -> Result<Vec<DeviceId>, DispatchSyncError> {
        let records =
            self.peer_addr_repo.list().await.map_err(|err| {
                DispatchSyncError::Repository(format!("peer_addr_repo.list: {err}"))
            })?;

        let mut candidates: Vec<DeviceId> = Vec::with_capacity(records.len());
        for record in records {
            if record.device_id == *local_device {
                continue;
            }
            // ADR-005 §2.5 resend: `target_filter` narrows the fan-out
            // allowlist. `None` keeps the status quo (full fan-out);
            // `Some(list)` keeps only the intersection.
            if let Some(filter) = &input.target_filter {
                if !filter.iter().any(|d| d == &record.device_id) {
                    continue;
                }
            }
            if !self
                .is_send_allowed(&record.device_id, &input.categories)
                .await
            {
                continue;
            }
            candidates.push(record.device_id);
        }
        Ok(candidates)
    }

    /// Per-device sync gate: returns `true` when the local device should
    /// fan a clipboard frame out to `device_id`. Two stages:
    ///
    /// 1. Device-level kill switch (`send_enabled`).
    /// 2. Content-type filter (`send_content_types`, AND-of-allowed across
    ///    the snapshot's category set — see `uc-core` `category.rs` module doc).
    ///    Empty set (raw-bytes / unrecognised payload) passes (fail open)
    ///    so we don't stall sync silently.
    ///
    /// Member-record miss / repo error → fail open with a WARN, mirroring
    /// the device-level gate's posture: a transient glitch should not
    /// silently kill sync.
    async fn is_send_allowed(
        &self,
        device_id: &DeviceId,
        categories: &ClipboardContentCategorySet,
    ) -> bool {
        match self.member_repo.get(device_id).await {
            Ok(Some(member)) => {
                if !member.sync_preferences.send_enabled {
                    info!(
                        device_id = %device_id.as_str(),
                        reason = "send_disabled_by_user",
                        "dispatch: skipping peer per per-device sync preferences"
                    );
                    return false;
                }
                if !categories.allowed_by(&member.sync_preferences.send_content_types) {
                    info!(
                        device_id = %device_id.as_str(),
                        categories = %categories.labels(),
                        denied = %categories
                            .denied_labels(&member.sync_preferences.send_content_types),
                        reason = "content_type_disabled_by_user",
                        "dispatch: skipping peer per per-device content_types filter"
                    );
                    return false;
                }
                true
            }
            Ok(None) => {
                warn!(
                    device_id = %device_id.as_str(),
                    "dispatch: peer in addr repo but missing from member repo; failing open"
                );
                true
            }
            Err(err) => {
                warn!(
                    device_id = %device_id.as_str(),
                    error = %err,
                    "dispatch: member repo lookup failed; failing open"
                );
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;

    use uc_core::clipboard::ClipboardContentCategory;
    use uc_core::settings::model::ContentTypes;
    use uc_core::{MemberSyncPreferences, MembershipError};

    fn selector(peer_addr_repo: MockPeerAddrRepo, member_repo: MockMemberRepo) -> TargetSelector {
        TargetSelector::new(Arc::new(peer_addr_repo), Arc::new(member_repo))
    }

    /// `member_repo` returning the default (all-enabled) preferences for any
    /// queried device — the send gate is a no-op, so `select` reflects pure
    /// roster ∩ !self ∩ filter narrowing.
    fn member_repo_all_enabled() -> MockMemberRepo {
        let mut m = MockMemberRepo::new();
        m.expect_get()
            .returning(|did| Ok(Some(member(did, MemberSyncPreferences::default()))));
        m
    }

    fn ids(devices: &[DeviceId]) -> Vec<String> {
        devices.iter().map(|d| d.as_str().to_string()).collect()
    }

    /// `None` filter ⇒ every roster peer except the local device, in roster
    /// order. The send gate is open, so nothing else is dropped.
    #[tokio::test]
    async fn select_returns_all_non_self_peers_when_no_filter() {
        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list().times(1).returning(|| {
            Ok(vec![
                record("peer-a"),
                record("self-device"),
                record("peer-b"),
            ])
        });

        let selector = selector(repo, member_repo_all_enabled());
        let targets = selector
            .select(&dispatch_input(), &dev("self-device"))
            .await
            .expect("select ok");

        assert_eq!(ids(&targets), vec!["peer-a", "peer-b"]);
    }

    /// `Some(list)` keeps only the intersection of roster and filter.
    #[tokio::test]
    async fn select_keeps_only_target_filter_intersection() {
        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list()
            .times(1)
            .returning(|| Ok(vec![record("peer-a"), record("peer-b"), record("peer-c")]));

        let selector = selector(repo, member_repo_all_enabled());
        let mut input = dispatch_input();
        input.target_filter = Some(vec![dev("peer-b")]);
        let targets = selector
            .select(&input, &dev("self-device"))
            .await
            .expect("select ok");

        assert_eq!(ids(&targets), vec!["peer-b"]);
    }

    /// `Some(vec![])` is the legal "no targets" filter — an empty
    /// intersection, NOT a passthrough.
    #[tokio::test]
    async fn select_with_empty_target_filter_yields_no_targets() {
        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list()
            .times(1)
            .returning(|| Ok(vec![record("peer-a"), record("peer-b")]));

        let selector = selector(repo, member_repo_all_enabled());
        let mut input = dispatch_input();
        input.target_filter = Some(vec![]);
        let targets = selector
            .select(&input, &dev("self-device"))
            .await
            .expect("select ok");

        assert!(targets.is_empty(), "got {:?}", ids(&targets));
    }

    /// A peer with `send_enabled = false` is dropped by the device-level
    /// kill switch; the other peer survives.
    #[tokio::test]
    async fn select_excludes_send_disabled_peer() {
        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list()
            .times(1)
            .returning(|| Ok(vec![record("peer-on"), record("peer-mute")]));

        let mut member_repo = MockMemberRepo::new();
        member_repo
            .expect_get()
            .returning(|did| match did.as_str() {
                "peer-mute" => {
                    let mut prefs = MemberSyncPreferences::default();
                    prefs.send_enabled = false;
                    Ok(Some(member(did, prefs)))
                }
                _ => Ok(Some(member(did, MemberSyncPreferences::default()))),
            });

        let selector = selector(repo, member_repo);
        let targets = selector
            .select(&dispatch_input(), &dev("self-device"))
            .await
            .expect("select ok");

        assert_eq!(ids(&targets), vec!["peer-on"]);
    }

    /// A `Text` snapshot is withheld from a peer whose `send_content_types`
    /// has text disabled; the default-allowed peer still receives it.
    #[tokio::test]
    async fn select_excludes_peer_denying_content_type() {
        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list()
            .times(1)
            .returning(|| Ok(vec![record("peer-on"), record("peer-no-text")]));

        let mut member_repo = MockMemberRepo::new();
        member_repo
            .expect_get()
            .returning(|did| match did.as_str() {
                "peer-no-text" => {
                    let mut prefs = MemberSyncPreferences::default();
                    let mut ct = ContentTypes::default();
                    ct.text = false;
                    prefs.send_content_types = ct;
                    Ok(Some(member(did, prefs)))
                }
                _ => Ok(Some(member(did, MemberSyncPreferences::default()))),
            });

        let selector = selector(repo, member_repo);
        let mut input = dispatch_input();
        input.categories = ClipboardContentCategorySet::empty();
        input.categories.insert(ClipboardContentCategory::Text);
        let targets = selector
            .select(&input, &dev("self-device"))
            .await
            .expect("select ok");

        assert_eq!(ids(&targets), vec!["peer-on"]);
    }

    /// Member-repo miss (`Ok(None)`) fails OPEN — a roster/member drift must
    /// not silently kill sync.
    #[tokio::test]
    async fn select_fails_open_on_member_lookup_miss() {
        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list()
            .times(1)
            .returning(|| Ok(vec![record("peer-orphan")]));

        let mut member_repo = MockMemberRepo::new();
        member_repo.expect_get().returning(|_| Ok(None));

        let selector = selector(repo, member_repo);
        let targets = selector
            .select(&dispatch_input(), &dev("self-device"))
            .await
            .expect("select ok");

        assert_eq!(ids(&targets), vec!["peer-orphan"]);
    }

    /// Member-repo error also fails OPEN — a transient glitch is not a
    /// reason to drop a paired peer.
    #[tokio::test]
    async fn select_fails_open_on_member_lookup_error() {
        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list()
            .times(1)
            .returning(|| Ok(vec![record("peer-glitch")]));

        let mut member_repo = MockMemberRepo::new();
        member_repo
            .expect_get()
            .returning(|_| Err(MembershipError::Repository("db down".to_string())));

        let selector = selector(repo, member_repo);
        let targets = selector
            .select(&dispatch_input(), &dev("self-device"))
            .await
            .expect("select ok");

        assert_eq!(ids(&targets), vec!["peer-glitch"]);
    }

    /// `peer_addr_repo.list` failure is a fatal pass error, surfaced as
    /// `DispatchSyncError::Repository` (not swallowed into an empty roster).
    #[tokio::test]
    async fn select_surfaces_peer_addr_repo_error() {
        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list()
            .times(1)
            .returning(|| Err(uc_core::ports::PeerAddressError::Internal("io".to_string())));

        let selector = selector(repo, member_repo_all_enabled());
        let err = selector
            .select(&dispatch_input(), &dev("self-device"))
            .await
            .expect_err("list failure must surface");

        assert!(matches!(err, DispatchSyncError::Repository(_)));
    }
}
