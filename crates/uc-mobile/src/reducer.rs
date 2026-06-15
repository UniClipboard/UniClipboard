//! M5 SyncEngine decision-core reducer — UniFFI boundary mirror of
//! [`uc_mobile_proto::sync_engine`].
//!
//! Same pattern as [`crate::ConnectPayload`] / [`crate::client::ClipboardMeta`]:
//! FFI-native mirror types (`uniffi::Record` / `uniffi::Enum`) plus `From`
//! conversions to/from the proto types, and thin `#[uniffi::export]` wrappers
//! over the proto reducer functions. The proto crate stays uniffi-free (leaf).
//!
//! ## State crosses the FFI by value
//!
//! The proto reducer is `fn(st: &mut SyncRuntimeState, …) -> Output` —
//! caller-holds-plain-struct, mutated in place. UniFFI records are value types
//! with no `&mut`, so every mutating wrapper takes [`SyncRuntimeState`] *by
//! value* and returns the updated state (bundled with any output in a `*Step`
//! record). The Swift `SyncReducerAdapter` rebinds its state from the returned
//! value on each call. State is small and ticks run at ~1 Hz, so the per-call
//! clone across the FFI is negligible.
//!
//! ## `Clipboard` reuse
//!
//! `server_entry` / `staged_entry` reuse [`ClipboardMeta`]. Its `size: u64`
//! collapses a wire-omitted size to `0`, but these entries never reach a
//! persisted blob (runtime / transient input) and the only consumer is the
//! hashless-dedup equality check, where both sides go through the same mapping —
//! so the collapse is behaviorally neutral here (unlike the history blob, where
//! it would drift bytes).

use crate::client::ClipboardMeta;
use uc_mobile_proto::sync_engine as se;
use uc_mobile_proto::{LoopDirection as ProtoLoopDirection, LoopGuardEvent as ProtoLoopGuardEvent};

// ===========================================================================
// Enums
// ===========================================================================

/// FFI mirror of [`se::SyncState`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SyncState {
    Idle,
    Succeeded,
    HasNewUnwritten,
    OfflineRetrying,
    AuthFailed,
    LoopDetected,
}

impl From<se::SyncState> for SyncState {
    fn from(s: se::SyncState) -> Self {
        match s {
            se::SyncState::Idle => Self::Idle,
            se::SyncState::Succeeded => Self::Succeeded,
            se::SyncState::HasNewUnwritten => Self::HasNewUnwritten,
            se::SyncState::OfflineRetrying => Self::OfflineRetrying,
            se::SyncState::AuthFailed => Self::AuthFailed,
            se::SyncState::LoopDetected => Self::LoopDetected,
        }
    }
}

impl From<SyncState> for se::SyncState {
    fn from(s: SyncState) -> Self {
        match s {
            SyncState::Idle => Self::Idle,
            SyncState::Succeeded => Self::Succeeded,
            SyncState::HasNewUnwritten => Self::HasNewUnwritten,
            SyncState::OfflineRetrying => Self::OfflineRetrying,
            SyncState::AuthFailed => Self::AuthFailed,
            SyncState::LoopDetected => Self::LoopDetected,
        }
    }
}

/// FFI mirror of [`ProtoLoopDirection`] (loop-guard subset of history direction).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum LoopDirection {
    Pulled,
    Pushed,
}

impl From<ProtoLoopDirection> for LoopDirection {
    fn from(d: ProtoLoopDirection) -> Self {
        match d {
            ProtoLoopDirection::Pulled => Self::Pulled,
            ProtoLoopDirection::Pushed => Self::Pushed,
        }
    }
}

impl From<LoopDirection> for ProtoLoopDirection {
    fn from(d: LoopDirection) -> Self {
        match d {
            LoopDirection::Pulled => Self::Pulled,
            LoopDirection::Pushed => Self::Pushed,
        }
    }
}

/// Why the preamble stopped the tick before the network ([`se::StopReason`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum StopReason {
    NoActiveServer,
    Paused,
    BackoffGate,
}

impl From<se::StopReason> for StopReason {
    fn from(r: se::StopReason) -> Self {
        match r {
            se::StopReason::NoActiveServer => Self::NoActiveServer,
            se::StopReason::Paused => Self::Paused,
            se::StopReason::BackoffGate => Self::BackoffGate,
        }
    }
}

/// Whether the preamble lets the tick proceed ([`se::PreambleProceed`]).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum PreambleProceed {
    /// Stop here for the given reason.
    Stop { reason: StopReason },
    /// Continue to `getClipboard()`.
    ToNetwork,
}

