//! SyncEngine decision core — the pure half of uc-ios `Sync/SyncEngine.swift`
//! (968 lines).
//!
//! Split per goal-B M5 (user 2026-06-14): the **decision core** lives here as a
//! [`SyncRuntimeState`] plain struct plus pure transition functions; the
//! **execution shell** (1 Hz tick loop, scenePhase, `UIPasteboard` I/O, the
//! server round-trips, banners, prefetch) stays native. The shell drives one
//! tick as a reducer — plan, do I/O, commit:
//!
//! 1. [`plan_preamble`] — local-observation decision + early-exit + network
//!    backoff gate + cross-process watermark resync (mirrors `tick` 434-477).
//! 2. native: `getClipboard()` (the server round-trip).
//! 3. [`plan_after_server_get`] — truth-gate / server-new route / push route
//!    (`tick` 497-521).
//! 4. native: apply / push byte I/O.
//! 5. `commit_*` — fold the I/O outcome back into the state (advance the synced
//!    hash, record the loop-guard event, detect a trip).
//!
//! All non-determinism (wall clock, jitter) is injected as a parameter — the
//! same boundary M4 used for UUID/`Date.now()`. Timestamps are
//! epoch-milliseconds. Hashes are stored uppercased (Swift `hashesEqual` is
//! case-insensitive).
//!
//! Delivery boundary (M5): proto-only pure logic + unit tests, no `uniffi`
//! derive — the FFI mirror lands with the native wire-up in M6, same as M4.

use crate::clipboard_doc::Clipboard;
use crate::loop_guard::{
    self, LoopDirection, LoopGuardEvent, DEFAULT_FLIP_THRESHOLD, DEFAULT_WINDOW_SECS,
};

/// Visible sync state — mirror of Swift `SyncEngine.State`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncState {
    /// No active server, or freshly reset.
    Idle,
    /// Last tick converged cleanly.
    Succeeded,
    /// Server has new content the user hasn't applied (auto-apply off).
    HasNewUnwritten,
    /// A network-shaped failure; the network half is backing off.
    OfflineRetrying,
    /// 401 — the loop is paused until credentials are fixed.
    AuthFailed,
    /// The loop guard tripped; paused until the user acknowledges.
    LoopDetected,
}

/// Cadence / backoff / loop-guard tunables. Defaults match the Swift literals
/// exactly (`SyncEngine` public knobs).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SyncConfig {
    /// Foreground cadence (Swift `normalCadenceSeconds`).
    pub normal_cadence_secs: f64,
    /// Scene-inactive cadence (Swift `inactiveCadenceSeconds`).
    pub inactive_cadence_secs: f64,
    /// First-error backoff (Swift `offlineBackoffSeconds`).
    pub offline_backoff_secs: f64,
    /// Backoff ceiling (Swift `offlineBackoffMaxSeconds`).
    pub offline_backoff_max_secs: f64,
    /// History-sync throttle (Swift `historySyncInterval`).
    pub history_sync_interval_secs: f64,
    /// Loop-guard window (Swift `SyncLoopGuard.window`).
    pub loop_window_secs: f64,
    /// Loop-guard flip threshold (Swift `SyncLoopGuard.flipThreshold`).
    pub loop_flip_threshold: i64,
}

impl Default for SyncConfig {
    fn default() -> Self {
        SyncConfig {
            normal_cadence_secs: 1.0,
            inactive_cadence_secs: 5.0,
            offline_backoff_secs: 5.0,
            offline_backoff_max_secs: 60.0,
            history_sync_interval_secs: 30.0,
            loop_window_secs: DEFAULT_WINDOW_SECS,
            loop_flip_threshold: DEFAULT_FLIP_THRESHOLD,
        }
    }
}

/// Per-server runtime state, owned by the caller (native shell / M5 tests).
///
/// Decision-relevant fields only. UI fields (`lastSyncedAt`, `lastError`,
/// `isExplicitlyRefreshing`, `stagedEntry` highlight) and concurrency locks
/// (`isTicking`, `isHistorySyncing`) stay native. Persistence of
/// `last_synced_hash` / `last_history_sync_ms` (Swift writes them to the App
/// Group) is the native shell's job — the core only mutates the in-memory
/// values.
#[derive(Debug, Clone, PartialEq)]
pub struct SyncRuntimeState {
    /// Visible state.
    pub state: SyncState,
    /// Dedup guard #1 — prevents re-pulling content already synced. Uppercase.
    /// Mirror of the native-persisted `lastSyncedContentHash`.
    pub last_synced_hash: Option<String>,
    /// Dedup guard #2 — prevents pushing back content we just wrote to the
    /// pasteboard ourselves. Uppercase.
    pub last_applied_hash: Option<String>,
    /// Loop-guard event buffer (owned per [`loop_guard`]'s caller-holds-Vec
    /// model).
    pub loop_events: Vec<LoopGuardEvent>,
    /// Hash already downloaded but not yet written — dedups the bytes fetch
    /// when auto-apply is off and the server hash is unchanged. Uppercase only
    /// where the source hash was; stored verbatim from `entry.hash`.
    pub staged_server_hash: Option<String>,
    /// The full staged entry — only consulted for hashless dedup (a §4 spec
    /// violation where the server omits the SHA-256).
    pub staged_entry: Option<Clipboard>,
    /// Consecutive failed ticks — drives the exponential backoff.
    pub consecutive_failures: i64,
    /// Earliest epoch-ms the next network attempt may run; `None` = no gate.
    pub next_attempt_ms: Option<i64>,
    /// Last history-sync epoch-ms; `None` fires an immediate first sync.
    pub last_history_sync_ms: Option<i64>,
}

