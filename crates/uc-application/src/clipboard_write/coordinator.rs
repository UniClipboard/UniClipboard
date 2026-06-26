//! `ClipboardWriteCoordinator` — single write boundary for all
//! programmatic clipboard writes, with per-intent guard TTL handling so
//! the daemon's clipboard watcher can attribute the change to the
//! originating intent (LocalRestore / RemotePush) and
//! avoid write-back loops.
//!
//! ## History
//!
//! Originally lived at
//! `uc-app/src/usecases/clipboard/clipboard_write_coordinator.rs`. Moved
//! here in Slice 2 Phase 3 (T0b) so `uc-application` use cases (e.g.
//! `ApplyInboundClipboardUseCase`) can depend on it without a reverse
//! `uc-application → uc-app` import (forbidden per `uc-app/AGENTS.md` §3).
//! The old path keeps a deprecated re-export shim until Slice 5 deletes
//! `uc-app`.

use anyhow::Result;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::Instrument;
use tracing::{error, info, info_span, warn};

use uc_core::clipboard::SystemClipboardSnapshot;
use uc_core::ports::clipboard::{
    SelfWriteAttribution, SelfWriteLedgerPort, SelfWriteMatch, SystemClipboardPort,
};

use super::timing::{LOCAL_ECHO_RTT_MAX, REMOTE_ECHO_RTT_MAX};

/// Number of consecutive OS-write failures that trip the circuit.
///
/// Chosen empirically against Sentry issue UNICLIPBOARD-RUST-F (35 minutes /
/// 50 failed writes on a single Windows host whose clipboard was held by an
/// AV/IME process). 5 lets a couple of transient retries through; beyond
/// that the clipboard is clearly unavailable and we stop hammering it.
const CIRCUIT_FAILURE_THRESHOLD: u32 = 5;

/// Default duration the circuit stays open after tripping.
///
/// Long enough that the contending process (AV scan / IME hook / RDP clip
/// proxy) has time to release the clipboard, short enough that recovery is
/// invisible to a typing user. Inbound writes during this window are
/// rejected immediately (no OS call, no guard registration).
///
/// Production `new()` uses this constant; tests use `with_cooldown` to pass
/// a millisecond-scale value so they can exercise the recovery path
/// without literally sleeping 30 seconds.
const DEFAULT_CIRCUIT_OPEN_DURATION: Duration = Duration::from_secs(30);

/// Represents the intent behind a programmatic clipboard write.
///
/// Each variant arms a self-write record under a named echo budget from
/// [`super::timing`] (`LOCAL_ECHO_RTT_MAX` / `REMOTE_ECHO_RTT_MAX`), never as
/// inline literals. The budget is a GC backstop; the next watcher event is what
/// consumes the record:
/// - `LocalRestore`: content record + next-change fallback, local budget
/// - `RemotePush`: content record + next-change fallback, remote budget (OS re-encoding guard)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardWriteIntent {
    /// A local clipboard history restore (user clicked "restore" in history).
    LocalRestore,
    /// A remote push from a peer device arriving via inbound sync.
    RemotePush,
}

/// Single write boundary for all programmatic clipboard writes.
///
/// Centralises the guard-registration + write + cleanup-on-error pattern
/// that was previously duplicated across `restore_clipboard_selection.rs`
/// and `copy_file_to_clipboard.rs` (and the now-deleted libp2p-era
/// `sync_inbound.rs`).
///
/// # Contract
///
/// `write(snapshot, intent)` is the ONLY caller of `snapshot.origin_guard_key()`.
/// Callers build the snapshot and choose the intent; the coordinator handles
/// all guard lifecycle operations.
pub struct ClipboardWriteCoordinator {
    system_clipboard: Arc<dyn SystemClipboardPort>,
    clipboard_change_origin: Arc<dyn SelfWriteLedgerPort>,
    /// True while a `write()` call is in progress. Used by workers to detect
    /// genuinely concurrent clipboard writes, replacing the overly broad
    /// `has_pending_origin()` check that conflated attribution guards with
    /// active write operations.
    writing: AtomicBool,
    /// Number of consecutive OS-write failures since the last success.
    /// Reset to 0 on any successful write; reset to 0 when the circuit trips
    /// (so the next post-cooldown window starts clean).
    consecutive_failures: AtomicU32,
    /// `Some(instant)` while the breaker is open — incoming writes are
    /// rejected without touching the OS clipboard or registering a guard.
    /// `None` means the circuit is closed (normal operation).
    circuit_open_until: Mutex<Option<Instant>>,
    /// How long the breaker stays open after tripping. Per-instance so
    /// tests can use a millisecond-scale value while production uses the
    /// `DEFAULT_CIRCUIT_OPEN_DURATION` constant.
    cooldown: Duration,
}