impl From<se::PreambleProceed> for PreambleProceed {
    fn from(p: se::PreambleProceed) -> Self {
        match p {
            se::PreambleProceed::Stop(reason) => Self::Stop {
                reason: reason.into(),
            },
            se::PreambleProceed::ToNetwork => Self::ToNetwork,
        }
    }
}

/// Push decision ([`se::PushDecision`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PushDecision {
    SkipConsentMode,
    SkipNoDevice,
    SkipAlreadySynced,
    SkipSelfWritten,
    DoPush,
}

impl From<se::PushDecision> for PushDecision {
    fn from(d: se::PushDecision) -> Self {
        match d {
            se::PushDecision::SkipConsentMode => Self::SkipConsentMode,
            se::PushDecision::SkipNoDevice => Self::SkipNoDevice,
            se::PushDecision::SkipAlreadySynced => Self::SkipAlreadySynced,
            se::PushDecision::SkipSelfWritten => Self::SkipSelfWritten,
            se::PushDecision::DoPush => Self::DoPush,
        }
    }
}

/// Routing verdict after the server GET ([`se::ServerRoute`]).
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum ServerRoute {
    /// Truth-gate: server and device hold identical content.
    Converged { server_hash: String },
    /// Server has new content.
    ServerNew { plan: ServerNewPlan },
    /// Server unchanged — fall through to the push side.
    Push { decision: PushDecision },
}

impl From<se::ServerRoute> for ServerRoute {
    fn from(r: se::ServerRoute) -> Self {
        match r {
            se::ServerRoute::Converged { server_hash } => Self::Converged { server_hash },
            se::ServerRoute::ServerNew(plan) => Self::ServerNew { plan: plan.into() },
            se::ServerRoute::Push(decision) => Self::Push {
                decision: decision.into(),
            },
        }
    }
}

/// Error classes the tick handler distinguishes ([`se::TickErrorKind`]). The
/// native shell maps its `SyncError.kind` onto these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum TickErrorKind {
    AuthFailed,
    Cancelled,
    NetworkUnreachable,
    ConnectTimeout,
    ReceiveTimeout,
    OtherSyncError,
    Unexpected,
}

impl From<TickErrorKind> for se::TickErrorKind {
    fn from(k: TickErrorKind) -> Self {
        match k {
            TickErrorKind::AuthFailed => Self::AuthFailed,
            TickErrorKind::Cancelled => Self::Cancelled,
            TickErrorKind::NetworkUnreachable => Self::NetworkUnreachable,
            TickErrorKind::ConnectTimeout => Self::ConnectTimeout,
            TickErrorKind::ReceiveTimeout => Self::ReceiveTimeout,
            TickErrorKind::OtherSyncError => Self::OtherSyncError,
            TickErrorKind::Unexpected => Self::Unexpected,
        }
    }
}

// ===========================================================================
// Records
// ===========================================================================

/// Cadence / backoff / loop-guard tunables ([`se::SyncConfig`]).
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct SyncConfig {
    pub normal_cadence_secs: f64,
    pub inactive_cadence_secs: f64,
    pub offline_backoff_secs: f64,
    pub offline_backoff_max_secs: f64,
    pub history_sync_interval_secs: f64,
    pub loop_window_secs: f64,
    pub loop_flip_threshold: i64,
}

impl From<se::SyncConfig> for SyncConfig {
    fn from(c: se::SyncConfig) -> Self {
        Self {
            normal_cadence_secs: c.normal_cadence_secs,
            inactive_cadence_secs: c.inactive_cadence_secs,
            offline_backoff_secs: c.offline_backoff_secs,
            offline_backoff_max_secs: c.offline_backoff_max_secs,
            history_sync_interval_secs: c.history_sync_interval_secs,
            loop_window_secs: c.loop_window_secs,
            loop_flip_threshold: c.loop_flip_threshold,
        }
    }
}

impl From<SyncConfig> for se::SyncConfig {
    fn from(c: SyncConfig) -> Self {
        Self {
            normal_cadence_secs: c.normal_cadence_secs,
            inactive_cadence_secs: c.inactive_cadence_secs,
            offline_backoff_secs: c.offline_backoff_secs,
            offline_backoff_max_secs: c.offline_backoff_max_secs,
            history_sync_interval_secs: c.history_sync_interval_secs,
            loop_window_secs: c.loop_window_secs,
            loop_flip_threshold: c.loop_flip_threshold,
        }
    }
}