impl Default for SyncRuntimeState {
    fn default() -> Self {
        SyncRuntimeState {
            state: SyncState::Idle,
            last_synced_hash: None,
            last_applied_hash: None,
            loop_events: Vec::new(),
            staged_server_hash: None,
            staged_entry: None,
            consecutive_failures: 0,
            next_attempt_ms: None,
            last_history_sync_ms: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Preamble (tick first half — before the server round-trip)
// ---------------------------------------------------------------------------

/// What the shell observed locally before the network round-trip.
#[derive(Debug, Clone, PartialEq)]
pub struct PreambleSnapshot {
    /// User-explicit tick (pull-to-refresh / toolbar) — punches through the
    /// backoff gate.
    pub explicit: bool,
    /// `appSettings.autoPushDeviceChanges`.
    pub auto_push: bool,
    /// Whether an active server is configured (`vm.activeServer != nil`).
    pub has_active_server: bool,
    /// Current pasteboard content hash (native polled). `None` = nothing
    /// observed yet.
    pub device_hash: Option<String>,
    /// `history.first?.entry.hash` — the `isHashInRecentHistory` check.
    pub history_head_hash: Option<String>,
    /// `store.loadLastSyncedHash()` — cross-process resync source (the Share
    /// Extension writes this key directly).
    pub persisted_synced_hash: Option<String>,
    /// Wall clock, epoch-ms.
    pub now_ms: i64,
}

/// Plan emitted by [`plan_preamble`].
#[derive(Debug, Clone, PartialEq)]
pub struct Preamble {
    /// Auto-push ON observed fresh local content not written by us — the shell
    /// should seed the local payload cache, then append a `.local` history row.
    pub record_local: bool,
    /// Whether to continue to the server round-trip.
    pub proceed: PreambleProceed,
}

/// Whether the preamble lets the tick proceed to the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreambleProceed {
    /// Stop here, for the given reason.
    Stop(StopReason),
    /// Continue to `getClipboard()`.
    ToNetwork,
}

/// Why the preamble stopped the tick before the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// No active server — state set to `Idle`.
    NoActiveServer,
    /// `.authFailed` / `.loopDetected` — the loop is paused, state untouched.
    Paused,
    /// Inside the network backoff window — local observation already ran, the
    /// server round-trip is throttled; state untouched.
    BackoffGate,
}

/// Tick preamble (`tick` 434-477): decide the local-history record, run the
/// early-exit guards, and — on the proceed path — fold the cross-process
/// persisted hash back into `last_synced_hash`.
///
/// Mutates `st` for the cross-process resync only (and only when proceeding to
/// the network, matching the Swift order: the resync sits after the backoff
/// gate).
pub fn plan_preamble(st: &mut SyncRuntimeState, snap: &PreambleSnapshot) -> Preamble {
    // Pasteboard observation runs before the server guard, so local content is
    // recorded even without a configured server (`tick` 436-456). We log a
    // `.local` row only for content that is genuinely new (not our own apply,
    // not already at the history head).
    let record_local = snap.auto_push
        && snap.device_hash.as_deref().is_some_and(|h| {
            !h.is_empty()
                && !hashes_equal(Some(h), st.last_applied_hash.as_deref())
                && !hashes_equal(Some(h), snap.history_head_hash.as_deref())
        });

    let proceed = if !snap.has_active_server {
        // `tick` 457-459.
        st.state = SyncState::Idle;
        PreambleProceed::Stop(StopReason::NoActiveServer)
    } else if matches!(st.state, SyncState::AuthFailed | SyncState::LoopDetected) {
        // `tick` 461.
        PreambleProceed::Stop(StopReason::Paused)
    } else if !snap.explicit && st.next_attempt_ms.is_some_and(|next| snap.now_ms < next) {
        // `tick` 465 — network backoff gate; explicit refreshes punch through.
        PreambleProceed::Stop(StopReason::BackoffGate)
    } else {
        // `tick` 474-477 — cross-process re-sync before the round-trip.
        if !hashes_equal(
            snap.persisted_synced_hash.as_deref(),
            st.last_synced_hash.as_deref(),
        ) {
            st.last_synced_hash = snap.persisted_synced_hash.as_deref().map(str::to_uppercase);
        }
        PreambleProceed::ToNetwork
    };

    Preamble {
        record_local,
        proceed,
    }
}

// ---------------------------------------------------------------------------
// Routing (after the server round-trip)
// ---------------------------------------------------------------------------

/// What the shell holds after `getClipboard()`.
#[derive(Debug, Clone, PartialEq)]
pub struct ServerGetSnapshot {
    /// `appSettings.autoApplyServerChanges`.
    pub auto_apply: bool,
    /// `appSettings.autoPushDeviceChanges`.
    pub auto_push: bool,
    /// Server latest entry; `None` when the server returned 404 (empty server).
    pub server_entry: Option<Clipboard>,
    /// Whether the device pasteboard has been observed (`vm.deviceClipboard !=
    /// nil`).
    pub device_present: bool,
    /// `vm.deviceClipboard?.hash`.
    pub device_hash: Option<String>,
}

/// Routing verdict — which branch the tick takes after the server GET
/// (`tick` 497-521).
#[derive(Debug, Clone, PartialEq)]
pub enum ServerRoute {
    /// Truth-gate (`tick` 497-514): server latest and device clipboard hold
    /// identical content. Already converged; the shell calls
    /// [`commit_converged`] with this (uppercased) hash.
    Converged { server_hash: String },
    /// Server has new content (server hash != `last_synced_hash`,
    /// `tick` 515-517 → `processServerNew`).
    ServerNew(ServerNewPlan),
    /// Server unchanged — fall through to the push side (`tick` 519-520 →
    /// `maybePush`).
    Push(PushDecision),
}

/// Sub-plan for the server-has-new branch (`processServerNew` 614-633).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerNewPlan {
    /// We already fetched this hash and are waiting on the user — skip the
    /// bytes round-trip, the `.pulled` history log, and the prefetch.
    pub already_staged: bool,
    /// `autoApply && entry has hash` — the shell applies, then calls
    /// [`commit_apply`]. When `false`: stage-only ([`commit_stage`]) if not
    /// already staged, else a no-op tick.
    pub will_apply: bool,
}

/// Push decision (`maybePush` 691-729).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushDecision {
    /// Consent-push mode — auto-push is off; the engine never reads the
    /// pasteboard on its own.
    SkipConsentMode,
    /// The observer hasn't surfaced any device clipboard yet.
    SkipNoDevice,
    /// Device content already matches `last_synced_hash`.
    SkipAlreadySynced,
    /// Device content is what we just wrote ourselves — pushing it back would
    /// start the apply↔push pong.
    SkipSelfWritten,
    /// Push it — the shell PUTs, then calls [`commit_push`].
    DoPush,
}

/// Decide the post-`getClipboard` route (`tick` 497-521).
pub fn plan_after_server_get(st: &SyncRuntimeState, snap: &ServerGetSnapshot) -> ServerRoute {
    // Truth-gate: server latest == device clipboard, both with a non-empty
    // hash. Converged regardless of what the watermark says (`tick` 497-500).
    if let Some(entry) = &snap.server_entry {
        let server_hash = entry.hash.as_deref().filter(|h| !h.is_empty());
        let device_hash = snap.device_hash.as_deref().filter(|h| !h.is_empty());
        if let (Some(sh), Some(dh)) = (server_hash, device_hash) {
            if hashes_equal(Some(sh), Some(dh)) {
                return ServerRoute::Converged {
                    server_hash: sh.to_uppercase(),
                };
            }
        }
    }
    // Server has new content (`tick` 515-516).
    if let Some(entry) = &snap.server_entry {
        if !hashes_equal(entry.hash.as_deref(), st.last_synced_hash.as_deref()) {
            return ServerRoute::ServerNew(plan_server_new(st, snap.auto_apply, entry));
        }
    }
    // Push side (`tick` 519-520).
    ServerRoute::Push(plan_push(st, snap))
}

/// Server-has-new sub-plan (`processServerNew` 614-633).
fn plan_server_new(st: &SyncRuntimeState, auto_apply: bool, entry: &Clipboard) -> ServerNewPlan {
    let entry_has_hash = entry.hash.as_deref().is_some_and(|h| !h.is_empty());
    let already_staged = if entry_has_hash {
        st.staged_server_hash
            .as_deref()
            .is_some_and(|s| hashes_equal(Some(s), entry.hash.as_deref()))
    } else {
        // Hashless dedup by full-Clipboard equality (`processServerNew` 617).
        st.staged_entry.as_ref() == Some(entry)
    };
    ServerNewPlan {
        already_staged,
        will_apply: auto_apply && entry_has_hash,
    }
}