impl ClipboardWriteCoordinator {
    pub fn new(
        system_clipboard: Arc<dyn SystemClipboardPort>,
        clipboard_change_origin: Arc<dyn SelfWriteLedgerPort>,
    ) -> Self {
        Self::with_cooldown(
            system_clipboard,
            clipboard_change_origin,
            DEFAULT_CIRCUIT_OPEN_DURATION,
        )
    }

    /// Construct with a custom circuit-breaker cooldown. Intended for tests
    /// that need to exercise the recovery path without literally waiting
    /// 30 seconds. Not exposed outside the crate in production builds; use
    /// `new` for normal construction.
    pub(crate) fn with_cooldown(
        system_clipboard: Arc<dyn SystemClipboardPort>,
        clipboard_change_origin: Arc<dyn SelfWriteLedgerPort>,
        cooldown: Duration,
    ) -> Self {
        Self {
            system_clipboard,
            clipboard_change_origin,
            writing: AtomicBool::new(false),
            consecutive_failures: AtomicU32::new(0),
            circuit_open_until: Mutex::new(None),
            cooldown,
        }
    }

    /// Returns `Some(remaining_duration)` if the breaker is currently open,
    /// `None` if the circuit is closed. Also auto-closes the circuit when
    /// the open window has elapsed (lazy cooldown reset).
    fn circuit_check(&self) -> Option<Duration> {
        let mut guard = self.circuit_open_until.lock().expect("poisoned");
        match *guard {
            Some(until) => {
                let now = Instant::now();
                if now >= until {
                    *guard = None;
                    info!(
                        event = "circuit_recovered",
                        reason = "cooldown_elapsed",
                        "clipboard_write_coordinator: circuit breaker closed after cooldown"
                    );
                    None
                } else {
                    Some(until - now)
                }
            }
            None => None,
        }
    }

    /// Record a failed OS write; trip the breaker if the threshold is reached.
    ///
    /// Returns the new consecutive-failure count (post-increment) so the
    /// caller can include it in its own structured error event without
    /// re-reading the atomic. Note: on a trip the atomic is reset to 0,
    /// but the returned value still reflects the pre-reset count — i.e.
    /// "this was failure number N which tripped the circuit".
    fn record_failure(&self) -> u32 {
        let new_count = self.consecutive_failures.fetch_add(1, Ordering::AcqRel) + 1;
        if new_count >= CIRCUIT_FAILURE_THRESHOLD {
            let until = Instant::now() + self.cooldown;
            *self.circuit_open_until.lock().expect("poisoned") = Some(until);
            self.consecutive_failures.store(0, Ordering::Release);
            warn!(
                event = "circuit_tripped",
                error_kind = "circuit_tripped",
                consecutive_failures = new_count,
                threshold = CIRCUIT_FAILURE_THRESHOLD,
                cooldown_ms = self.cooldown.as_millis() as u64,
                "clipboard_write_coordinator: circuit breaker tripped — pausing OS writes"
            );
        }
        new_count
    }

    /// Record a successful OS write — clears the failure counter and any
    /// stale open circuit (defensive; the open path normally lazy-closes).
    ///
    /// Emits an `info` recovery event if the success follows one or more
    /// failures, so operators can pair the "first failure" log line with
    /// a corresponding "recovered" line.
    fn record_success(&self) {
        let prev = self.consecutive_failures.swap(0, Ordering::AcqRel);
        if prev > 0 {
            *self.circuit_open_until.lock().expect("poisoned") = None;
            info!(
                event = "circuit_recovered",
                reason = "success_after_failure",
                recovered_after_failures = prev,
                "clipboard_write_coordinator: OS clipboard write recovered"
            );
        }
    }

    /// Returns `true` while another `write()` call is actively in progress.
    ///
    /// Unlike the previous `has_pending_origin()`, this only returns `true`
    /// during an actual concurrent write — it does NOT trigger on stale
    /// attribution guards left by previously completed writes.
    pub fn is_write_in_progress(&self) -> bool {
        self.writing.load(Ordering::Acquire)
    }