/// One recorded loop-guard event ([`ProtoLoopGuardEvent`]).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LoopGuardEvent {
    pub hash: String,
    pub direction: LoopDirection,
    pub at_millis: i64,
}

impl From<ProtoLoopGuardEvent> for LoopGuardEvent {
    fn from(e: ProtoLoopGuardEvent) -> Self {
        Self {
            hash: e.hash,
            direction: e.direction.into(),
            at_millis: e.at_millis,
        }
    }
}

impl From<LoopGuardEvent> for ProtoLoopGuardEvent {
    fn from(e: LoopGuardEvent) -> Self {
        Self {
            hash: e.hash,
            direction: e.direction.into(),
            at_millis: e.at_millis,
        }
    }
}

/// Per-server runtime decision state ([`se::SyncRuntimeState`]). Owned by the
/// native shell; passed in and returned by value on every mutating call.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SyncRuntimeState {
    pub state: SyncState,
    pub last_synced_hash: Option<String>,
    pub last_applied_hash: Option<String>,
    pub loop_events: Vec<LoopGuardEvent>,
    pub staged_server_hash: Option<String>,
    pub staged_entry: Option<ClipboardMeta>,
    pub consecutive_failures: i64,
    pub next_attempt_ms: Option<i64>,
    pub last_history_sync_ms: Option<i64>,
}

impl From<se::SyncRuntimeState> for SyncRuntimeState {
    fn from(s: se::SyncRuntimeState) -> Self {
        Self {
            state: s.state.into(),
            last_synced_hash: s.last_synced_hash,
            last_applied_hash: s.last_applied_hash,
            loop_events: s.loop_events.into_iter().map(Into::into).collect(),
            staged_server_hash: s.staged_server_hash,
            staged_entry: s.staged_entry.map(ClipboardMeta::from_proto),
            consecutive_failures: s.consecutive_failures,
            next_attempt_ms: s.next_attempt_ms,
            last_history_sync_ms: s.last_history_sync_ms,
        }
    }
}

impl From<SyncRuntimeState> for se::SyncRuntimeState {
    fn from(s: SyncRuntimeState) -> Self {
        Self {
            state: s.state.into(),
            last_synced_hash: s.last_synced_hash,
            last_applied_hash: s.last_applied_hash,
            loop_events: s.loop_events.into_iter().map(Into::into).collect(),
            staged_server_hash: s.staged_server_hash,
            staged_entry: s.staged_entry.map(ClipboardMeta::into_proto),
            consecutive_failures: s.consecutive_failures,
            next_attempt_ms: s.next_attempt_ms,
            last_history_sync_ms: s.last_history_sync_ms,
        }
    }
}

/// What the shell observed locally before the network round-trip
/// ([`se::PreambleSnapshot`], FFI input).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct PreambleSnapshot {
    pub explicit: bool,
    pub auto_push: bool,
    pub has_active_server: bool,
    pub device_hash: Option<String>,
    pub history_head_hash: Option<String>,
    pub persisted_synced_hash: Option<String>,
    pub now_ms: i64,
}

impl From<PreambleSnapshot> for se::PreambleSnapshot {
    fn from(s: PreambleSnapshot) -> Self {
        Self {
            explicit: s.explicit,
            auto_push: s.auto_push,
            has_active_server: s.has_active_server,
            device_hash: s.device_hash,
            history_head_hash: s.history_head_hash,
            persisted_synced_hash: s.persisted_synced_hash,
            now_ms: s.now_ms,
        }
    }
}

/// Plan emitted by [`plan_preamble`] ([`se::Preamble`]).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Preamble {
    pub record_local: bool,
    pub proceed: PreambleProceed,
}

impl From<se::Preamble> for Preamble {
    fn from(p: se::Preamble) -> Self {
        Self {
            record_local: p.record_local,
            proceed: p.proceed.into(),
        }
    }
}

/// What the shell holds after `getClipboard()` ([`se::ServerGetSnapshot`], FFI
/// input).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ServerGetSnapshot {
    pub auto_apply: bool,
    pub auto_push: bool,
    pub server_entry: Option<ClipboardMeta>,
    pub device_present: bool,
    pub device_hash: Option<String>,
}

impl From<ServerGetSnapshot> for se::ServerGetSnapshot {
    fn from(s: ServerGetSnapshot) -> Self {
        Self {
            auto_apply: s.auto_apply,
            auto_push: s.auto_push,
            server_entry: s.server_entry.map(ClipboardMeta::into_proto),
            device_present: s.device_present,
            device_hash: s.device_hash,
        }
    }
}