/// Push sub-decision (`maybePush` 691-729).
fn plan_push(st: &SyncRuntimeState, snap: &ServerGetSnapshot) -> PushDecision {
    if !snap.auto_push {
        return PushDecision::SkipConsentMode;
    }
    if !snap.device_present {
        return PushDecision::SkipNoDevice;
    }
    if hashes_equal(snap.device_hash.as_deref(), st.last_synced_hash.as_deref()) {
        return PushDecision::SkipAlreadySynced;
    }
    if hashes_equal(snap.device_hash.as_deref(), st.last_applied_hash.as_deref()) {
        return PushDecision::SkipSelfWritten;
    }
    PushDecision::DoPush
}

// ---------------------------------------------------------------------------
// Commits (fold the I/O outcome back into the state)
// ---------------------------------------------------------------------------

/// Outcome of a commit that records a loop-guard event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommitOutcome {
    /// The loop guard tripped — the shell should stop the loop (Swift
    /// `tripLoopBreaker` calls `stop()`) and the state is set to
    /// [`SyncState::LoopDetected`] on the apply, push, and consent-push paths
    /// alike.
    pub tripped: bool,
}

/// Truth-gate commit (`tick` 508-514): repair the watermark, mark applied,
/// clear staged, succeed. No loop-guard event — nothing actually flowed.
pub fn commit_converged(st: &mut SyncRuntimeState, server_hash: &str) {
    advance_synced(st, Some(server_hash));
    st.last_applied_hash = upper_nonempty(Some(server_hash));
    st.staged_server_hash = None;
    st.staged_entry = None;
    st.state = SyncState::Succeeded;
}

/// Apply commit (`processServerNew` 660-668): a server entry was written to the
/// pasteboard. Advance synced, mark applied, clear staged, record a `.pulled`
/// loop event, detect a trip.
pub fn commit_apply(
    st: &mut SyncRuntimeState,
    hash: Option<&str>,
    now_ms: i64,
    cfg: &SyncConfig,
) -> CommitOutcome {
    advance_synced(st, hash);
    st.last_applied_hash = upper_nonempty(hash);
    st.staged_server_hash = None;
    st.staged_entry = None;
    st.state = SyncState::Succeeded;
    record_and_check(st, LoopDirection::Pulled, hash, now_ms, cfg)
}

/// Apply-failed commit (`processServerNew` 656-657): park the entry in the
/// staged slot so the next tick short-circuits via `already_staged`. State
/// stays whatever the tick error handler sets ([`commit_tick_failure`]).
pub fn commit_apply_failed(st: &mut SyncRuntimeState, entry: &Clipboard) {
    st.staged_server_hash = entry.hash.clone();
    st.staged_entry = Some(entry.clone());
}

/// Stage-only commit (`processServerNew` 674-676): auto-apply off (or hashless)
/// and not already staged — stash the entry, surface `HasNewUnwritten`.
pub fn commit_stage(st: &mut SyncRuntimeState, entry: &Clipboard) {
    st.staged_server_hash = entry.hash.clone();
    st.staged_entry = Some(entry.clone());
    st.state = SyncState::HasNewUnwritten;
}

/// Push commit (`maybePush` 741-758). `pushed_hash` is the hash of the entry
/// that was actually PUT, or `None` for a documented silent skip (no snapshot /
/// unpushable type / in-flight push).
///
/// A push that trips the loop guard sticks as [`SyncState::LoopDetected`], the
/// same as the apply / consent-push paths. This corrects an iOS inconsistency:
/// Swift recorded the `.pushed` loop event and called `tripLoopBreaker()` (which
/// set `.loopDetected`) *before* the unconditional `state = .succeeded` at line
/// 756, silently overwriting a push-direction trip back to `.succeeded` (the
/// apply path ordered these the other way, so its trip stuck). The fix lands in
/// both this reducer and the native `SyncEngine.swift`.
pub fn commit_push(
    st: &mut SyncRuntimeState,
    pushed_hash: Option<&str>,
    now_ms: i64,
    cfg: &SyncConfig,
) -> CommitOutcome {
    let Some(h) = pushed_hash.filter(|h| !h.is_empty()) else {
        // Documented silent skip: nothing flowed, the tick stays healthy.
        st.state = SyncState::Succeeded;
        return CommitOutcome { tripped: false };
    };
    advance_synced(st, Some(h));
    // maybePush deliberately does NOT set last_applied / clear staged (unlike
    // the apply path); a trip in `record_and_check` overrides this to
    // `LoopDetected`.
    st.state = SyncState::Succeeded;
    record_and_check(st, LoopDirection::Pushed, Some(h), now_ms, cfg)
}

/// Push-skip commit — any `maybePush` skip branch leaves the tick healthy
/// (`state = .succeeded`). `lastSyncedAt` differences between the skip reasons
/// are a native UI concern.
pub fn commit_push_skipped(st: &mut SyncRuntimeState) {
    st.state = SyncState::Succeeded;
}

/// Consent-push commit (`consentPush` 787-796): the user handed us bytes via
/// the paste control. Unlike [`commit_push`] this DOES set `last_applied_hash`
/// (we wrote these bytes to the pasteboard ourselves) and resets the failure
/// counter; it also orders the trip check before the success state, so here a
/// trip sticks as `LoopDetected`.
pub fn commit_consent_push(
    st: &mut SyncRuntimeState,
    pushed_hash: Option<&str>,
    now_ms: i64,
    cfg: &SyncConfig,
) -> CommitOutcome {
    advance_synced(st, pushed_hash);
    st.last_applied_hash = upper_nonempty(pushed_hash);
    let outcome = record_and_check(st, LoopDirection::Pushed, pushed_hash, now_ms, cfg);
    if !outcome.tripped {
        st.state = SyncState::Succeeded;
        st.consecutive_failures = 0;
    }
    outcome
}

/// Happy-path tick tail (`tick` 539-544): a healthy tick drops the backoff
/// counter so a recovered network reverts to the normal cadence.
pub fn commit_tick_success(st: &mut SyncRuntimeState) {
    st.consecutive_failures = 0;
    st.next_attempt_ms = None;
}

/// Error classes the tick handler distinguishes (`tick` 545-598). The native
/// shell maps its `SyncError.kind` (and non-`SyncError` panics) onto these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickErrorKind {
    /// 401 — pause the loop (state → `AuthFailed`).
    AuthFailed,
    /// Deliberate abort from a network-route / endpoint change — no backoff, no
    /// state flip.
    Cancelled,
    /// A dead route — back off and re-probe.
    NetworkUnreachable,
    /// Connect timed out — back off and re-probe.
    ConnectTimeout,
    /// Receive timed out — back off and re-probe.
    ReceiveTimeout,
    /// Any other mapped `SyncError` — back off, but do not re-probe.
    OtherSyncError,
    /// A non-`SyncError` escaped the tick (bug-grade) — back off, re-probe, and
    /// the native shell captures it to Sentry.
    Unexpected,
}

/// What the shell should do after a failed tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickFailureOutcome {
    /// Ask the VM to re-probe the live endpoints (§5.3 layer 2).
    pub kick_probe: bool,
    /// This is the offline TRANSITION (`consecutive_failures` just became 1) —
    /// the shell mirrors it to Sentry once.
    pub first_offline: bool,
}

