//! Per-peer result classification + delivery recording + the deferred
//! (post-deadline) drain continuation.
//!
//! `DeliveryRecorder` holds the two ports that persist and surface
//! delivery outcomes (`entry_delivery_repo` + the host-event bus).
//! Write-then-emit ordering in [`DeliveryRecorder::flush`] is LOAD-BEARING:
//! the host event is a payload-less "refetch ping", so the frontend's
//! follow-up read must observe the DB write — reordering or batching
//! emit-before-write surfaces a stale snapshot to the detail view.

use std::sync::Arc;
use std::time::Instant;

use tokio::task::{JoinError, JoinSet};
use tracing::{debug, info, warn, Instrument};

use uc_core::clipboard::{DeliveryFailureReason, EntryDeliveryRecord, EntryDeliveryStatus};
use uc_core::ids::EntryId;
use uc_core::ports::{ClipboardDispatchError, ClockPort, DispatchAck, EntryDeliveryRepositoryPort};

use crate::facade::blob_transfer::SharedHostEventEmitter;
use crate::facade::host_event::{DeliveryHostEvent, HostEvent};

use super::{DispatchPerTarget, PeerDispatchResult};

/// Outcome bucket for [`classify_dispatch_result`]. `Panicked` rolls up
/// into `Errored` at the call site because `DispatchOutcome` has no
/// separate panic bucket and treating a panicked task as "errored" keeps
/// `attempted = succeeded + failed + deferred` intact for telemetry.
pub(crate) enum DispatchResultBucket {
    Accepted,
    Duplicate,
    Offline,
    Errored,
    Panicked,
}

/// Folded view of one fanned-out peer's `JoinSet` result, ready to fold
/// into `DispatchOutcome` (or the background continuation).
pub(crate) struct ProcessedDispatchResult {
    /// `None` iff the task panicked / was cancelled (no DeviceId recoverable).
    pub per_target: Option<DispatchPerTarget>,
    /// `None` iff `entry_id` was `None` OR the task panicked. Otherwise a
    /// fully populated record ready for `record_attempt`.
    pub delivery_record: Option<EntryDeliveryRecord>,
    pub bucket: DispatchResultBucket,
}

/// Shared per-peer result-handling — used by both the foreground fold and
/// the background continuation that drains the leftover `JoinSet` after the
/// fan-out deadline. A free function (not a method) so the detached
/// background task can call it without holding `&self`.
///
/// `now_ms` is sampled by the caller (each peer's `updated_at_ms` reflects
/// the moment that peer's result was observed, not a shared snapshot).
pub(crate) fn classify_dispatch_result(
    joined: Result<PeerDispatchResult, JoinError>,
    entry_id: Option<&EntryId>,
    now_ms: i64,
) -> ProcessedDispatchResult {
    match joined {
        Ok((device_id, Ok(DispatchAck::Accepted))) => {
            debug!(device_id = %device_id.as_str(), "dispatch → Accepted");
            let delivery_record = entry_id.map(|eid| EntryDeliveryRecord {
                entry_id: eid.clone(),
                target_device_id: device_id.clone(),
                status: EntryDeliveryStatus::Delivered,
                reason_detail: None,
                updated_at_ms: now_ms,
            });
            ProcessedDispatchResult {
                per_target: Some(DispatchPerTarget {
                    device_id,
                    outcome: Ok(DispatchAck::Accepted),
                }),
                delivery_record,
                bucket: DispatchResultBucket::Accepted,
            }
        }
        Ok((device_id, Ok(DispatchAck::DuplicateIgnored))) => {
            debug!(device_id = %device_id.as_str(), "dispatch → DuplicateIgnored");
            let delivery_record = entry_id.map(|eid| EntryDeliveryRecord {
                entry_id: eid.clone(),
                target_device_id: device_id.clone(),
                status: EntryDeliveryStatus::Duplicate,
                reason_detail: None,
                updated_at_ms: now_ms,
            });
            ProcessedDispatchResult {
                per_target: Some(DispatchPerTarget {
                    device_id,
                    outcome: Ok(DispatchAck::DuplicateIgnored),
                }),
                delivery_record,
                bucket: DispatchResultBucket::Duplicate,
            }
        }
        Ok((device_id, Err(ClipboardDispatchError::Offline))) => {
            debug!(device_id = %device_id.as_str(), "dispatch → Offline (unreachable)");
            let delivery_record = entry_id.map(|eid| EntryDeliveryRecord {
                entry_id: eid.clone(),
                target_device_id: device_id.clone(),
                status: EntryDeliveryStatus::Unreachable,
                reason_detail: None,
                updated_at_ms: now_ms,
            });
            ProcessedDispatchResult {
                per_target: Some(DispatchPerTarget {
                    device_id,
                    outcome: Err("offline".to_string()),
                }),
                delivery_record,
                bucket: DispatchResultBucket::Offline,
            }
        }
        Ok((device_id, Err(err))) => {
            warn!(device_id = %device_id.as_str(), error = %err, "dispatch failed");
            let (failure_reason, reason_detail) = match &err {
                // Offline is handled in the previous arm (Unreachable); this
                // arm only fires for the non-Offline error variants.
                ClipboardDispatchError::Offline => {
                    unreachable!("Offline is matched in the dedicated arm above")
                }
                ClipboardDispatchError::LocalPolicyExceeded(s) => {
                    (DeliveryFailureReason::LocalPolicy, Some(s.clone()))
                }
                ClipboardDispatchError::PeerRejected(s) => {
                    (DeliveryFailureReason::PeerRejected, Some(s.clone()))
                }
                ClipboardDispatchError::Io(s) => (DeliveryFailureReason::Io, Some(s.clone())),
                ClipboardDispatchError::Internal(s) => {
                    (DeliveryFailureReason::Internal, Some(s.clone()))
                }
            };
            let delivery_record = entry_id.map(|eid| EntryDeliveryRecord {
                entry_id: eid.clone(),
                target_device_id: device_id.clone(),
                status: EntryDeliveryStatus::Failed {
                    reason: failure_reason,
                },
                reason_detail,
                updated_at_ms: now_ms,
            });
            ProcessedDispatchResult {
                per_target: Some(DispatchPerTarget {
                    device_id,
                    outcome: Err(err.to_string()),
                }),
                delivery_record,
                bucket: DispatchResultBucket::Errored,
            }
        }
        Err(err) => {
            warn!(error = %err, "dispatch task panicked or cancelled");
            ProcessedDispatchResult {
                per_target: None,
                delivery_record: None,
                bucket: DispatchResultBucket::Panicked,
            }
        }
    }
}