/// Server-has-new sub-plan ([`se::ServerNewPlan`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct ServerNewPlan {
    pub already_staged: bool,
    pub will_apply: bool,
}

impl From<se::ServerNewPlan> for ServerNewPlan {
    fn from(p: se::ServerNewPlan) -> Self {
        Self {
            already_staged: p.already_staged,
            will_apply: p.will_apply,
        }
    }
}

/// Outcome of a commit that records a loop-guard event ([`se::CommitOutcome`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct CommitOutcome {
    /// The loop guard tripped — the shell stops the loop; state is
    /// [`SyncState::LoopDetected`].
    pub tripped: bool,
}

impl From<se::CommitOutcome> for CommitOutcome {
    fn from(o: se::CommitOutcome) -> Self {
        Self { tripped: o.tripped }
    }
}

/// What the shell should do after a failed tick ([`se::TickFailureOutcome`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct TickFailureOutcome {
    pub kick_probe: bool,
    pub first_offline: bool,
}

impl From<se::TickFailureOutcome> for TickFailureOutcome {
    fn from(o: se::TickFailureOutcome) -> Self {
        Self {
            kick_probe: o.kick_probe,
            first_offline: o.first_offline,
        }
    }
}

// ===========================================================================
// Step records — updated state bundled with a mutating call's output
// ===========================================================================

/// Result of [`plan_preamble`]: the (possibly resync-mutated) state + the plan.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct PreambleStep {
    pub state: SyncRuntimeState,
    pub preamble: Preamble,
}

/// Result of an apply / push / consent-push commit: updated state + outcome.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct CommitStep {
    pub state: SyncRuntimeState,
    pub outcome: CommitOutcome,
}

/// Result of [`commit_tick_failure`]: updated state + what to do next.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TickFailureStep {
    pub state: SyncRuntimeState,
    pub outcome: TickFailureOutcome,
}

/// Result of [`mark_staged_applied`]: updated state + whether anything was
/// staged (Swift returns `false` for a no-op).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MarkStagedStep {
    pub state: SyncRuntimeState,
    pub was_staged: bool,
}

// ===========================================================================
// Defaults
// ===========================================================================

/// Default tunables ([`se::SyncConfig::default`]) — single source of truth for
/// the Swift literals.
#[uniffi::export]
pub fn default_sync_config() -> SyncConfig {
    se::SyncConfig::default().into()
}

/// A freshly-reset runtime state ([`se::SyncRuntimeState::default`]).
#[uniffi::export]
pub fn default_sync_runtime_state() -> SyncRuntimeState {
    se::SyncRuntimeState::default().into()
}

// ===========================================================================
// Plan (decision) wrappers
// ===========================================================================

/// Tick preamble — local-history decision, early-exit guards, and (on the
/// proceed path) the cross-process synced-hash resync. Mutates `state`.
#[uniffi::export]
pub fn plan_preamble(state: SyncRuntimeState, snap: PreambleSnapshot) -> PreambleStep {
    let mut st: se::SyncRuntimeState = state.into();
    let pre = se::plan_preamble(&mut st, &snap.into());
    PreambleStep {
        state: st.into(),
        preamble: pre.into(),
    }
}

/// Post-`getClipboard` route. Read-only over `state`.
#[uniffi::export]
pub fn plan_after_server_get(state: SyncRuntimeState, snap: ServerGetSnapshot) -> ServerRoute {
    let st: se::SyncRuntimeState = state.into();
    se::plan_after_server_get(&st, &snap.into()).into()
}

// ===========================================================================
// Commit wrappers
// ===========================================================================

/// Truth-gate commit: repair watermark, mark applied, clear staged, succeed.
#[uniffi::export]
pub fn commit_converged(state: SyncRuntimeState, server_hash: String) -> SyncRuntimeState {
    let mut st: se::SyncRuntimeState = state.into();
    se::commit_converged(&mut st, &server_hash);
    st.into()
}

/// Apply commit: a server entry was written to the pasteboard.
#[uniffi::export]
pub fn commit_apply(
    state: SyncRuntimeState,
    hash: Option<String>,
    now_ms: i64,
    cfg: SyncConfig,
) -> CommitStep {
    let mut st: se::SyncRuntimeState = state.into();
    let outcome = se::commit_apply(&mut st, hash.as_deref(), now_ms, &cfg.into());
    CommitStep {
        state: st.into(),
        outcome: outcome.into(),
    }
}