/// Tick error handler (`tick` 545-598). `jitter` is native-supplied
/// (`Double.random(in: 0.8...1.2)`), injected for determinism.
pub fn commit_tick_failure(
    st: &mut SyncRuntimeState,
    kind: TickErrorKind,
    jitter: f64,
    now_ms: i64,
    cfg: &SyncConfig,
) -> TickFailureOutcome {
    match kind {
        TickErrorKind::Cancelled => {
            // `tick` 551-558 — not a failure: no backoff, no state flip.
            TickFailureOutcome {
                kick_probe: false,
                first_offline: false,
            }
        }
        TickErrorKind::AuthFailed => {
            // `tick` 545-550 — pause; native stops the loop (cadence is ∞).
            st.state = SyncState::AuthFailed;
            TickFailureOutcome {
                kick_probe: false,
                first_offline: false,
            }
        }
        _ => {
            st.consecutive_failures += 1;
            let backoff = backoff_secs(
                st.consecutive_failures,
                cfg.offline_backoff_secs,
                cfg.offline_backoff_max_secs,
                jitter,
            );
            st.next_attempt_ms = Some(now_ms + (backoff * 1000.0) as i64);
            st.state = SyncState::OfflineRetrying;
            // Generic `SyncError` (`tick` 580-585) re-probes only on the three
            // network-shaped kinds; the non-`SyncError` fallthrough
            // (`tick` 597) always re-probes.
            let kick_probe = matches!(
                kind,
                TickErrorKind::NetworkUnreachable
                    | TickErrorKind::ConnectTimeout
                    | TickErrorKind::ReceiveTimeout
                    | TickErrorKind::Unexpected
            );
            TickFailureOutcome {
                kick_probe,
                first_offline: st.consecutive_failures == 1,
            }
        }
    }
}

/// Record that a history-sync round ran (`runHistorySyncIfDue` defer 883-887).
pub fn commit_history_sync_done(st: &mut SyncRuntimeState, now_ms: i64) {
    st.last_history_sync_ms = Some(now_ms);
}

// ---------------------------------------------------------------------------
// State transitions (Swift public methods, pure parts)
// ---------------------------------------------------------------------------

/// User manually applied the staged entry (`markStagedApplied` 260-269).
/// Returns `false` (no-op) when nothing is staged.
pub fn mark_staged_applied(st: &mut SyncRuntimeState) -> bool {
    let Some(hash) = st.staged_server_hash.clone() else {
        return false;
    };
    advance_synced(st, Some(&hash));
    st.last_applied_hash = upper_nonempty(Some(&hash));
    st.staged_server_hash = None;
    st.staged_entry = None;
    st.state = SyncState::Succeeded;
    true
}

/// User dismissed the loop-detected banner (`acknowledgeLoopDetection`
/// 276-281): wipe the cycle buffer, back to idle. The native shell restarts the
/// loop afterwards.
pub fn acknowledge_loop_detection(st: &mut SyncRuntimeState) {
    st.loop_events.clear();
    st.state = SyncState::Idle;
}

/// Clear per-server runtime state without touching the persisted synced hash
/// (`resetRuntimeState` 286-306). The native shell separately clears the
/// history watermark and persists `lastHistorySyncAt = nil`.
pub fn reset_runtime_state(st: &mut SyncRuntimeState) {
    st.staged_server_hash = None;
    st.staged_entry = None;
    st.last_applied_hash = None;
    st.loop_events.clear();
    st.state = SyncState::Idle;
    st.next_attempt_ms = None;
    st.consecutive_failures = 0;
    st.last_history_sync_ms = None;
}

/// Active server changed (`handleActiveServerChanged` 323-332): full reset plus
/// clearing the synced hash — the new server has its own content timeline.
pub fn handle_active_server_changed(st: &mut SyncRuntimeState) {
    reset_runtime_state(st);
    st.last_synced_hash = None;
}

/// Network route changed / endpoint flipped (`handleNetworkRouteChanged`
/// 344-348, `handleEndpointChanged` 357-362): the backoff accumulated against a
/// dead route says nothing about the new one — clear it.
pub fn handle_network_route_changed(st: &mut SyncRuntimeState) {
    st.consecutive_failures = 0;
    st.next_attempt_ms = None;
}

// ---------------------------------------------------------------------------
// Pure helpers (no state)
// ---------------------------------------------------------------------------

/// Case-insensitive hash comparison treating `None == None` (`hashesEqual`
/// 961-967).
pub fn hashes_equal(a: Option<&str>, b: Option<&str>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(l), Some(r)) => l.eq_ignore_ascii_case(r),
        _ => false,
    }
}

/// Exponential backoff with caller-supplied jitter (`currentBackoffSeconds`
/// 390-401). `jitter` is `Double.random(in: 0.8...1.2)`.
pub fn backoff_secs(consecutive_failures: i64, base: f64, max: f64, jitter: f64) -> f64 {
    let failures = consecutive_failures.max(1);
    // 2^(failures-1) capped at 2^6, matching the Swift exponent clamp.
    let exponent = (failures - 1).min(6);
    let multiplier = 2f64.powi(exponent as i32);
    let capped = (base * multiplier).min(max);
    capped * jitter
}

/// Cadence for the current state (`cadenceSeconds` 364-375). `AuthFailed` /
/// `LoopDetected` return infinity — the native loop breaks on that.
pub fn cadence_secs(state: SyncState, is_scene_inactive: bool, cfg: &SyncConfig) -> f64 {
    match state {
        SyncState::AuthFailed | SyncState::LoopDetected => f64::INFINITY,
        _ => {
            if is_scene_inactive {
                cfg.inactive_cadence_secs
            } else {
                cfg.normal_cadence_secs
            }
        }
    }
}

/// Whether a history-sync round is due (`runHistorySyncIfDue` 872-875).
pub fn is_history_sync_due(last_sync_ms: Option<i64>, now_ms: i64, interval_secs: f64) -> bool {
    match last_sync_ms {
        None => true,
        Some(last) => (now_ms - last) as f64 / 1000.0 >= interval_secs,
    }
}

/// Cold-start = no history watermark yet (`runHistorySyncIfDue` 890): fetch only
/// page 1 and seed the watermark instead of paginating the whole server history.
pub fn is_cold_start(watermark_ms: Option<i64>) -> bool {
    watermark_ms.is_none()
}

/// New watermark after a history round (`runHistorySyncIfDue` 931-933): advance
/// to `max(current, max_last_modified)`. Returns `Some(new)` only when it moved.
pub fn advance_watermark(current_ms: Option<i64>, max_last_modified_ms: i64) -> Option<i64> {
    match current_ms {
        Some(c) if max_last_modified_ms <= c => None,
        _ => Some(max_last_modified_ms),
    }
}