/// Persists per-peer delivery outcomes and pings the frontend to refetch.
pub(crate) struct DeliveryRecorder {
    entry_delivery_repo: Arc<dyn EntryDeliveryRepositoryPort>,
    host_event_bus: SharedHostEventEmitter,
}

impl DeliveryRecorder {
    pub(crate) fn new(
        entry_delivery_repo: Arc<dyn EntryDeliveryRepositoryPort>,
        host_event_bus: SharedHostEventEmitter,
    ) -> Self {
        Self {
            entry_delivery_repo,
            host_event_bus,
        }
    }

    /// Sequentially `record_attempt` each record then `emit_or_warn` the
    /// matching refetch ping. Write-then-emit order is load-bearing — the
    /// host event carries no payload, so the frontend's follow-up read must
    /// observe the write. Errors only `warn!`; this is an observability
    /// side-effect that must not mask dispatch's real success/failure.
    pub(crate) async fn flush(&self, records: &[EntryDeliveryRecord]) {
        for record in records {
            if let Err(err) = self.entry_delivery_repo.record_attempt(record).await {
                warn!(
                    error = %err,
                    entry_id = %record.entry_id,
                    target_device_id = %record.target_device_id,
                    "failed to record entry delivery"
                );
                continue;
            }
            self.host_event_bus.emit_or_warn(HostEvent::Delivery(
                DeliveryHostEvent::StatusChanged {
                    entry_id: record.entry_id.to_string(),
                    target_device_id: record.target_device_id.as_str().to_string(),
                },
            ));
        }
    }
}