/// Apply-failed commit: park the entry so the next tick short-circuits.
#[uniffi::export]
pub fn commit_apply_failed(state: SyncRuntimeState, entry: ClipboardMeta) -> SyncRuntimeState {
    let mut st: se::SyncRuntimeState = state.into();
    se::commit_apply_failed(&mut st, &entry.into_proto());
    st.into()
}

/// Stage-only commit: auto-apply off (or hashless) — stash + `HasNewUnwritten`.
#[uniffi::export]
pub fn commit_stage(state: SyncRuntimeState, entry: ClipboardMeta) -> SyncRuntimeState {
    let mut st: se::SyncRuntimeState = state.into();
    se::commit_stage(&mut st, &entry.into_proto());
    st.into()
}

/// Push commit. `pushed_hash` is `None` for a documented silent skip.
#[uniffi::export]
pub fn commit_push(
    state: SyncRuntimeState,
    pushed_hash: Option<String>,
    now_ms: i64,
    cfg: SyncConfig,
) -> CommitStep {
    let mut st: se::SyncRuntimeState = state.into();
    let outcome = se::commit_push(&mut st, pushed_hash.as_deref(), now_ms, &cfg.into());
    CommitStep {
        state: st.into(),
        outcome: outcome.into(),
    }
}

/// Push-skip commit: any skip branch leaves the tick healthy.
#[uniffi::export]
pub fn commit_push_skipped(state: SyncRuntimeState) -> SyncRuntimeState {
    let mut st: se::SyncRuntimeState = state.into();
    se::commit_push_skipped(&mut st);
    st.into()
}

/// Consent-push commit: user handed us bytes via the paste control.
#[uniffi::export]
pub fn commit_consent_push(
    state: SyncRuntimeState,
    pushed_hash: Option<String>,
    now_ms: i64,
    cfg: SyncConfig,
) -> CommitStep {
    let mut st: se::SyncRuntimeState = state.into();
    let outcome = se::commit_consent_push(&mut st, pushed_hash.as_deref(), now_ms, &cfg.into());
    CommitStep {
        state: st.into(),
        outcome: outcome.into(),
    }
}

/// Happy-path tick tail: drop the backoff counter.
#[uniffi::export]
pub fn commit_tick_success(state: SyncRuntimeState) -> SyncRuntimeState {
    let mut st: se::SyncRuntimeState = state.into();
    se::commit_tick_success(&mut st);
    st.into()
}

/// Tick error handler. `jitter` is native-supplied (`0.8…1.2`) for determinism.
#[uniffi::export]
pub fn commit_tick_failure(
    state: SyncRuntimeState,
    kind: TickErrorKind,
    jitter: f64,
    now_ms: i64,
    cfg: SyncConfig,
) -> TickFailureStep {
    let mut st: se::SyncRuntimeState = state.into();
    let outcome = se::commit_tick_failure(&mut st, kind.into(), jitter, now_ms, &cfg.into());
    TickFailureStep {
        state: st.into(),
        outcome: outcome.into(),
    }
}

/// Record that a history-sync round ran.
#[uniffi::export]
pub fn commit_history_sync_done(state: SyncRuntimeState, now_ms: i64) -> SyncRuntimeState {
    let mut st: se::SyncRuntimeState = state.into();
    se::commit_history_sync_done(&mut st, now_ms);
    st.into()
}

// ===========================================================================
// State-transition wrappers
// ===========================================================================

/// User manually applied the staged entry. `was_staged` is `false` (no-op) when
/// nothing was staged.
#[uniffi::export]
pub fn mark_staged_applied(state: SyncRuntimeState) -> MarkStagedStep {
    let mut st: se::SyncRuntimeState = state.into();
    let was_staged = se::mark_staged_applied(&mut st);
    MarkStagedStep {
        state: st.into(),
        was_staged,
    }
}

/// User dismissed the loop-detected banner: wipe the cycle buffer, back to idle.
#[uniffi::export]
pub fn acknowledge_loop_detection(state: SyncRuntimeState) -> SyncRuntimeState {
    let mut st: se::SyncRuntimeState = state.into();
    se::acknowledge_loop_detection(&mut st);
    st.into()
}

/// Clear per-server runtime state without touching the persisted synced hash.
#[uniffi::export]
pub fn reset_runtime_state(state: SyncRuntimeState) -> SyncRuntimeState {
    let mut st: se::SyncRuntimeState = state.into();
    se::reset_runtime_state(&mut st);
    st.into()
}

/// Active server changed: full reset plus clearing the synced hash.
#[uniffi::export]
pub fn handle_active_server_changed(state: SyncRuntimeState) -> SyncRuntimeState {
    let mut st: se::SyncRuntimeState = state.into();
    se::handle_active_server_changed(&mut st);
    st.into()
}