/// A probe verdict is valid only while the network path it was captured under is
/// still current (§5.3). M3 stamps the `ProbeReport` epoch; M5 validates it.
pub fn is_probe_conclusion_valid(report_epoch: u64, current_epoch: u64) -> bool {
    report_epoch == current_epoch
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// `advanceSynced` (839-849): a nil/empty hash is unverifiable — leave the
/// watermark alone so the next tick re-evaluates. Otherwise store uppercased.
fn advance_synced(st: &mut SyncRuntimeState, hash: Option<&str>) {
    if let Some(h) = hash {
        if !h.is_empty() {
            st.last_synced_hash = Some(h.to_uppercase());
        }
    }
}

/// `hash?.uppercased()` with empty treated as absent.
fn upper_nonempty(hash: Option<&str>) -> Option<String> {
    hash.filter(|h| !h.is_empty()).map(str::to_uppercase)
}

/// Record an apply/push event and report whether the guard tripped, setting
/// `LoopDetected` on a trip (the apply / consent-push ordering where the trip
/// sticks).
fn record_and_check(
    st: &mut SyncRuntimeState,
    direction: LoopDirection,
    hash: Option<&str>,
    now_ms: i64,
    cfg: &SyncConfig,
) -> CommitOutcome {
    st.loop_events = loop_guard::record(
        std::mem::take(&mut st.loop_events),
        direction,
        hash,
        now_ms,
        cfg.loop_window_secs,
    );
    let tripped = loop_guard::tripped(&st.loop_events, cfg.loop_flip_threshold);
    if tripped {
        st.state = SyncState::LoopDetected;
    }
    CommitOutcome { tripped }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard_doc::ClipboardKind;

    const NOW: i64 = 1_700_000_000_000;

    fn entry(hash: &str) -> Clipboard {
        Clipboard {
            kind: ClipboardKind::Text,
            hash: Some(hash.to_string()),
            text: String::new(),
            has_data: false,
            data_name: None,
            size: None,
        }
    }

    fn hashless_entry(text: &str) -> Clipboard {
        Clipboard {
            kind: ClipboardKind::Text,
            hash: None,
            text: text.to_string(),
            has_data: false,
            data_name: None,
            size: None,
        }
    }

    // --- hashes_equal -----------------------------------------------------

    #[test]
    fn hashes_equal_nil_nil_and_case() {
        assert!(hashes_equal(None, None));
        assert!(!hashes_equal(Some("A"), None));
        assert!(!hashes_equal(None, Some("A")));
        assert!(hashes_equal(Some("aabb"), Some("AABB")));
        assert!(!hashes_equal(Some("AA"), Some("BB")));
    }

    // --- backoff_secs (currentBackoffSeconds) -----------------------------

    #[test]
    fn backoff_doubles_and_caps() {
        let (base, max, j) = (5.0, 60.0, 1.0);
        assert_eq!(backoff_secs(0, base, max, j), 5.0); // max(1,0)=1 → 2^0
        assert_eq!(backoff_secs(1, base, max, j), 5.0); // 2^0 * 5
        assert_eq!(backoff_secs(2, base, max, j), 10.0); // 2^1 * 5
        assert_eq!(backoff_secs(3, base, max, j), 20.0); // 2^2 * 5
        assert_eq!(backoff_secs(4, base, max, j), 40.0); // 2^3 * 5
        assert_eq!(backoff_secs(5, base, max, j), 60.0); // 2^4*5=80 → cap 60
        assert_eq!(backoff_secs(50, base, max, j), 60.0); // exponent clamp + cap
    }

    #[test]
    fn backoff_applies_jitter() {
        assert_eq!(backoff_secs(1, 5.0, 60.0, 0.8), 4.0);
        assert_eq!(backoff_secs(1, 5.0, 60.0, 1.2), 6.0);
    }

    // --- cadence_secs -----------------------------------------------------

    #[test]
    fn cadence_active_inactive_and_paused() {
        let cfg = SyncConfig::default();
        assert_eq!(cadence_secs(SyncState::Succeeded, false, &cfg), 1.0);
        assert_eq!(cadence_secs(SyncState::Succeeded, true, &cfg), 5.0);
        // offlineRetrying keeps the normal cadence (network gated separately).
        assert_eq!(cadence_secs(SyncState::OfflineRetrying, false, &cfg), 1.0);
        assert!(cadence_secs(SyncState::AuthFailed, false, &cfg).is_infinite());
        assert!(cadence_secs(SyncState::LoopDetected, true, &cfg).is_infinite());
    }

    // --- plan_preamble ----------------------------------------------------

    fn preamble_snap() -> PreambleSnapshot {
        PreambleSnapshot {
            explicit: false,
            auto_push: false,
            has_active_server: true,
            device_hash: None,
            history_head_hash: None,
            persisted_synced_hash: None,
            now_ms: NOW,
        }
    }

    #[test]
    fn preamble_no_server_sets_idle_and_stops() {
        let mut st = SyncRuntimeState {
            state: SyncState::Succeeded,
            ..Default::default()
        };
        let snap = PreambleSnapshot {
            has_active_server: false,
            ..preamble_snap()
        };
        let out = plan_preamble(&mut st, &snap);
        assert_eq!(
            out.proceed,
            PreambleProceed::Stop(StopReason::NoActiveServer)
        );
        assert_eq!(st.state, SyncState::Idle);
    }

    #[test]
    fn preamble_paused_states_stop_without_touching_state() {
        for paused in [SyncState::AuthFailed, SyncState::LoopDetected] {
            let mut st = SyncRuntimeState {
                state: paused,
                ..Default::default()
            };
            let out = plan_preamble(&mut st, &preamble_snap());
            assert_eq!(out.proceed, PreambleProceed::Stop(StopReason::Paused));
            assert_eq!(st.state, paused);
        }
    }

    #[test]
    fn preamble_backoff_gate_blocks_routine_but_not_explicit() {
        let mut st = SyncRuntimeState {
            next_attempt_ms: Some(NOW + 5_000),
            ..Default::default()
        };
        // Routine tick inside the window is gated.
        let out = plan_preamble(&mut st, &preamble_snap());
        assert_eq!(out.proceed, PreambleProceed::Stop(StopReason::BackoffGate));
        // Explicit refresh punches through.
        let snap = PreambleSnapshot {
            explicit: true,
            ..preamble_snap()
        };
        let out = plan_preamble(&mut st, &snap);
        assert_eq!(out.proceed, PreambleProceed::ToNetwork);
    }

    #[test]
    fn preamble_backoff_gate_expired_proceeds() {
        let mut st = SyncRuntimeState {
            next_attempt_ms: Some(NOW - 1),
            ..Default::default()
        };
        let out = plan_preamble(&mut st, &preamble_snap());
        assert_eq!(out.proceed, PreambleProceed::ToNetwork);
    }

    #[test]
    fn preamble_cross_process_resync_refreshes_synced_hash() {
        let mut st = SyncRuntimeState {
            last_synced_hash: Some("OLD".into()),
            ..Default::default()
        };
        let snap = PreambleSnapshot {
            persisted_synced_hash: Some("new".into()),
            ..preamble_snap()
        };
        let out = plan_preamble(&mut st, &snap);
        assert_eq!(out.proceed, PreambleProceed::ToNetwork);
        assert_eq!(st.last_synced_hash.as_deref(), Some("NEW"));
    }

    #[test]
    fn preamble_records_local_only_for_fresh_autopush_content() {
        // auto-push off → never record.
        let mut st = SyncRuntimeState::default();
        let snap = PreambleSnapshot {
            auto_push: false,
            device_hash: Some("AABB".into()),
            ..preamble_snap()
        };
        assert!(!plan_preamble(&mut st, &snap).record_local);

        // auto-push on, fresh content → record.
        let snap = PreambleSnapshot {
            auto_push: true,
            device_hash: Some("AABB".into()),
            ..preamble_snap()
        };
        assert!(plan_preamble(&mut st, &snap).record_local);

        // Self-written content (== last_applied) → skip.
        let mut st = SyncRuntimeState {
            last_applied_hash: Some("AABB".into()),
            ..Default::default()
        };
        assert!(!plan_preamble(&mut st, &snap).record_local);

        // Already at the history head → skip.
        let mut st = SyncRuntimeState::default();
        let snap = PreambleSnapshot {
            auto_push: true,
            device_hash: Some("AABB".into()),
            history_head_hash: Some("aabb".into()),
            ..preamble_snap()
        };
        assert!(!plan_preamble(&mut st, &snap).record_local);
    }

    // --- plan_after_server_get: routing -----------------------------------

    fn get_snap(server: Option<Clipboard>, device_hash: Option<&str>) -> ServerGetSnapshot {
        ServerGetSnapshot {
            auto_apply: true,
            auto_push: true,
            server_entry: server,
            device_present: device_hash.is_some(),
            device_hash: device_hash.map(str::to_string),
        }
    }

    #[test]
    fn route_truth_gate_when_server_equals_device() {
        // server-wins: even with a stale watermark, identical content converges.
        let st = SyncRuntimeState {
            last_synced_hash: Some("STALE".into()),
            ..Default::default()
        };
        let snap = get_snap(Some(entry("aabb")), Some("AABB"));
        assert_eq!(
            plan_after_server_get(&st, &snap),
            ServerRoute::Converged {
                server_hash: "AABB".into()
            }
        );
    }

    #[test]
    fn route_server_new_when_hash_differs_from_synced() {
        let st = SyncRuntimeState {
            last_synced_hash: Some("OLD".into()),
            ..Default::default()
        };
        let snap = get_snap(Some(entry("NEW")), Some("DEV"));
        assert_eq!(
            plan_after_server_get(&st, &snap),
            ServerRoute::ServerNew(ServerNewPlan {
                already_staged: false,
                will_apply: true,
            })
        );
    }

    #[test]
    fn route_push_when_server_unchanged() {
        // server hash == last_synced → not server-new; device differs → push.
        let st = SyncRuntimeState {
            last_synced_hash: Some("SAME".into()),
            ..Default::default()
        };
        let snap = get_snap(Some(entry("SAME")), Some("DEV"));
        assert_eq!(
            plan_after_server_get(&st, &snap),
            ServerRoute::Push(PushDecision::DoPush)
        );
    }

    #[test]
    fn route_404_empty_server_falls_through_to_push() {
        let st = SyncRuntimeState::default();
        let snap = get_snap(None, Some("DEV"));
        assert_eq!(
            plan_after_server_get(&st, &snap),
            ServerRoute::Push(PushDecision::DoPush)
        );
    }

    // --- plan_server_new dedup + will_apply -------------------------------

    #[test]
    fn server_new_already_staged_by_hash() {
        let st = SyncRuntimeState {
            last_synced_hash: Some("OLD".into()),
            staged_server_hash: Some("new".into()),
            ..Default::default()
        };
        let snap = get_snap(Some(entry("NEW")), Some("DEV"));
        assert_eq!(
            plan_after_server_get(&st, &snap),
            ServerRoute::ServerNew(ServerNewPlan {
                already_staged: true,
                will_apply: true,
            })
        );
    }

    #[test]
    fn server_new_hashless_dedup_by_full_entry() {
        let staged = hashless_entry("hello");
        // `last_synced_hash` must be non-nil for a hashless entry to count as
        // server-new: `!hashesEqual(nil, nil)` is false (falls through to push),
        // whereas `!hashesEqual(nil, Some)` is true (enters processServerNew),
        // matching Swift `tick` 515-516.
        let st = SyncRuntimeState {
            last_synced_hash: Some("PRIOR".into()),
            staged_entry: Some(staged.clone()),
            ..Default::default()
        };
        // Hashless entry equal to the staged one → already staged, never applies.
        let snap = get_snap(Some(staged), Some("DEV"));
        assert_eq!(
            plan_after_server_get(&st, &snap),
            ServerRoute::ServerNew(ServerNewPlan {
                already_staged: true,
                will_apply: false,
            })
        );
    }

    #[test]
    fn hashless_entry_with_nil_watermark_falls_through_to_push() {
        // Faithful Swift edge: hashless server entry + nil watermark →
        // `!hashesEqual(nil, nil)` is false → not server-new → push side.
        let st = SyncRuntimeState::default();
        let snap = get_snap(Some(hashless_entry("hello")), Some("DEV"));
        assert_eq!(
            plan_after_server_get(&st, &snap),
            ServerRoute::Push(PushDecision::DoPush)
        );
    }

    #[test]
    fn server_new_auto_apply_off_does_not_apply() {
        let st = SyncRuntimeState::default();
        let mut snap = get_snap(Some(entry("NEW")), Some("DEV"));
        snap.auto_apply = false;
        assert_eq!(
            plan_after_server_get(&st, &snap),
            ServerRoute::ServerNew(ServerNewPlan {
                already_staged: false,
                will_apply: false,
            })
        );
    }

    // --- plan_push (maybePush gates) --------------------------------------

    #[test]
    fn push_consent_mode_when_auto_push_off() {
        let st = SyncRuntimeState {
            last_synced_hash: Some("SAME".into()),
            ..Default::default()
        };
        let mut snap = get_snap(Some(entry("SAME")), Some("DEV"));
        snap.auto_push = false;
        assert_eq!(
            plan_after_server_get(&st, &snap),
            ServerRoute::Push(PushDecision::SkipConsentMode)
        );
    }

    #[test]
    fn push_no_device() {
        let st = SyncRuntimeState {
            last_synced_hash: Some("SAME".into()),
            ..Default::default()
        };
        let snap = ServerGetSnapshot {
            auto_apply: true,
            auto_push: true,
            server_entry: Some(entry("SAME")),
            device_present: false,
            device_hash: None,
        };
        assert_eq!(
            plan_after_server_get(&st, &snap),
            ServerRoute::Push(PushDecision::SkipNoDevice)
        );
    }

    #[test]
    fn push_already_synced() {
        let st = SyncRuntimeState {
            last_synced_hash: Some("DEV".into()),
            ..Default::default()
        };
        // server hash == last_synced (so not server-new) AND device == synced.
        let snap = get_snap(Some(entry("DEV")), Some("dev"));
        // Truth-gate fires first (server==device). Use differing server hash.
        let snap2 = ServerGetSnapshot {
            server_entry: Some(entry("DEV")),
            device_hash: Some("dev".into()),
            ..snap
        };
        assert_eq!(
            plan_after_server_get(&st, &snap2),
            ServerRoute::Converged {
                server_hash: "DEV".into()
            }
        );
    }

    #[test]
    fn push_self_written_guard_blocks_reapplied_content() {
        // Dedup guard #2: device holds what we just applied → don't push back.
        // server unchanged (== synced) so not server-new; device == last_applied.
        let st = SyncRuntimeState {
            last_synced_hash: Some("SRV".into()),
            last_applied_hash: Some("DEV".into()),
            ..Default::default()
        };
        let snap = get_snap(Some(entry("SRV")), Some("dev"));
        assert_eq!(
            plan_after_server_get(&st, &snap),
            ServerRoute::Push(PushDecision::SkipSelfWritten)
        );
    }

    // --- commit_converged -------------------------------------------------

    #[test]
    fn commit_converged_repairs_watermark() {
        let mut st = SyncRuntimeState {
            last_synced_hash: Some("STALE".into()),
            staged_server_hash: Some("X".into()),
            staged_entry: Some(entry("X")),
            ..Default::default()
        };
        commit_converged(&mut st, "aabb");
        assert_eq!(st.last_synced_hash.as_deref(), Some("AABB"));
        assert_eq!(st.last_applied_hash.as_deref(), Some("AABB"));
        assert_eq!(st.staged_server_hash, None);
        assert_eq!(st.staged_entry, None);
        assert_eq!(st.state, SyncState::Succeeded);
    }

    // --- commit_apply -----------------------------------------------------

    #[test]
    fn commit_apply_advances_guards_and_succeeds() {
        let cfg = SyncConfig::default();
        let mut st = SyncRuntimeState {
            staged_server_hash: Some("aabb".into()),
            staged_entry: Some(entry("aabb")),
            ..Default::default()
        };
        let out = commit_apply(&mut st, Some("aabb"), NOW, &cfg);
        assert!(!out.tripped);
        assert_eq!(st.last_synced_hash.as_deref(), Some("AABB"));
        assert_eq!(st.last_applied_hash.as_deref(), Some("AABB"));
        assert_eq!(st.staged_server_hash, None);
        assert_eq!(st.staged_entry, None);
        assert_eq!(st.state, SyncState::Succeeded);
        assert_eq!(st.loop_events.len(), 1);
        assert_eq!(st.loop_events[0].direction, LoopDirection::Pulled);
    }

    #[test]
    fn commit_apply_failed_parks_staged() {
        let mut st = SyncRuntimeState::default();
        let e = entry("aabb");
        commit_apply_failed(&mut st, &e);
        assert_eq!(st.staged_server_hash.as_deref(), Some("aabb"));
        assert_eq!(st.staged_entry, Some(e));
        // State untouched (the tick error handler owns it).
        assert_eq!(st.state, SyncState::Idle);
    }

    #[test]
    fn commit_stage_sets_has_new_unwritten() {
        let mut st = SyncRuntimeState::default();
        let e = entry("aabb");
        commit_stage(&mut st, &e);
        assert_eq!(st.staged_server_hash.as_deref(), Some("aabb"));
        assert_eq!(st.staged_entry, Some(e));
        assert_eq!(st.state, SyncState::HasNewUnwritten);
        // Dedup guard #1 stays put — we are "stuck" until apply / new copy.
        assert_eq!(st.last_synced_hash, None);
    }

    // --- commit_push ------------------------------------------------------

    #[test]
    fn commit_push_advances_and_records_pushed() {
        let cfg = SyncConfig::default();
        let mut st = SyncRuntimeState::default();
        let out = commit_push(&mut st, Some("aabb"), NOW, &cfg);
        assert!(!out.tripped);
        assert_eq!(st.last_synced_hash.as_deref(), Some("AABB"));
        // maybePush deliberately does NOT set last_applied.
        assert_eq!(st.last_applied_hash, None);
        assert_eq!(st.state, SyncState::Succeeded);
        assert_eq!(st.loop_events[0].direction, LoopDirection::Pushed);
    }

    #[test]
    fn commit_push_silent_skip_only_succeeds() {
        let cfg = SyncConfig::default();
        let mut st = SyncRuntimeState {
            last_synced_hash: Some("OLD".into()),
            ..Default::default()
        };
        let out = commit_push(&mut st, None, NOW, &cfg);
        assert!(!out.tripped);
        assert_eq!(st.last_synced_hash.as_deref(), Some("OLD")); // unchanged
        assert!(st.loop_events.is_empty());
        assert_eq!(st.state, SyncState::Succeeded);
    }

    // --- loop guard integration (apply/push trip) -------------------------

    #[test]
    fn push_path_trip_shows_loop_detected() {
        let cfg = SyncConfig::default();
        let mut st = SyncRuntimeState::default();
        // Alternate apply/push of the same hash 4× → 3 flips → trip.
        commit_apply(&mut st, Some("H"), NOW, &cfg);
        commit_push(&mut st, Some("H"), NOW + 1, &cfg);
        commit_apply(&mut st, Some("H"), NOW + 2, &cfg);
        let out = commit_push(&mut st, Some("H"), NOW + 3, &cfg);
        assert!(out.tripped);
        // The final commit was a PUSH — the trip now sticks as LoopDetected,
        // matching the apply path (previously an iOS quirk overwrote it back to
        // Succeeded; fixed in both this reducer and SyncEngine.swift).
        assert_eq!(st.state, SyncState::LoopDetected);
    }

    #[test]
    fn apply_final_trip_shows_loop_detected() {
        let cfg = SyncConfig::default();
        let mut st = SyncRuntimeState::default();
        commit_push(&mut st, Some("H"), NOW, &cfg);
        commit_apply(&mut st, Some("H"), NOW + 1, &cfg);
        commit_push(&mut st, Some("H"), NOW + 2, &cfg);
        let out = commit_apply(&mut st, Some("H"), NOW + 3, &cfg);
        assert!(out.tripped);
        // Final commit was an APPLY — the trip sticks.
        assert_eq!(st.state, SyncState::LoopDetected);
    }

    // --- consent push -----------------------------------------------------

    #[test]
    fn commit_consent_push_sets_last_applied_and_resets_failures() {
        let cfg = SyncConfig::default();
        let mut st = SyncRuntimeState {
            consecutive_failures: 3,
            ..Default::default()
        };
        let out = commit_consent_push(&mut st, Some("aabb"), NOW, &cfg);
        assert!(!out.tripped);
        assert_eq!(st.last_synced_hash.as_deref(), Some("AABB"));
        assert_eq!(st.last_applied_hash.as_deref(), Some("AABB"));
        assert_eq!(st.consecutive_failures, 0);
        assert_eq!(st.state, SyncState::Succeeded);
    }

    // --- tick tail / failure handling -------------------------------------

    #[test]
    fn tick_success_clears_backoff() {
        let mut st = SyncRuntimeState {
            consecutive_failures: 4,
            next_attempt_ms: Some(NOW + 1000),
            ..Default::default()
        };
        commit_tick_success(&mut st);
        assert_eq!(st.consecutive_failures, 0);
        assert_eq!(st.next_attempt_ms, None);
    }

    #[test]
    fn tick_failure_auth_pauses() {
        let cfg = SyncConfig::default();
        let mut st = SyncRuntimeState::default();
        let out = commit_tick_failure(&mut st, TickErrorKind::AuthFailed, 1.0, NOW, &cfg);
        assert_eq!(st.state, SyncState::AuthFailed);
        assert!(!out.kick_probe);
        assert_eq!(st.consecutive_failures, 0); // not bumped
    }

    #[test]
    fn tick_failure_cancelled_is_noop() {
        let cfg = SyncConfig::default();
        let mut st = SyncRuntimeState {
            state: SyncState::Succeeded,
            consecutive_failures: 2,
            ..Default::default()
        };
        let out = commit_tick_failure(&mut st, TickErrorKind::Cancelled, 1.0, NOW, &cfg);
        assert_eq!(st.state, SyncState::Succeeded); // unchanged
        assert_eq!(st.consecutive_failures, 2); // unchanged
        assert!(!out.kick_probe);
    }

    #[test]
    fn tick_failure_network_backs_off_and_kicks_probe() {
        let cfg = SyncConfig::default();
        let mut st = SyncRuntimeState::default();
        let out = commit_tick_failure(&mut st, TickErrorKind::NetworkUnreachable, 1.0, NOW, &cfg);
        assert_eq!(st.state, SyncState::OfflineRetrying);
        assert_eq!(st.consecutive_failures, 1);
        assert!(out.first_offline);
        assert!(out.kick_probe);
        // next_attempt = now + backoff(1) = now + 5s.
        assert_eq!(st.next_attempt_ms, Some(NOW + 5_000));
    }

    #[test]
    fn tick_failure_other_sync_error_does_not_kick_probe() {
        let cfg = SyncConfig::default();
        let mut st = SyncRuntimeState::default();
        let out = commit_tick_failure(&mut st, TickErrorKind::OtherSyncError, 1.0, NOW, &cfg);
        assert_eq!(st.state, SyncState::OfflineRetrying);
        assert!(!out.kick_probe);
    }

    #[test]
    fn tick_failure_unexpected_kicks_probe() {
        let cfg = SyncConfig::default();
        let mut st = SyncRuntimeState::default();
        let out = commit_tick_failure(&mut st, TickErrorKind::Unexpected, 1.0, NOW, &cfg);
        assert!(out.kick_probe);
    }

    // --- history sync decisions -------------------------------------------

    #[test]
    fn history_sync_due_first_time_and_after_interval() {
        assert!(is_history_sync_due(None, NOW, 30.0));
        assert!(!is_history_sync_due(Some(NOW), NOW + 29_000, 30.0));
        assert!(is_history_sync_due(Some(NOW), NOW + 30_000, 30.0));
    }

    #[test]
    fn cold_start_when_no_watermark() {
        assert!(is_cold_start(None));
        assert!(!is_cold_start(Some(NOW)));
    }

    #[test]
    fn watermark_advances_only_forward() {
        assert_eq!(advance_watermark(None, NOW), Some(NOW));
        assert_eq!(advance_watermark(Some(NOW), NOW + 1), Some(NOW + 1));
        assert_eq!(advance_watermark(Some(NOW), NOW), None);
        assert_eq!(advance_watermark(Some(NOW), NOW - 1), None);
    }

    // --- epoch validation -------------------------------------------------

    #[test]
    fn probe_conclusion_valid_only_for_same_epoch() {
        assert!(is_probe_conclusion_valid(7, 7));
        assert!(!is_probe_conclusion_valid(7, 8));
    }

    // --- transitions ------------------------------------------------------

    #[test]
    fn mark_staged_applied_advances_and_clears() {
        let mut st = SyncRuntimeState {
            staged_server_hash: Some("aabb".into()),
            staged_entry: Some(entry("aabb")),
            state: SyncState::HasNewUnwritten,
            ..Default::default()
        };
        assert!(mark_staged_applied(&mut st));
        assert_eq!(st.last_synced_hash.as_deref(), Some("AABB"));
        assert_eq!(st.last_applied_hash.as_deref(), Some("AABB"));
        assert_eq!(st.staged_server_hash, None);
        assert_eq!(st.state, SyncState::Succeeded);
    }

    #[test]
    fn mark_staged_applied_noop_when_nothing_staged() {
        let mut st = SyncRuntimeState::default();
        assert!(!mark_staged_applied(&mut st));
        assert_eq!(st.state, SyncState::Idle);
    }

    #[test]
    fn acknowledge_loop_detection_clears_buffer_and_idles() {
        let cfg = SyncConfig::default();
        let mut st = SyncRuntimeState::default();
        commit_apply(&mut st, Some("H"), NOW, &cfg);
        st.state = SyncState::LoopDetected;
        acknowledge_loop_detection(&mut st);
        assert!(st.loop_events.is_empty());
        assert_eq!(st.state, SyncState::Idle);
    }

    #[test]
    fn reset_runtime_state_keeps_synced_hash() {
        let mut st = SyncRuntimeState {
            last_synced_hash: Some("KEEP".into()),
            last_applied_hash: Some("X".into()),
            staged_server_hash: Some("Y".into()),
            staged_entry: Some(entry("Y")),
            consecutive_failures: 3,
            next_attempt_ms: Some(NOW),
            last_history_sync_ms: Some(NOW),
            state: SyncState::OfflineRetrying,
            ..Default::default()
        };
        st.loop_events =
            loop_guard::record(Vec::new(), LoopDirection::Pulled, Some("Z"), NOW, 30.0);
        reset_runtime_state(&mut st);
        assert_eq!(st.last_synced_hash.as_deref(), Some("KEEP")); // kept
        assert_eq!(st.last_applied_hash, None);
        assert_eq!(st.staged_server_hash, None);
        assert_eq!(st.staged_entry, None);
        assert_eq!(st.consecutive_failures, 0);
        assert_eq!(st.next_attempt_ms, None);
        assert_eq!(st.last_history_sync_ms, None);
        assert!(st.loop_events.is_empty());
        assert_eq!(st.state, SyncState::Idle);
    }

    #[test]
    fn handle_active_server_changed_clears_synced_hash_too() {
        let mut st = SyncRuntimeState {
            last_synced_hash: Some("OLD".into()),
            ..Default::default()
        };
        handle_active_server_changed(&mut st);
        assert_eq!(st.last_synced_hash, None);
        assert_eq!(st.state, SyncState::Idle);
    }

    #[test]
    fn handle_network_route_changed_clears_backoff() {
        let mut st = SyncRuntimeState {
            consecutive_failures: 5,
            next_attempt_ms: Some(NOW),
            ..Default::default()
        };
        handle_network_route_changed(&mut st);
        assert_eq!(st.consecutive_failures, 0);
        assert_eq!(st.next_attempt_ms, None);
    }

    // --- end-to-end: server-wins prevents echo ----------------------------

    #[test]
    fn server_wins_then_dedup_short_circuits_push() {
        let cfg = SyncConfig::default();
        let mut st = SyncRuntimeState::default();
        // Tick 1: server has new content; we apply it.
        let snap = get_snap(Some(entry("SRV")), Some("DEV"));
        match plan_after_server_get(&st, &snap) {
            ServerRoute::ServerNew(p) => {
                assert!(p.will_apply && !p.already_staged);
                commit_apply(&mut st, Some("SRV"), NOW, &cfg);
            }
            other => panic!("expected ServerNew, got {other:?}"),
        }
        // Tick 2: device now holds what we applied; server unchanged. The
        // self-written guard must block the push (no echo back to server).
        let snap2 = get_snap(Some(entry("SRV")), Some("srv"));
        // server==device → truth-gate converges (the strongest short-circuit).
        assert_eq!(
            plan_after_server_get(&st, &snap2),
            ServerRoute::Converged {
                server_hash: "SRV".into()
            }
        );
    }
}