/// Drive the post-deadline leftover tasks to completion on a detached
/// task: classify each settle, record it immediately (per-settle, NOT
/// batched, so an early-settling peer's badge isn't held hostage by a
/// staggered-retry long-tail), and log a per-bucket summary when drained.
///
/// Best-effort RECORD-ONLY (VISION locked decision #59): a peer that
/// finally settles Offline / Errored after the deadline is recorded as
/// such, never replayed — automatic redelivery is an absolute禁区.
pub(crate) fn spawn_deferred_drain(
    mut set: JoinSet<PeerDispatchResult>,
    entry_id: Option<EntryId>,
    clock: Arc<dyn ClockPort>,
    recorder: Arc<DeliveryRecorder>,
    snapshot_hash: String,
) {
    let deferred_count = set.len();
    tokio::spawn(
        async move {
            let started = Instant::now();
            let mut accepted = 0usize;
            let mut duplicate = 0usize;
            let mut offline = 0usize;
            let mut errored = 0usize;
            while let Some(joined) = set.join_next().await {
                let processed = classify_dispatch_result(joined, entry_id.as_ref(), clock.now_ms());
                match processed.bucket {
                    DispatchResultBucket::Accepted => accepted += 1,
                    DispatchResultBucket::Duplicate => duplicate += 1,
                    DispatchResultBucket::Offline => offline += 1,
                    DispatchResultBucket::Errored | DispatchResultBucket::Panicked => errored += 1,
                }
                if let Some(rec) = processed.delivery_record {
                    recorder.flush(std::slice::from_ref(&rec)).await;
                }
            }
            info!(
                snapshot_hash = %snapshot_hash,
                deferred_count,
                accepted,
                duplicate,
                offline,
                errored,
                bg_duration_ms = started.elapsed().as_millis() as u64,
                "dispatch: deferred fan-out completed"
            );
        }
        .in_current_span(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use uc_core::ids::DeviceId;

    fn dev(id: &str) -> DeviceId {
        DeviceId::new(id)
    }

    fn eid() -> EntryId {
        EntryId::from("entry-1".to_string())
    }

    #[test]
    fn accepted_maps_to_delivered_record_and_accepted_bucket() {
        let joined = Ok((dev("peer-a"), Ok(DispatchAck::Accepted)));
        let p = classify_dispatch_result(joined, Some(&eid()), 42);
        assert!(matches!(p.bucket, DispatchResultBucket::Accepted));
        let pt = p.per_target.expect("per_target present");
        assert_eq!(pt.device_id, dev("peer-a"));
        assert!(matches!(pt.outcome, Ok(DispatchAck::Accepted)));
        let rec = p.delivery_record.expect("delivery record present");
        assert!(matches!(rec.status, EntryDeliveryStatus::Delivered));
        assert_eq!(rec.updated_at_ms, 42);
        assert_eq!(rec.reason_detail, None);
    }

    #[test]
    fn duplicate_maps_to_duplicate_record_and_bucket() {
        let joined = Ok((dev("peer-a"), Ok(DispatchAck::DuplicateIgnored)));
        let p = classify_dispatch_result(joined, Some(&eid()), 1);
        assert!(matches!(p.bucket, DispatchResultBucket::Duplicate));
        assert!(matches!(
            p.per_target.unwrap().outcome,
            Ok(DispatchAck::DuplicateIgnored)
        ));
        assert!(matches!(
            p.delivery_record.unwrap().status,
            EntryDeliveryStatus::Duplicate
        ));
    }

    #[test]
    fn offline_maps_to_unreachable_record_and_offline_bucket() {
        let joined = Ok((dev("peer-a"), Err(ClipboardDispatchError::Offline)));
        let p = classify_dispatch_result(joined, Some(&eid()), 1);
        assert!(matches!(p.bucket, DispatchResultBucket::Offline));
        assert!(matches!(p.per_target.unwrap().outcome, Err(ref s) if s == "offline"));
        assert!(matches!(
            p.delivery_record.unwrap().status,
            EntryDeliveryStatus::Unreachable,
        ));
    }

    #[test]
    fn non_offline_error_maps_to_errored_with_reason_detail() {
        let joined = Ok((
            dev("peer-a"),
            Err(ClipboardDispatchError::PeerRejected("nope".to_string())),
        ));
        let p = classify_dispatch_result(joined, Some(&eid()), 1);
        assert!(matches!(p.bucket, DispatchResultBucket::Errored));
        let rec = p.delivery_record.unwrap();
        assert!(matches!(
            rec.status,
            EntryDeliveryStatus::Failed {
                reason: DeliveryFailureReason::PeerRejected
            }
        ));
        assert_eq!(rec.reason_detail, Some("nope".to_string()));
    }

    #[test]
    fn no_entry_id_yields_per_target_but_no_delivery_record() {
        let joined = Ok((dev("peer-a"), Ok(DispatchAck::Accepted)));
        let p = classify_dispatch_result(joined, None, 1);
        assert!(p.per_target.is_some());
        assert!(p.delivery_record.is_none());
    }

    #[tokio::test]
    async fn panicked_task_maps_to_panicked_bucket_with_no_target_or_record() {
        // A real JoinError is only obtainable from a JoinSet, so drive a
        // panicking task through to settle, then classify its Err(JoinError).
        let mut set: JoinSet<PeerDispatchResult> = JoinSet::new();
        set.spawn(async { panic!("boom") });
        let joined = set.join_next().await.expect("one task settled");
        assert!(joined.is_err(), "panicked task must surface as JoinError");
        let p = classify_dispatch_result(joined, Some(&eid()), 1);
        assert!(matches!(p.bucket, DispatchResultBucket::Panicked));
        assert!(p.per_target.is_none());
        assert!(p.delivery_record.is_none());
    }
}