    /// Write a snapshot to the system clipboard with the given intent.
    ///
    /// # Intent semantics
    ///
    /// Each record uses a named echo budget from [`super::timing`]
    /// (`LOCAL_ECHO_RTT_MAX` / `REMOTE_ECHO_RTT_MAX`), never inline, armed via
    /// `SelfWriteLedgerPort::record_self_write`.
    ///
    /// - `LocalRestore`: records a `ByContent`/`Local` self-write under the local
    ///   budget, writes, then records a `ByNextChange`/`Local` fallback to cover
    ///   file URI/path rewrites that change bytes between write and watcher callback.
    /// - `RemotePush`: records a `ByContent`/`Remote` self-write under the remote
    ///   budget, writes, then records a `ByNextChange`/`Remote` fallback to guard
    ///   against OS re-encoding loopback (e.g., Windows DIB→PNG re-encode yields
    ///   bytes the content record won't match). The remote budget is generous
    ///   because that fallback is the sole suppression for a re-encoded echo.
    ///
    /// # Error handling
    ///
    /// On `write_snapshot` failure, the just-armed content record is consumed via
    /// `attribute_observed_change` to prevent stale record accumulation. The
    /// `ByNextChange` fallback is NOT armed on failure.
    pub async fn write(
        &self,
        snapshot: SystemClipboardSnapshot,
        intent: ClipboardWriteIntent,
    ) -> Result<()> {
        self.writing.store(true, Ordering::Release);
        let result = self.write_inner(snapshot, intent).await;
        self.writing.store(false, Ordering::Release);
        result
    }

    async fn write_inner(
        &self,
        snapshot: SystemClipboardSnapshot,
        intent: ClipboardWriteIntent,
    ) -> Result<()> {
        let origin_guard_key = snapshot.origin_guard_key();

        // Circuit breaker check — when the OS clipboard is unavailable
        // (typically held by AV / IME / RDP), keep hammering it produces
        // nothing but Sentry noise (see UNICLIPBOARD-RUST-F: 50 failed
        // writes in 35 minutes on a single host). Skip the OS call entirely
        // and do NOT register a guard — without an actual write the
        // watcher won't fire, so leftover guards would just mis-attribute
        // a future unrelated change.
        if let Some(remaining) = self.circuit_check() {
            warn!(
                event = "circuit_open_skip",
                error_kind = "circuit_open",
                remaining_secs = remaining.as_secs(),
                intent = ?intent,
                origin_guard_key = %origin_guard_key,
                "clipboard_write_coordinator: circuit breaker open — skipping OS write"
            );
            anyhow::bail!(
                "clipboard write skipped: circuit breaker open ({}s remaining)",
                remaining.as_secs()
            );
        }

        async {
            // Arm a content-keyed self-write record before writing.
            match intent {
                ClipboardWriteIntent::LocalRestore => {
                    self.clipboard_change_origin
                        .record_self_write(
                            SelfWriteMatch::ByContent(origin_guard_key.clone()),
                            SelfWriteAttribution::Local,
                            LOCAL_ECHO_RTT_MAX,
                        )
                        .await;
                }
                ClipboardWriteIntent::RemotePush => {
                    self.clipboard_change_origin
                        .record_self_write(
                            SelfWriteMatch::ByContent(origin_guard_key.clone()),
                            SelfWriteAttribution::Remote,
                            REMOTE_ECHO_RTT_MAX,
                        )
                        .await;
                }
            }

            // Attempt the write. Log and unwind the guard on failure so subsequent
            // writes start from a clean slate (and so outages surface in Seq/stdout
            // instead of being hidden in the `Err` bubbling back up the call chain).
            if let Err(err) = self.system_clipboard.write_snapshot(snapshot) {
                // Unwind the just-armed content record so a later unrelated
                // change is not mis-attributed to this failed write.
                self.clipboard_change_origin
                    .attribute_observed_change(&origin_guard_key)
                    .await;
                // Record failure first so the error event below can include
                // the post-increment count and `circuit_tripped` derived state.
                // Whoever reads Sentry can pair `error_kind=os_write_failed`
                // with `consecutive_failures` and `circuit_tripped` to see
                // immediately whether this is the Nth failure in a row.
                let consecutive_failures = self.record_failure();
                let circuit_tripped = consecutive_failures >= CIRCUIT_FAILURE_THRESHOLD;
                error!(
                    event = "os_write_failed",
                    error_kind = "os_write_failed",
                    error = %err,
                    intent = ?intent,
                    origin_guard_key = %origin_guard_key,
                    consecutive_failures,
                    threshold = CIRCUIT_FAILURE_THRESHOLD,
                    circuit_tripped,
                    "clipboard_write_coordinator: OS clipboard write failed"
                );
                return Err(err);
            }

            self.record_success();

            match intent {
                ClipboardWriteIntent::LocalRestore => {
                    // File restores can come back from the platform clipboard with rewritten
                    // URI/path bytes, so the content record alone is not sufficient. Pair the
                    // fallback to this write via its guard key so a repeat write of the same
                    // snapshot coalesces instead of leaving a stray fallback behind.
                    self.clipboard_change_origin
                        .record_self_write(
                            SelfWriteMatch::ByNextChange(origin_guard_key.clone()),
                            SelfWriteAttribution::Local,
                            LOCAL_ECHO_RTT_MAX,
                        )
                        .await;
                }
                ClipboardWriteIntent::RemotePush => {
                    // Some platforms (e.g. Windows clipboard-rs) re-encode images (PNG→DIB→PNG),
                    // producing different bytes than the original. The content record above won't
                    // match the re-encoded content, so we arm a next-change record: the NEXT
                    // clipboard change is treated as RemotePush regardless of hash. The guard key
                    // pairs the fallback to this write so a duplicated push of the same snapshot
                    // coalesces instead of leaking a fallback that would swallow a later copy.
                    self.clipboard_change_origin
                        .record_self_write(
                            SelfWriteMatch::ByNextChange(origin_guard_key.clone()),
                            SelfWriteAttribution::Remote,
                            REMOTE_ECHO_RTT_MAX,
                        )
                        .await;
                }
            }

            Ok(())
        }
        .instrument(info_span!(
            "clipboard_write_coordinator.write",
            intent = ?intent,
            origin_guard_key = %origin_guard_key
        ))
        .await
    }
}

