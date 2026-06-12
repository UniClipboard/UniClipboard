//! In-memory [`PairingInvitation`] holder for sponsor-side pending
//! invitations.
//!
//! Slice 1 decision Q-2: invitations live **only** in this holder and are
//! dropped when the process exits. No replacement axis (no "redis holder"
//! etc.) — invitations are intrinsically short-lived (typical TTL ≤ 10
//! minutes) and re-issuing on next launch is acceptable, so we don't need
//! a port here.
//!
//! Operations:
//! * `insert` — parking path, called by `IssuePairingInvitationUseCase`
//!   after a successful rendezvous issue.
//! * `take_matching` — consume path (P7e), called by the sponsor-side
//!   inbound orchestrator when a joiner `JoinerRequest` arrives; atomically
//!   locates the aggregate by code, drives it through `consume(code, now)`,
//!   and removes it from the map on success or terminal failure.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use thiserror::Error;
use tokio::sync::Mutex;

use uc_core::pairing::invitation::{
    ConsumeError, InvitationCode, InvitationState, PairingInvitation,
};

/// Process-local map of outstanding [`PairingInvitation`]s keyed by code.
///
/// The code is chosen as the key because the sponsor-side `Incoming`
/// event (P7e) carries only the joiner-echoed code, not the aggregate's
/// internal pointer.
pub(crate) struct InMemoryPairingInvitationHolder {
    by_code: Mutex<HashMap<InvitationCode, PairingInvitation>>,
}

impl InMemoryPairingInvitationHolder {
    pub(crate) fn new() -> Self {
        Self {
            by_code: Mutex::new(HashMap::new()),
        }
    }

    /// Insert (or overwrite) the aggregate keyed by its code.
    ///
    /// Overwrite semantics: a fresh `issue_invitation()` that reuses the
    /// same code (rendezvous adapter decides code uniqueness) replaces the
    /// previous slot. The caller's invariant is "the latest issue wins";
    /// we don't enforce a "single pending per device" rule here because
    /// that's a UI-level policy decision, not a core invariant.
    pub(crate) async fn insert(&self, invitation: PairingInvitation) {
        let code = invitation.code().clone();
        self.by_code.lock().await.insert(code, invitation);
    }

    /// Atomically locate + consume the aggregate matching `code`.
    ///
    /// Behaviour:
    /// * `Ok(consumed)` — aggregate existed, was `Pending`, `now <
    ///   expires_at`: aggregate is driven to `Consumed`, removed from the
    ///   map, and returned so the caller can use its content (space id,
    ///   issuer device id, …).
    /// * `Err(NotFound)` — no entry under this code. Stale rendezvous
    ///   lookup or attacker replay.
    /// * `Err(Expired)` — entry existed but is past TTL. Aggregate is
    ///   dropped from the map (lazy expiry).
    ///
    /// The `CodeMismatch` / `NotPending` variants from
    /// [`PairingInvitation::consume`] are treated as internal invariant
    /// violations (the holder never stores an aggregate under a non-matching
    /// key, and `insert` never takes a non-`Pending` aggregate) — they
    /// surface as `Internal` so bugs are loud rather than silent.
    pub(crate) async fn take_matching(
        &self,
        code: &InvitationCode,
        now: DateTime<Utc>,
    ) -> Result<PairingInvitation, TakeMatchingError> {
        let mut map = self.by_code.lock().await;
        let Some(mut invitation) = map.remove(code) else {
            return Err(TakeMatchingError::NotFound);
        };
        match invitation.consume(code, now) {
            Ok(_event) => Ok(invitation),
            Err(ConsumeError::Expired) => Err(TakeMatchingError::Expired),
            Err(ConsumeError::CodeMismatch) => Err(TakeMatchingError::Internal(
                "holder key mismatches aggregate code — holder invariant broken".into(),
            )),
            Err(ConsumeError::NotPending) => Err(TakeMatchingError::Internal(
                "holder stored a non-pending aggregate — insert/issue invariant broken".into(),
            )),
        }
    }

    /// Snapshot the **earliest-expiring** outstanding invitation, if any.
    ///
    /// Returned without consuming, intended for read-only query paths
    /// (Slice4 P3 T3.2 `SpaceSetupFacade::query_setup_state`). Callers
    /// must not assume a single pending invitation exists; this just
    /// gives a deterministic representative when multiple are parked.
    pub(crate) async fn snapshot_earliest(&self) -> Option<(InvitationCode, DateTime<Utc>)> {
        let map = self.by_code.lock().await;
        map.values()
            .filter_map(|inv| match inv.state() {
                InvitationState::Pending { expires_at } => Some((inv.code().clone(), *expires_at)),
                _ => None,
            })
            .min_by_key(|(_, exp)| *exp)
    }

    /// Drop every outstanding invitation, returning the count removed.
    ///
    /// Used by Slice4 P3 T3.2 `SpaceSetupFacade::cancel_invitation` and
    /// `reset` to wipe in-flight pairing state. Aggregates already
    /// `Consumed` are not present in the holder (they are removed at
    /// `take_matching` time), so this only clears `Pending` entries.
    pub(crate) async fn cancel_all(&self) -> usize {
        let mut map = self.by_code.lock().await;
        let count = map.len();
        map.clear();
        count
    }