/// Network route / endpoint changed: clear the backoff accumulated against the
/// dead route.
#[uniffi::export]
pub fn handle_network_route_changed(state: SyncRuntimeState) -> SyncRuntimeState {
    let mut st: se::SyncRuntimeState = state.into();
    se::handle_network_route_changed(&mut st);
    st.into()
}

// ===========================================================================
// Pure-helper wrappers (no state)
// ===========================================================================

/// Case-insensitive hash comparison treating `None == None`.
#[uniffi::export]
pub fn hashes_equal(a: Option<String>, b: Option<String>) -> bool {
    se::hashes_equal(a.as_deref(), b.as_deref())
}

/// Exponential backoff with caller-supplied jitter.
#[uniffi::export]
pub fn backoff_secs(consecutive_failures: i64, base: f64, max: f64, jitter: f64) -> f64 {
    se::backoff_secs(consecutive_failures, base, max, jitter)
}

/// Cadence for the current state (`AuthFailed` / `LoopDetected` → infinity).
#[uniffi::export]
pub fn cadence_secs(state: SyncState, is_scene_inactive: bool, cfg: SyncConfig) -> f64 {
    se::cadence_secs(state.into(), is_scene_inactive, &cfg.into())
}

/// Whether a history-sync round is due.
#[uniffi::export]
pub fn is_history_sync_due(last_sync_ms: Option<i64>, now_ms: i64, interval_secs: f64) -> bool {
    se::is_history_sync_due(last_sync_ms, now_ms, interval_secs)
}

/// Cold-start = no history watermark yet (fetch only page 1, seed the watermark).
#[uniffi::export]
pub fn is_cold_start(watermark_ms: Option<i64>) -> bool {
    se::is_cold_start(watermark_ms)
}

/// New watermark after a history round; `None` when it didn't move.
#[uniffi::export]
pub fn advance_watermark(current_ms: Option<i64>, max_last_modified_ms: i64) -> Option<i64> {
    se::advance_watermark(current_ms, max_last_modified_ms)
}