#[cfg(test)]
mod tests {
    //! Circuit-breaker behavioural tests.
    //!
    //! These exercise the three states transitions added in response to
    //! UNICLIPBOARD-RUST-F (50 OS-clipboard failures on a single Windows
    //! host in 35 minutes):
    //!
    //! 1. Five consecutive failures trip the breaker; the sixth call MUST
    //!    short-circuit without invoking the OS port at all.
    //! 2. After the cooldown elapses the breaker auto-closes and subsequent
    //!    writes reach the OS again.
    //! 3. A single success resets the failure counter so a subsequent
    //!    streak doesn't inherit stale credit.
    //!
    //! Tests run on the tokio current-thread runtime; cooldown is shortened
    //! via `with_cooldown` to keep wall-clock waits in the ms range.
    use super::*;
    use async_trait::async_trait;
    use mockall::mock;
    use std::sync::atomic::AtomicU32;
    use uc_core::clipboard::ClipboardChangeOrigin;
    use uc_core::ids::{FormatId, RepresentationId};
    use uc_core::{MimeType, ObservedClipboardRepresentation};

    mock! {
        SystemClipboard {}
        #[async_trait]
        impl SystemClipboardPort for SystemClipboard {
            fn read_snapshot(&self) -> Result<SystemClipboardSnapshot>;
            fn write_snapshot(&self, snapshot: SystemClipboardSnapshot) -> Result<()>;
        }
    }

    mock! {
        ChangeOrigin {}
        #[async_trait]
        impl SelfWriteLedgerPort for ChangeOrigin {
            async fn record_self_write(
                &self,
                matching: SelfWriteMatch,
                attribution: SelfWriteAttribution,
                ttl: Duration,
            );
            async fn attribute_observed_change(&self, snapshot_hash: &str) -> ClipboardChangeOrigin;
        }
    }