    /// Count of outstanding entries (test-only — not part of the
    /// application-facing surface).
    #[cfg(test)]
    pub(crate) async fn len(&self) -> usize {
        self.by_code.lock().await.len()
    }

    /// Test-only: look up by code without consuming the aggregate.
    #[cfg(test)]
    pub(crate) async fn get_for_test(&self, code: &InvitationCode) -> Option<PairingInvitation> {
        self.by_code.lock().await.get(code).cloned()
    }
}

impl Default for InMemoryPairingInvitationHolder {
    fn default() -> Self {
        Self::new()
    }
}

/// Reasons `take_matching` did not yield a consumed aggregate.
#[derive(Debug, Error)]
pub(crate) enum TakeMatchingError {
    /// No invitation parked under this code.
    #[error("no pending invitation for code")]
    NotFound,

    /// Invitation existed but TTL has elapsed.
    #[error("invitation expired")]
    Expired,

    /// Holder invariant broken — see message. Should not happen in
    /// production; surfaced so the orchestrator's log path is explicit
    /// instead of hiding the bug behind a NotFound.
    #[error("holder invariant violated: {0}")]
    Internal(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::{DateTime, Duration, Utc};

    use uc_core::ids::DeviceId;
    use uc_core::pairing::invitation::InvitationState;

    fn fixed_now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-04-20T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn pending(code: &str) -> PairingInvitation {
        let issued = fixed_now();
        let expires = issued + Duration::minutes(5);
        let (invitation, _) = PairingInvitation::issue(
            InvitationCode::new(code),
            issued,
            expires,
            DeviceId::new("device-1"),
        );
        invitation
    }

    #[tokio::test]
    async fn insert_stores_aggregate_by_code() {
        let holder = InMemoryPairingInvitationHolder::new();
        holder.insert(pending("ABCD-1234")).await;
        assert_eq!(holder.len().await, 1);
        let stored = holder
            .get_for_test(&InvitationCode::new("ABCD-1234"))
            .await
            .expect("aggregate stored");
        assert!(matches!(stored.state(), InvitationState::Pending { .. }));
    }

    #[tokio::test]
    async fn insert_with_same_code_overwrites() {
        let holder = InMemoryPairingInvitationHolder::new();
        holder.insert(pending("SAME")).await;
        holder.insert(pending("SAME")).await;
        assert_eq!(holder.len().await, 1, "overwrite, not duplicate");
    }

    #[tokio::test]
    async fn distinct_codes_coexist() {
        let holder = InMemoryPairingInvitationHolder::new();
        holder.insert(pending("ONE")).await;
        holder.insert(pending("TWO")).await;
        assert_eq!(holder.len().await, 2);
    }

    // ── take_matching (P7e) ───────────────────────────────────────────────

    #[tokio::test]
    async fn take_matching_consumes_pending_aggregate_and_removes_slot() {
        let holder = InMemoryPairingInvitationHolder::new();
        holder.insert(pending("ABCD-1234")).await;

        let taken = holder
            .take_matching(&InvitationCode::new("ABCD-1234"), fixed_now())
            .await
            .expect("pending aggregate should be consumed");
        assert_eq!(taken.state(), &InvitationState::Consumed);
        assert_eq!(taken.code().as_str(), "ABCD-1234");
        assert_eq!(
            holder.len().await,
            0,
            "aggregate must be removed from the map once consumed"
        );
    }

    #[tokio::test]
    async fn take_matching_absent_code_returns_not_found() {
        let holder = InMemoryPairingInvitationHolder::new();
        holder.insert(pending("ABCD-1234")).await;

        let err = holder
            .take_matching(&InvitationCode::new("WRONG"), fixed_now())
            .await
            .unwrap_err();
        assert!(matches!(err, TakeMatchingError::NotFound));
        assert_eq!(
            holder.len().await,
            1,
            "a missing-code lookup must not disturb unrelated entries"
        );
    }

    #[tokio::test]
    async fn take_matching_expired_invitation_returns_expired_and_drops_slot() {
        let holder = InMemoryPairingInvitationHolder::new();
        holder.insert(pending("ABCD-1234")).await;

        let late = fixed_now() + Duration::minutes(10);
        let err = holder
            .take_matching(&InvitationCode::new("ABCD-1234"), late)
            .await
            .unwrap_err();
        assert!(matches!(err, TakeMatchingError::Expired));
        assert_eq!(
            holder.len().await,
            0,
            "expired aggregate is removed (lazy expiry, not put back)"
        );
    }

    #[tokio::test]
    async fn take_matching_is_single_shot_second_call_is_not_found() {
        let holder = InMemoryPairingInvitationHolder::new();
        holder.insert(pending("ABCD-1234")).await;
        let _ = holder
            .take_matching(&InvitationCode::new("ABCD-1234"), fixed_now())
            .await
            .expect("first consume succeeds");

        let err = holder
            .take_matching(&InvitationCode::new("ABCD-1234"), fixed_now())
            .await
            .unwrap_err();
        assert!(matches!(err, TakeMatchingError::NotFound));
    }
}