/// Whether a probe conclusion is still valid (its network epoch is current).
#[uniffi::export]
pub fn is_probe_conclusion_valid(report_epoch: u64, current_epoch: u64) -> bool {
    se::is_probe_conclusion_valid(report_epoch, current_epoch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::ClipboardKind;

    fn meta(hash: &str) -> ClipboardMeta {
        ClipboardMeta {
            kind: ClipboardKind::Text,
            text: "hi".into(),
            data_name: None,
            has_data: false,
            size: 2,
            hash: Some(hash.into()),
        }
    }

    // ---- enum mapping ----

    #[test]
    fn sync_state_round_trips() {
        for s in [
            SyncState::Idle,
            SyncState::Succeeded,
            SyncState::HasNewUnwritten,
            SyncState::OfflineRetrying,
            SyncState::AuthFailed,
            SyncState::LoopDetected,
        ] {
            let proto: se::SyncState = s.into();
            assert_eq!(SyncState::from(proto), s);
        }
    }

    #[test]
    fn loop_direction_round_trips() {
        for d in [LoopDirection::Pulled, LoopDirection::Pushed] {
            let proto: ProtoLoopDirection = d.into();
            assert_eq!(LoopDirection::from(proto), d);
        }
    }

    #[test]
    fn tick_error_kind_maps_to_proto() {
        assert_eq!(
            se::TickErrorKind::from(TickErrorKind::NetworkUnreachable),
            se::TickErrorKind::NetworkUnreachable
        );
        assert_eq!(
            se::TickErrorKind::from(TickErrorKind::Cancelled),
            se::TickErrorKind::Cancelled
        );
    }

    // ---- record round-trips ----

    #[test]
    fn runtime_state_round_trips_with_events_and_staged_entry() {
        let st = SyncRuntimeState {
            state: SyncState::HasNewUnwritten,
            last_synced_hash: Some("AA".into()),
            last_applied_hash: None,
            loop_events: vec![LoopGuardEvent {
                hash: "BB".into(),
                direction: LoopDirection::Pushed,
                at_millis: 123,
            }],
            staged_server_hash: Some("CC".into()),
            staged_entry: Some(meta("CC")),
            consecutive_failures: 2,
            next_attempt_ms: Some(9_000),
            last_history_sync_ms: Some(8_000),
        };
        let proto: se::SyncRuntimeState = st.clone().into();
        assert_eq!(SyncRuntimeState::from(proto), st);
    }

    #[test]
    fn default_config_matches_proto_literals() {
        let c = default_sync_config();
        assert_eq!(c.normal_cadence_secs, 1.0);
        assert_eq!(c.inactive_cadence_secs, 5.0);
        assert_eq!(c.offline_backoff_secs, 5.0);
        assert_eq!(c.offline_backoff_max_secs, 60.0);
        assert_eq!(c.history_sync_interval_secs, 30.0);
        assert_eq!(c.loop_flip_threshold, 3);
    }

    #[test]
    fn default_runtime_state_is_idle_and_empty() {
        let st = default_sync_runtime_state();
        assert_eq!(st.state, SyncState::Idle);
        assert!(st.last_synced_hash.is_none());
        assert!(st.loop_events.is_empty());
        assert_eq!(st.consecutive_failures, 0);
    }

    // ---- plan wrappers ----

    #[test]
    fn plan_after_server_get_converged() {
        let st = default_sync_runtime_state();
        let snap = ServerGetSnapshot {
            auto_apply: true,
            auto_push: true,
            server_entry: Some(meta("aa")),
            device_present: true,
            device_hash: Some("AA".into()),
        };
        match plan_after_server_get(st, snap) {
            ServerRoute::Converged { server_hash } => assert_eq!(server_hash, "AA"),
            other => panic!("expected Converged, got {other:?}"),
        }
    }

    #[test]
    fn plan_after_server_get_server_new() {
        let st = default_sync_runtime_state(); // last_synced_hash = None
        let snap = ServerGetSnapshot {
            auto_apply: true,
            auto_push: true,
            server_entry: Some(meta("BB")),
            device_present: true,
            device_hash: Some("CC".into()),
        };
        match plan_after_server_get(st, snap) {
            ServerRoute::ServerNew { plan } => {
                assert!(!plan.already_staged);
                assert!(plan.will_apply); // auto_apply && has hash
            }
            other => panic!("expected ServerNew, got {other:?}"),
        }
    }

    #[test]
    fn plan_after_server_get_push_when_server_unchanged() {
        let mut st = default_sync_runtime_state();
        st.last_synced_hash = Some("BB".into());
        let snap = ServerGetSnapshot {
            auto_apply: true,
            auto_push: false, // consent mode
            server_entry: Some(meta("BB")),
            device_present: true,
            device_hash: Some("DD".into()),
        };
        match plan_after_server_get(st, snap) {
            ServerRoute::Push { decision } => assert_eq!(decision, PushDecision::SkipConsentMode),
            other => panic!("expected Push, got {other:?}"),
        }
    }

    #[test]
    fn plan_preamble_records_local_and_proceeds() {
        let st = default_sync_runtime_state();
        let snap = PreambleSnapshot {
            explicit: false,
            auto_push: true,
            has_active_server: true,
            device_hash: Some("NEW".into()),
            history_head_hash: None,
            persisted_synced_hash: None,
            now_ms: 1_000,
        };
        let step = plan_preamble(st, snap);
        assert!(step.preamble.record_local);
        assert_eq!(step.preamble.proceed, PreambleProceed::ToNetwork);
    }

    #[test]
    fn plan_preamble_stops_without_active_server() {
        let st = default_sync_runtime_state();
        let snap = PreambleSnapshot {
            explicit: false,
            auto_push: false,
            has_active_server: false,
            device_hash: None,
            history_head_hash: None,
            persisted_synced_hash: None,
            now_ms: 0,
        };
        let step = plan_preamble(st, snap);
        assert_eq!(
            step.preamble.proceed,
            PreambleProceed::Stop {
                reason: StopReason::NoActiveServer
            }
        );
        assert_eq!(step.state.state, SyncState::Idle);
    }

    // ---- commit wrappers ----

    #[test]
    fn commit_converged_advances_and_succeeds() {
        let st = default_sync_runtime_state();
        let out = commit_converged(st, "bb".into());
        assert_eq!(out.last_synced_hash.as_deref(), Some("BB"));
        assert_eq!(out.last_applied_hash.as_deref(), Some("BB"));
        assert!(out.staged_server_hash.is_none());
        assert_eq!(out.state, SyncState::Succeeded);
    }

    #[test]
    fn commit_apply_single_event_does_not_trip() {
        let st = default_sync_runtime_state();
        let step = commit_apply(st, Some("dd".into()), 1_000, default_sync_config());
        assert!(!step.outcome.tripped);
        assert_eq!(step.state.last_synced_hash.as_deref(), Some("DD"));
        assert_eq!(step.state.state, SyncState::Succeeded);
        assert_eq!(step.state.loop_events.len(), 1);
    }

    #[test]
    fn commit_tick_failure_backs_off_and_kicks_probe() {
        let st = default_sync_runtime_state();
        let step = commit_tick_failure(
            st,
            TickErrorKind::NetworkUnreachable,
            1.0,
            0,
            default_sync_config(),
        );
        assert_eq!(step.state.consecutive_failures, 1);
        assert_eq!(step.state.next_attempt_ms, Some(5_000)); // base 5s * jitter 1.0
        assert_eq!(step.state.state, SyncState::OfflineRetrying);
        assert!(step.outcome.kick_probe);
        assert!(step.outcome.first_offline);
    }

    #[test]
    fn commit_tick_failure_cancelled_is_no_op() {
        let st = default_sync_runtime_state();
        let step = commit_tick_failure(st, TickErrorKind::Cancelled, 1.0, 0, default_sync_config());
        assert_eq!(step.state.consecutive_failures, 0);
        assert!(step.state.next_attempt_ms.is_none());
        assert!(!step.outcome.kick_probe);
    }

    // ---- transition wrappers ----

    #[test]
    fn mark_staged_applied_reports_staged() {
        let mut st = default_sync_runtime_state();
        st.staged_server_hash = Some("ee".into());
        st.staged_entry = Some(meta("ee"));
        let step = mark_staged_applied(st);
        assert!(step.was_staged);
        assert_eq!(step.state.last_synced_hash.as_deref(), Some("EE"));
        assert!(step.state.staged_server_hash.is_none());
        assert_eq!(step.state.state, SyncState::Succeeded);
    }

    #[test]
    fn mark_staged_applied_no_op_when_nothing_staged() {
        let st = default_sync_runtime_state();
        let step = mark_staged_applied(st);
        assert!(!step.was_staged);
    }

    #[test]
    fn handle_active_server_changed_clears_synced_hash() {
        let mut st = default_sync_runtime_state();
        st.last_synced_hash = Some("AA".into());
        st.last_applied_hash = Some("AA".into());
        let out = handle_active_server_changed(st);
        assert!(out.last_synced_hash.is_none());
        assert!(out.last_applied_hash.is_none());
        assert_eq!(out.state, SyncState::Idle);
    }

    #[test]
    fn handle_network_route_changed_clears_backoff() {
        let mut st = default_sync_runtime_state();
        st.consecutive_failures = 4;
        st.next_attempt_ms = Some(50_000);
        let out = handle_network_route_changed(st);
        assert_eq!(out.consecutive_failures, 0);
        assert!(out.next_attempt_ms.is_none());
    }

    #[test]
    fn acknowledge_loop_detection_clears_events_and_idles() {
        let mut st = default_sync_runtime_state();
        st.state = SyncState::LoopDetected;
        st.loop_events = vec![LoopGuardEvent {
            hash: "AA".into(),
            direction: LoopDirection::Pulled,
            at_millis: 1,
        }];
        let out = acknowledge_loop_detection(st);
        assert!(out.loop_events.is_empty());
        assert_eq!(out.state, SyncState::Idle);
    }

    // ---- pure helpers ----

    #[test]
    fn pure_helpers_delegate() {
        assert!(hashes_equal(Some("aa".into()), Some("AA".into())));
        assert!(!hashes_equal(Some("aa".into()), None));
        assert_eq!(backoff_secs(1, 5.0, 60.0, 1.0), 5.0);
        assert_eq!(backoff_secs(2, 5.0, 60.0, 1.0), 10.0);
        assert_eq!(
            cadence_secs(SyncState::AuthFailed, false, default_sync_config()),
            f64::INFINITY
        );
        assert_eq!(
            cadence_secs(SyncState::Idle, true, default_sync_config()),
            5.0
        );
        assert!(is_history_sync_due(None, 0, 30.0));
        assert!(is_history_sync_due(Some(0), 30_000, 30.0));
        assert!(!is_history_sync_due(Some(0), 1_000, 30.0));
        assert!(is_cold_start(None));
        assert!(!is_cold_start(Some(1)));
        assert_eq!(advance_watermark(None, 100), Some(100));
        assert_eq!(advance_watermark(Some(100), 50), None);
        assert_eq!(advance_watermark(Some(50), 100), Some(100));
        assert!(is_probe_conclusion_valid(5, 5));
        assert!(!is_probe_conclusion_valid(5, 6));
    }
}