    fn make_snapshot() -> SystemClipboardSnapshot {
        SystemClipboardSnapshot {
            ts_ms: 0,
            representations: vec![ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("text"),
                Some(MimeType("text/plain".to_string())),
                b"x".to_vec(),
            )],
            file_content_digests: Vec::new(),
        }
    }

    fn permissive_origin_mock() -> MockChangeOrigin {
        // Origin port is exercised but its specific calls are not what we're
        // testing here — accept any call count, return defaults.
        let mut origin = MockChangeOrigin::new();
        origin.expect_record_self_write().returning(|_, _, _| ());
        origin
            .expect_attribute_observed_change()
            .returning(|_| ClipboardChangeOrigin::LocalCapture);
        origin
    }

    /// 5 consecutive OS failures must trip the breaker, after which the
    /// 6th call short-circuits without invoking the underlying port.
    /// Mockall's `.times(5)` asserts exactly 5 OS calls — if the 6th
    /// leaked through, mock expectations would fail on drop.
    #[tokio::test]
    async fn five_failures_trip_breaker_and_block_sixth_call() {
        let mut clipboard = MockSystemClipboard::new();
        clipboard
            .expect_write_snapshot()
            .times(5)
            .returning(|_| Err(anyhow::anyhow!("simulated OS clipboard failure")));

        let coord =
            ClipboardWriteCoordinator::new(Arc::new(clipboard), Arc::new(permissive_origin_mock()));

        // First five calls go to the OS and each fail.
        for i in 0..5 {
            let r = coord
                .write(make_snapshot(), ClipboardWriteIntent::RemotePush)
                .await;
            assert!(r.is_err(), "call {i} should propagate OS Err");
        }

        // Sixth call must NOT reach the mock (times(5) would fail otherwise).
        let err = coord
            .write(make_snapshot(), ClipboardWriteIntent::RemotePush)
            .await
            .expect_err("breaker should reject 6th call");
        assert!(
            err.to_string().contains("circuit breaker open"),
            "expected circuit-open error, got: {err}"
        );
    }

    /// After cooldown elapses the breaker auto-closes; the next call
    /// reaches the OS and (in this test) succeeds.
    #[tokio::test]
    async fn breaker_recovers_after_cooldown() {
        let counter = Arc::new(AtomicU32::new(0));
        let c = counter.clone();

        let mut clipboard = MockSystemClipboard::new();
        // 5 fails to trip + 1 success after cooldown = 6 OS calls total.
        clipboard
            .expect_write_snapshot()
            .times(6)
            .returning(move |_| {
                let n = c.fetch_add(1, Ordering::SeqCst);
                if n < 5 {
                    Err(anyhow::anyhow!("simulated OS failure"))
                } else {
                    Ok(())
                }
            });

        let coord = ClipboardWriteCoordinator::with_cooldown(
            Arc::new(clipboard),
            Arc::new(permissive_origin_mock()),
            Duration::from_millis(80),
        );

        // Trip the breaker.
        for _ in 0..5 {
            let _ = coord
                .write(make_snapshot(), ClipboardWriteIntent::RemotePush)
                .await;
        }

        // Immediate retry blocked by open breaker.
        let err = coord
            .write(make_snapshot(), ClipboardWriteIntent::RemotePush)
            .await
            .expect_err("breaker should still be open");
        assert!(
            err.to_string().contains("circuit breaker open"),
            "got: {err}"
        );

        // Wait past cooldown — slightly more than 80ms to avoid flakiness.
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Now the breaker should be closed; write reaches OS and succeeds.
        coord
            .write(make_snapshot(), ClipboardWriteIntent::RemotePush)
            .await
            .expect("write should succeed after cooldown");
    }

    /// A single intervening success resets the consecutive-failure counter,
    /// so a 4-fail / success / 4-fail pattern (8 fails total but no
    /// 5-in-a-row streak) must NOT trip the breaker.
    #[tokio::test]
    async fn success_resets_consecutive_failure_counter() {
        let counter = Arc::new(AtomicU32::new(0));
        let c = counter.clone();

        let mut clipboard = MockSystemClipboard::new();
        // Pattern: fail x4, ok, fail x4, ok — 10 total OS calls, no trip.
        // If counter wasn't reset by the first success, the 5th overall fail
        // (which is the 4th in the second streak) would trip and the 10th
        // call would be short-circuited, failing `.times(10)`.
        clipboard
            .expect_write_snapshot()
            .times(10)
            .returning(move |_| {
                let n = c.fetch_add(1, Ordering::SeqCst);
                if n == 4 || n == 9 {
                    Ok(())
                } else {
                    Err(anyhow::anyhow!("simulated OS failure"))
                }
            });

        let coord =
            ClipboardWriteCoordinator::new(Arc::new(clipboard), Arc::new(permissive_origin_mock()));

        for _ in 0..10 {
            let _ = coord
                .write(make_snapshot(), ClipboardWriteIntent::RemotePush)
                .await;
        }
        // Mock's times(10) asserts every call reached OS — none short-circuited.
    }
}
