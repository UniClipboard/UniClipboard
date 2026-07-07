//! `MobileSyncEngine` — the push/pull sync SDK (design
//! `.planning/2026-07-05-mobile-push-pull-sdk-design.md`).
//!
//! Collapses the client-facing surface to two primitives — [`MobileSyncEngine::push`] /
//! [`MobileSyncEngine::pull`] (+ [`MobileSyncEngine::apply_staged`] and a few lifecycle
//! methods) — with dedup, anti-loop, watermark persistence, backoff, and conflict
//! resolution fully internal. Everything routes through one internal sync-tick built
//! from the existing pure reducer ([`uc_mobile_proto::sync_engine`]): `get_latest` →
//! [`se::plan_after_server_get`] → commit. `push` and `pull` share this tick (design
//! Q10): `push` runs it FIRST so a server-side change wins over a stale local upload
//! (stale-clobber avoidance) before ever considering `put_clipboard`.
//!
//! ## Persistence
//! Only the watermark (`last_synced_hash` / `last_synced_content_id`) is durable, via
//! the injected [`KeyValueStore`] (App Group keys from
//! [`uc_mobile_proto::persist_keys::files`] — cross-process-fresh, shared with the
//! Share Extension's direct writes). Everything else on [`se::SyncRuntimeState`]
//! (`last_applied_hash`, staged slot, loop events, backoff) is session-only, matching
//! the reducer's existing semantics.
//!
//! ## Concurrency
//! One [`tokio::sync::Mutex`] serializes `push` / `pull` / `apply_staged` end to end
//! (load-before-op through save-after-op), per design §6.5 — a concurrent push
//! (local-write trigger) and pull (SSE trigger) must not interleave on the same
//! runtime state.

use std::sync::Arc;

use uc_mobile_proto::sync_engine as se;
use uc_mobile_proto::{
    persist_keys, publish_file, publish_image, publish_text, Clipboard as ProtoClipboard,
};

use crate::client::{ClipboardKind, ClipboardMeta, MobileSyncClient, ServerConfig, SyncError};
use crate::reducer::SyncConfig;

// ─── KeyValueStore port ─────────────────────────────────────────────────

/// App Group key-value store, implemented natively and injected. Keys are the
/// pinned constants in [`uc_mobile_proto::persist_keys`] — this port reads/writes
/// the SAME keys the Share Extension writes directly (cross-process contract), not
/// an opaque blob.
///
/// `with_foreign` (not `callback_interface`) is required for a foreign
/// implementation to be usable as an `Arc<dyn …>` constructor argument
/// (uniffi-rs#2797) — same pattern already used by [`crate::client::PlatformBridge`]
/// and [`crate::client::SseListener`].
#[uniffi::export(with_foreign)]
pub trait KeyValueStore: Send + Sync {
    fn get(&self, key: String) -> Option<Vec<u8>>;
    fn set(&self, key: String, value: Vec<u8>);
    fn remove(&self, key: String);
}

// ─── FFI-facing domain types ────────────────────────────────────────────

/// Local pasteboard content, read by native and handed to [`MobileSyncEngine::push`].
/// `payload` carries the bytes for `Image`/`File`; `Text` is inline in `text` (the
/// engine handles the long-text-overflow-to-file transform internally, mirroring
/// [`ProtoClipboard::publish_text`]).
#[derive(Debug, Clone, uniffi::Record)]
pub struct LocalContent {
    pub kind: ClipboardKind,
    /// Text content for `Text`; unused (empty) for `Image`/`File`.
    pub text: String,
    /// Filename hint for `Image`/`File` (extension drives the upload name for
    /// `Image`); unused for `Text`.
    pub data_name: Option<String>,
    /// Payload bytes for `Image`/`File`; `None` for `Text`.
    pub payload: Option<Vec<u8>>,
}

/// Engine-owned settings. `auto_push` is deliberately absent (design Q2) — "when to
/// call `push`" is entirely the client's call; the engine only decides what happens
/// with server-new content once `pull`/`push` runs.
#[derive(Debug, Clone, Copy, uniffi::Record)]
pub struct SyncSettings {
    pub auto_apply: bool,
}

/// What triggered a [`MobileSyncEngine::pull`] call — decides the backoff gate and
/// the SSE `contentId` short-circuit (design Q3/Q6).
#[derive(Debug, Clone, uniffi::Enum)]
pub enum PullTrigger {
    /// Fallback-tick cadence — gated by the sync-op backoff window.
    Routine,
    /// User pull-to-refresh — punches through backoff.
    Explicit,
    /// SSE connection just became live — punches through (may have missed updates).
    SseHello,
    /// Server told us we lagged — punches through.
    SseResync,
    /// SSE push notification carrying a `contentId`. Short-circuits to
    /// `UpToDate{SseShortCircuit}` with no network call when it already matches the
    /// synced watermark; otherwise punches through like `Explicit`.
    SseUpdate { content_id: String },
}

/// Why a `push`/`pull`/`apply_staged` call flowed nothing (design §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum UpToDateReason {
    /// Device content already matches the synced watermark.
    AlreadySynced,
    /// Device content is what the engine itself last applied (anti-loop guard #1).
    SelfWritten,
    /// Truth-gate: server and device already hold identical content.
    Converged,
    /// Nothing to push (no local content presented, or auto_push semantics n/a).
    NoLocalChange,
    /// `SseUpdate`'s `contentId` already matched the synced watermark.
    SseShortCircuit,
    /// Consent-push mode — reserved for a future non-auto-push variant (Q2); the
    /// engine's `push`/`pull` never produce this today (auto_push is client-owned).
    ConsentMode,
}

/// Metadata about content that just flowed, for native to append a history row
/// (design Q4 — the engine holds no history keys itself).
#[derive(Debug, Clone, uniffi::Record)]
pub struct SyncedMeta {
    pub kind: ClipboardKind,
    pub hash: Option<String>,
    pub content_id: Option<String>,
    pub text: Option<String>,
    pub size: Option<i64>,
}

/// Enough to render a "new content available" banner without downloading bytes.
#[derive(Debug, Clone, uniffi::Record)]
pub struct StagedPreview {
    pub kind: ClipboardKind,
    pub text: String,
    pub size: Option<i64>,
}

/// Unified result of `push` / `pull` / `apply_staged` (design §4).
#[derive(Debug, Clone, uniffi::Enum)]
pub enum SyncOutcome {
    /// A full `put_clipboard` succeeded — the active register advanced.
    Uploaded {
        meta: SyncedMeta,
    },
    /// Server content was downloaded; native should write it to the
    /// pasteboard/Files. `last_applied_hash` is already set (optimistic, Q5).
    Applied {
        content: LocalContent,
        meta: SyncedMeta,
    },
    /// Server has new content but `auto_apply` is off — staged for later
    /// [`MobileSyncEngine::apply_staged`].
    Staged {
        preview: StagedPreview,
    },
    /// Nothing flowed.
    UpToDate {
        reason: UpToDateReason,
    },
    /// A routine call was gated by the sync-op backoff window.
    BackingOff {
        retry_after_ms: i64,
    },
    /// The anti-loop guard tripped; paused until
    /// [`MobileSyncEngine::acknowledge_loop_detected`].
    LoopDetected,
    Failed {
        error: SyncError,
    },
}

// ─── engine ──────────────────────────────────────────────────────────────

/// Everything the pure reducer needs, held privately — never crosses the FFI
/// boundary (unlike `reducer.rs`'s per-function mirror, which is the pattern this
/// engine replaces).
struct EngineState {
    server: ServerConfig,
    settings: SyncSettings,
    cfg: se::SyncConfig,
    runtime: se::SyncRuntimeState,
}

/// Long-lived sync engine (design §4) — one instance per active server for the
/// process lifetime (Q8: `set_server` reconfigures in place rather than requiring
/// reconstruction).
#[derive(uniffi::Object)]
pub struct MobileSyncEngine {
    client: Arc<MobileSyncClient>,
    store: Arc<dyn KeyValueStore>,
    state: tokio::sync::Mutex<EngineState>,
}

#[uniffi::export]
impl MobileSyncEngine {
    #[uniffi::constructor]
    pub fn new(
        server: ServerConfig,
        config: SyncConfig,
        settings: SyncSettings,
        store: Arc<dyn KeyValueStore>,
        client: Arc<MobileSyncClient>,
    ) -> Result<Arc<Self>, SyncError> {
        Ok(Arc::new(Self {
            client,
            store,
            state: tokio::sync::Mutex::new(EngineState {
                server,
                settings,
                cfg: config.into(),
                runtime: se::SyncRuntimeState::default(),
            }),
        }))
    }

    /// Native observed a local pasteboard change and read its bytes — call this to
    /// sync it. Internally runs `get_latest` FIRST (Q10): if the server changed
    /// since the last sync, that wins (`Applied`) and the local upload is skipped
    /// this round rather than clobbering newer server content. Only when the
    /// server is unchanged does it fall through to the watermark/self-write gate
    /// and (on a genuine change) a full `put_clipboard`.
    pub async fn push(&self, content: LocalContent) -> SyncOutcome {
        let (entry, payload) = match build_push_entry(&content) {
            Ok(v) => v,
            Err(e) => return SyncOutcome::Failed { error: e },
        };
        let mut guard = self.state.lock().await;
        let now = now_ms();
        self.fold_persisted_watermark(&mut guard);

        if let Some(outcome) = paused_outcome(&guard.runtime) {
            return outcome;
        }

        let auto_apply = guard.settings.auto_apply;
        let device_hash = entry.hash.clone();
        let push_job = PushJob { entry, payload };
        self.run_route(
            &mut guard,
            RouteRequest {
                auto_apply,
                auto_push: true, // the call to push() IS the consent (Q2 note).
                device_hash,
                device_present: true,
                push_job: Some(push_job),
            },
            now,
        )
        .await
    }

    /// Probe for (and, per `auto_apply`, apply) server-new content. `trigger`
    /// decides the backoff gate and SSE short-circuit; `current_device_hash` feeds
    /// the truth-gate convergence check (Q1).
    pub async fn pull(
        &self,
        trigger: PullTrigger,
        current_device_hash: Option<String>,
    ) -> SyncOutcome {
        let mut guard = self.state.lock().await;
        let now = now_ms();
        self.fold_persisted_watermark(&mut guard);

        if let Some(outcome) = paused_outcome(&guard.runtime) {
            return outcome;
        }

        if let PullTrigger::SseUpdate { content_id } = &trigger {
            if nonempty(content_id) == nonempty_opt(guard.runtime.last_synced_content_id.as_deref())
                && nonempty(content_id).is_some()
            {
                return SyncOutcome::UpToDate {
                    reason: UpToDateReason::SseShortCircuit,
                };
            }
        }

        let explicit = !matches!(trigger, PullTrigger::Routine);
        if !explicit {
            if let Some(next) = guard.runtime.next_attempt_ms {
                if now < next {
                    return SyncOutcome::BackingOff {
                        retry_after_ms: next - now,
                    };
                }
            }
        }

        let auto_apply = guard.settings.auto_apply;
        let device_present = current_device_hash.is_some();
        self.run_route(
            &mut guard,
            RouteRequest {
                auto_apply,
                auto_push: false, // pull never uploads — force the Push branch to a no-op.
                device_hash: current_device_hash,
                device_present,
                push_job: None,
            },
            now,
        )
        .await
    }

    /// User tapped "apply" on a staged banner (`auto_apply` off). Downloads the
    /// staged entry's bytes now and advances the watermark exactly like a
    /// `pull`-triggered apply.
    pub async fn apply_staged(&self) -> SyncOutcome {
        let mut guard = self.state.lock().await;
        let now = now_ms();

        let Some(entry) = guard.runtime.staged_entry.clone() else {
            return SyncOutcome::Failed {
                error: SyncError::InvalidInput {
                    reason: "apply_staged called with nothing staged".into(),
                },
            };
        };
        let server = guard.server.clone();
        match self.fetch_content(&server, &entry).await {
            Ok(content) => {
                if !se::mark_staged_applied(&mut guard.runtime) {
                    return SyncOutcome::Failed {
                        error: SyncError::Internal {
                            reason: "mark_staged_applied no-op despite a staged entry".into(),
                        },
                    };
                }
                self.persist_watermark(&guard);
                se::commit_tick_success(&mut guard.runtime);
                SyncOutcome::Applied {
                    content,
                    meta: synced_meta_from_proto(&entry),
                }
            }
            Err(e) => {
                let cfg = guard.cfg;
                se::commit_tick_failure(
                    &mut guard.runtime,
                    sync_error_to_tick_kind(&e),
                    jitter(),
                    now,
                    &cfg,
                );
                SyncOutcome::Failed { error: e }
            }
        }
    }

    /// Switch the active server. A genuine change (Q8) clears the durable
    /// watermark (new server, new content timeline) and resets in-memory runtime
    /// state; setting the same server again is a no-op.
    ///
    /// `async` (design's sketch shows this as a plain sync `fn`) — a
    /// synchronous `blocking_lock()` on the shared `tokio::sync::Mutex` panics
    /// if the caller happens to be inside a tokio task (proven by this crate's
    /// own tests; plausible for any future caller too), and there is no
    /// portable way to block-only-if-not-already-in-a-runtime. Uniffi bridges
    /// an async Rust fn to a Swift `async`/Kotlin `suspend` function
    /// automatically, so this is a mechanical, low-cost change for callers.
    pub async fn set_server(&self, server: ServerConfig) {
        let mut guard = self.state.lock().await;
        if !same_server(&guard.server, &server) {
            se::handle_active_server_changed(&mut guard.runtime);
            self.store
                .remove(persist_keys::files::LAST_SYNCED_HASH.to_string());
            self.store
                .remove(persist_keys::files::LAST_SYNCED_CONTENT_ID.to_string());
        }
        guard.server = server;
    }

    /// Network path changed (Wi-Fi/cellular flip) — the backoff accumulated
    /// against the dead route says nothing about the new one. `async`, see
    /// [`Self::set_server`]'s doc comment for why.
    pub async fn handle_network_route_changed(&self) {
        let mut guard = self.state.lock().await;
        se::handle_network_route_changed(&mut guard.runtime);
    }

    /// `async`, see [`Self::set_server`]'s doc comment for why.
    pub async fn set_settings(&self, settings: SyncSettings) {
        let mut guard = self.state.lock().await;
        guard.settings = settings;
    }

    /// User dismissed the loop-detected banner — clear the trip buffer and
    /// resume. `async`, see [`Self::set_server`]'s doc comment for why.
    pub async fn acknowledge_loop_detected(&self) {
        let mut guard = self.state.lock().await;
        se::acknowledge_loop_detection(&mut guard.runtime);
    }
}

impl MobileSyncEngine {
    /// The internal sync-tick shared by `push`/`pull` (design §3/§6.2): `get_latest`
    /// → route → commit. `push_job` is `Some` only from [`Self::push`] — pull never
    /// uploads, so a `DoPush` route (unreachable in practice since pull always
    /// passes `auto_push: false`) falls through to the no-op arm below regardless.
    async fn run_route(&self, guard: &mut EngineState, req: RouteRequest, now: i64) -> SyncOutcome {
        let server = guard.server.clone();
        let entry = match self.get_latest_entry(&server).await {
            Ok(e) => e,
            Err(e) => {
                se::commit_tick_failure(
                    &mut guard.runtime,
                    sync_error_to_tick_kind(&e),
                    jitter(),
                    now,
                    &guard.cfg,
                );
                return SyncOutcome::Failed { error: e };
            }
        };
        let route = {
            let snap = se::ServerGetSnapshot {
                auto_apply: req.auto_apply,
                auto_push: req.auto_push,
                server_entry: entry.clone(),
                device_present: req.device_present,
                device_hash: req.device_hash,
            };
            se::plan_after_server_get(&guard.runtime, &snap)
        };

        match route {
            se::ServerRoute::Converged { server_hash } => {
                let content_id = entry.as_ref().and_then(|e| e.content_id.as_deref());
                se::commit_converged(&mut guard.runtime, &server_hash, content_id);
                self.persist_watermark(guard);
                se::commit_tick_success(&mut guard.runtime);
                SyncOutcome::UpToDate {
                    reason: UpToDateReason::Converged,
                }
            }
            se::ServerRoute::ServerNew(plan) => {
                let Some(server_entry) = entry else {
                    return SyncOutcome::Failed {
                        error: SyncError::Internal {
                            reason: "ServerNew route without a server entry".into(),
                        },
                    };
                };
                self.handle_server_new(guard, plan, server_entry, now).await
            }
            se::ServerRoute::Push(decision) => match (decision, req.push_job) {
                (se::PushDecision::DoPush, Some(job)) => self.do_push(guard, job, now).await,
                (decision, Some(_)) => {
                    // A push() call whose own content didn't warrant an upload —
                    // the granular skip reason is meaningful here (device_hash is
                    // the content being pushed).
                    se::commit_push_skipped(&mut guard.runtime);
                    se::commit_tick_success(&mut guard.runtime);
                    SyncOutcome::UpToDate {
                        reason: push_skip_reason(decision),
                    }
                }
                (_, None) => {
                    // A pull() call reaching this branch means `get_latest`
                    // confirmed the server already matches what's synced (that's
                    // why routing didn't take the `ServerNew` branch) — push
                    // semantics (forced `auto_push: false`, see `Self::pull`)
                    // don't apply, so the specific `PushDecision` variant carries
                    // no meaning for a pull; always report `AlreadySynced`.
                    se::commit_push_skipped(&mut guard.runtime);
                    se::commit_tick_success(&mut guard.runtime);
                    SyncOutcome::UpToDate {
                        reason: UpToDateReason::AlreadySynced,
                    }
                }
            },
        }
    }

    async fn handle_server_new(
        &self,
        guard: &mut EngineState,
        plan: se::ServerNewPlan,
        server_entry: ProtoClipboard,
        now: i64,
    ) -> SyncOutcome {
        if plan.already_staged {
            // Parked already (fresh stage, or a prior apply failure) — a no-op
            // tick, matching the native `processServerNew`'s already-staged
            // short-circuit (skips the bytes round-trip and re-notification).
            se::commit_tick_success(&mut guard.runtime);
            return SyncOutcome::Staged {
                preview: staged_preview_from_proto(&server_entry),
            };
        }
        if !plan.will_apply {
            se::commit_stage(&mut guard.runtime, &server_entry);
            se::commit_tick_success(&mut guard.runtime);
            return SyncOutcome::Staged {
                preview: staged_preview_from_proto(&server_entry),
            };
        }
        let server = guard.server.clone();
        match self.fetch_content(&server, &server_entry).await {
            Ok(content) => {
                let outcome = se::commit_apply(
                    &mut guard.runtime,
                    server_entry.hash.as_deref(),
                    server_entry.content_id.as_deref(),
                    now,
                    &guard.cfg,
                );
                self.persist_watermark(guard);
                se::commit_tick_success(&mut guard.runtime);
                if outcome.tripped {
                    SyncOutcome::LoopDetected
                } else {
                    SyncOutcome::Applied {
                        content,
                        meta: synced_meta_from_proto(&server_entry),
                    }
                }
            }
            Err(e) => {
                se::commit_apply_failed(&mut guard.runtime, &server_entry);
                se::commit_tick_failure(
                    &mut guard.runtime,
                    sync_error_to_tick_kind(&e),
                    jitter(),
                    now,
                    &guard.cfg,
                );
                SyncOutcome::Failed { error: e }
            }
        }
    }

    async fn do_push(&self, guard: &mut EngineState, job: PushJob, now: i64) -> SyncOutcome {
        let server = guard.server.clone();
        let meta = ClipboardMeta::from_proto(job.entry.clone());
        let hash = job.entry.hash.clone();
        match self.client.put_clipboard(server, meta, job.payload).await {
            Ok(content_id) => {
                let outcome = se::commit_push(
                    &mut guard.runtime,
                    hash.as_deref(),
                    content_id.as_deref(),
                    now,
                    &guard.cfg,
                );
                self.persist_watermark(guard);
                se::commit_tick_success(&mut guard.runtime);
                if outcome.tripped {
                    SyncOutcome::LoopDetected
                } else {
                    SyncOutcome::Uploaded {
                        meta: synced_meta_from_proto(&job.entry),
                    }
                }
            }
            Err(e) => {
                se::commit_tick_failure(
                    &mut guard.runtime,
                    sync_error_to_tick_kind(&e),
                    jitter(),
                    now,
                    &guard.cfg,
                );
                SyncOutcome::Failed { error: e }
            }
        }
    }

    /// `GET /SyncClipboard.json`, folding the documented "empty server" 404 into
    /// `Ok(None)` (matches `plan_after_server_get`'s `server_entry: None` input
    /// shape) rather than propagating it as an error.
    async fn get_latest_entry(
        &self,
        server: &ServerConfig,
    ) -> Result<Option<ProtoClipboard>, SyncError> {
        match self.client.get_latest(server.clone()).await {
            Ok(meta) => Ok(Some(meta.into_proto())),
            Err(SyncError::NotFound) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Download a server entry's bytes when it carries a payload; inline text
    /// needs no round-trip.
    async fn fetch_content(
        &self,
        server: &ServerConfig,
        entry: &ProtoClipboard,
    ) -> Result<LocalContent, SyncError> {
        let payload = if entry.has_data {
            let name = entry
                .data_name
                .clone()
                .ok_or_else(|| SyncError::DecodingFailed {
                    reason: "server entry has_data but no data_name".into(),
                })?;
            Some(self.client.get_file(server.clone(), name).await?)
        } else {
            None
        };
        Ok(LocalContent {
            kind: entry.kind.into(),
            text: entry.text.clone(),
            data_name: entry.data_name.clone(),
            payload,
        })
    }

    /// Cross-process resync (design §7's load-before-op, simplified from
    /// `plan_preamble`'s fold): pull in whatever the Share Extension (or another
    /// process) last wrote before deciding anything. Done unconditionally, ahead
    /// of the paused/backoff gates — it is a cheap local read, and doing it first
    /// lets the `SseUpdate` short-circuit observe a cross-process write even while
    /// this engine instance is backed off.
    fn fold_persisted_watermark(&self, guard: &mut EngineState) {
        let persisted_hash = self
            .store
            .get(persist_keys::files::LAST_SYNCED_HASH.to_string())
            .and_then(|b| String::from_utf8(b).ok());
        let persisted_content_id = self
            .store
            .get(persist_keys::files::LAST_SYNCED_CONTENT_ID.to_string())
            .and_then(|b| String::from_utf8(b).ok());

        let hash_differs = !se::hashes_equal(
            persisted_hash.as_deref(),
            guard.runtime.last_synced_hash.as_deref(),
        );
        let content_id_differs = nonempty_opt(persisted_content_id.as_deref())
            != nonempty_opt(guard.runtime.last_synced_content_id.as_deref());
        if hash_differs || content_id_differs {
            guard.runtime.last_synced_hash = persisted_hash.map(|h| h.to_uppercase());
            guard.runtime.last_synced_content_id =
                nonempty_opt(persisted_content_id.as_deref()).map(str::to_string);
        }
    }

    /// Save-after-op (design §7): write the watermark back after any
    /// `advance_synced`-triggering commit.
    fn persist_watermark(&self, guard: &EngineState) {
        match guard.runtime.last_synced_hash.as_deref() {
            Some(h) => self.store.set(
                persist_keys::files::LAST_SYNCED_HASH.to_string(),
                h.as_bytes().to_vec(),
            ),
            None => self
                .store
                .remove(persist_keys::files::LAST_SYNCED_HASH.to_string()),
        }
        match guard.runtime.last_synced_content_id.as_deref() {
            Some(c) => self.store.set(
                persist_keys::files::LAST_SYNCED_CONTENT_ID.to_string(),
                c.as_bytes().to_vec(),
            ),
            None => self
                .store
                .remove(persist_keys::files::LAST_SYNCED_CONTENT_ID.to_string()),
        }
    }
}

/// What to do with a push/pull job pending upload — carries the already-built wire
/// entry (SHA-256 hash + long-text-overflow/file-naming already resolved via
/// [`ProtoClipboard::publish_text`]/`publish_image`/`publish_file`) plus its bytes.
struct PushJob {
    entry: ProtoClipboard,
    payload: Option<Vec<u8>>,
}

/// Bundles [`run_route`](MobileSyncEngine::run_route)'s inputs — the only
/// difference between a `push`- and a `pull`-driven tick.
struct RouteRequest {
    auto_apply: bool,
    auto_push: bool,
    device_hash: Option<String>,
    device_present: bool,
    push_job: Option<PushJob>,
}

/// Build the wire entry + payload for a [`LocalContent`] the same way the daemon's
/// own publish path does (SHA-256 hash, long-text-overflow, file naming) — NOT via
/// `uc_content_hash`/blake3, which is a separate, server-assigned identity axis the
/// device never computes (see findings.md #11: mixing the two would silently break
/// `hashes_equal`, which expects one consistent hash shape on both sides).
fn build_push_entry(
    content: &LocalContent,
) -> Result<(ProtoClipboard, Option<Vec<u8>>), SyncError> {
    match content.kind {
        ClipboardKind::Text => {
            let (entry, payload) = publish_text(&content.text);
            Ok((entry, payload))
        }
        ClipboardKind::Image => {
            let bytes = content
                .payload
                .clone()
                .ok_or_else(|| SyncError::InvalidInput {
                    reason: "Image content requires payload bytes".into(),
                })?;
            let ext = content
                .data_name
                .as_deref()
                .and_then(extension_of)
                .unwrap_or("png");
            let (entry, payload) = publish_image(&bytes, ext);
            Ok((entry, Some(payload)))
        }
        ClipboardKind::File => {
            let bytes = content
                .payload
                .clone()
                .ok_or_else(|| SyncError::InvalidInput {
                    reason: "File content requires payload bytes".into(),
                })?;
            let name = content
                .data_name
                .clone()
                .ok_or_else(|| SyncError::InvalidInput {
                    reason: "File content requires data_name".into(),
                })?;
            let (entry, payload) = publish_file(&name, &bytes);
            Ok((entry, Some(payload)))
        }
        ClipboardKind::Group => Err(SyncError::InvalidInput {
            reason: "Group content cannot be pushed".into(),
        }),
    }
}

fn extension_of(name: &str) -> Option<&str> {
    name.rsplit('.').next().filter(|s| !s.is_empty())
}

fn same_server(a: &ServerConfig, b: &ServerConfig) -> bool {
    a.base_url == b.base_url && a.username == b.username && a.password == b.password
}

fn synced_meta_from_proto(entry: &ProtoClipboard) -> SyncedMeta {
    SyncedMeta {
        kind: entry.kind.into(),
        hash: entry.hash.clone(),
        content_id: entry.content_id.clone(),
        text: Some(entry.text.clone()),
        size: entry.size,
    }
}

fn staged_preview_from_proto(entry: &ProtoClipboard) -> StagedPreview {
    StagedPreview {
        kind: entry.kind.into(),
        text: entry.text.clone(),
        size: entry.size,
    }
}

/// Only called for a `push()`-originated `Push` route (see `run_route`) — a
/// `pull()`-originated one always reports `AlreadySynced` directly, since push
/// semantics (and `auto_push`, forced `false` for `pull`) don't apply to it.
fn push_skip_reason(decision: se::PushDecision) -> UpToDateReason {
    match decision {
        se::PushDecision::SkipConsentMode => UpToDateReason::ConsentMode,
        se::PushDecision::SkipNoDevice => UpToDateReason::NoLocalChange,
        se::PushDecision::SkipAlreadySynced => UpToDateReason::AlreadySynced,
        se::PushDecision::SkipSelfWritten => UpToDateReason::SelfWritten,
        // `run_route`'s `DoPush` arm handles this variant before this fn is
        // called — kept for match exhaustiveness.
        se::PushDecision::DoPush => UpToDateReason::NoLocalChange,
    }
}

/// `Some` only for a non-empty `contentId` — mirrors the reducer's own
/// `nonempty_content_id` (private to `uc_mobile_proto`): an empty string carries no
/// cross-device identity.
fn nonempty_opt(s: Option<&str>) -> Option<&str> {
    s.filter(|s| !s.is_empty())
}

fn nonempty(s: &str) -> Option<&str> {
    nonempty_opt(Some(s))
}

fn sync_error_to_tick_kind(e: &SyncError) -> se::TickErrorKind {
    match e {
        SyncError::Unauthorized => se::TickErrorKind::AuthFailed,
        SyncError::Cancelled => se::TickErrorKind::Cancelled,
        SyncError::Network { .. } => se::TickErrorKind::NetworkUnreachable,
        SyncError::NotInitialized
        | SyncError::InvalidInput { .. }
        | SyncError::NotFound
        | SyncError::ServerError { .. }
        | SyncError::ProtocolError { .. }
        | SyncError::DecodingFailed { .. }
        | SyncError::Internal { .. } => se::TickErrorKind::OtherSyncError,
    }
}

/// `Some(outcome)` when the engine is paused (401, or an unacknowledged loop trip)
/// and the caller should stop here regardless of trigger — matches
/// `plan_preamble`'s `Stop(Paused)` branch, which fires even for an explicit/
/// punch-through call.
fn paused_outcome(runtime: &se::SyncRuntimeState) -> Option<SyncOutcome> {
    match runtime.state {
        se::SyncState::AuthFailed => Some(SyncOutcome::Failed {
            error: SyncError::Unauthorized,
        }),
        se::SyncState::LoopDetected => Some(SyncOutcome::LoopDetected),
        _ => None,
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Backoff jitter in `[0.8, 1.2)` (Swift `Double.random(in: 0.8...1.2)`). No `rand`
/// dependency exists in this crate (mobile jetsam-budget discipline avoids adding
/// one for a non-cryptographic need) — clock-nanosecond entropy is more than
/// sufficient to avoid synchronized retries across devices.
fn jitter() -> f64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    0.8 + (nanos % 400) as f64 / 1000.0
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::net::SocketAddr;
    use std::sync::Mutex;

    use crate::client::tests::{new_client, server_cfg, spawn_mock, MockConfig};

    use super::*;

    /// In-memory `KeyValueStore` — the engine's only persistence port.
    #[derive(Default)]
    struct FakeStore {
        data: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl FakeStore {
        fn new() -> Arc<Self> {
            Arc::new(Self::default())
        }
    }

    impl KeyValueStore for FakeStore {
        fn get(&self, key: String) -> Option<Vec<u8>> {
            self.data.lock().expect("lock").get(&key).cloned()
        }
        fn set(&self, key: String, value: Vec<u8>) {
            self.data.lock().expect("lock").insert(key, value);
        }
        fn remove(&self, key: String) {
            self.data.lock().expect("lock").remove(&key);
        }
    }

    async fn engine_with(
        addr: SocketAddr,
        store: Arc<dyn KeyValueStore>,
        auto_apply: bool,
    ) -> Arc<MobileSyncEngine> {
        MobileSyncEngine::new(
            server_cfg(addr, "p"),
            crate::reducer::default_sync_config(),
            SyncSettings { auto_apply },
            store,
            new_client(),
        )
        .expect("engine constructs")
    }

    fn text_content(text: &str) -> LocalContent {
        LocalContent {
            kind: ClipboardKind::Text,
            text: text.to_string(),
            data_name: None,
            payload: None,
        }
    }

    fn image_content(name: &str, bytes: &[u8]) -> LocalContent {
        LocalContent {
            kind: ClipboardKind::Image,
            text: String::new(),
            data_name: Some(name.to_string()),
            payload: Some(bytes.to_vec()),
        }
    }

    fn file_content(name: &str, bytes: &[u8]) -> LocalContent {
        LocalContent {
            kind: ClipboardKind::File,
            text: String::new(),
            data_name: Some(name.to_string()),
            payload: Some(bytes.to_vec()),
        }
    }

    // ── design §9 scenario: two different images pushed back-to-back ───────
    // (reproduces the original bug this whole design fixes: the second upload
    // must actually happen, never silently skipped).

    #[tokio::test]
    async fn push_second_different_image_truly_uploads() {
        let seed = uc_mobile_proto::Clipboard::new(
            uc_mobile_proto::ClipboardKind::Text,
            Some("AA".into()),
            "seed".into(),
            false,
            None,
            Some(0),
        );
        let (addr, state) = spawn_mock(MockConfig {
            clip: Some(seed),
            ..Default::default()
        })
        .await;
        let store = FakeStore::new();
        store.set(
            persist_keys::files::LAST_SYNCED_HASH.to_string(),
            b"AA".to_vec(),
        );
        let engine = engine_with(addr, store, true).await;

        let out1 = engine.push(image_content("a.png", b"photo one")).await;
        assert!(matches!(out1, SyncOutcome::Uploaded { .. }), "{out1:?}");

        let out2 = engine.push(image_content("b.png", b"photo two")).await;
        assert!(matches!(out2, SyncOutcome::Uploaded { .. }), "{out2:?}");

        let uploads = state
            .events()
            .iter()
            .filter(|e| e.starts_with("put-file"))
            .count();
        assert_eq!(
            uploads,
            2,
            "both images must actually upload bytes: {:?}",
            state.events()
        );
    }

    // ── Gap 1: `File`-kind content, previously untested (build_push_entry's
    //    File branch + fetch_content's has_data download path) ──────────────

    #[tokio::test]
    async fn push_second_different_file_truly_uploads() {
        let seed = uc_mobile_proto::Clipboard::new(
            uc_mobile_proto::ClipboardKind::Text,
            Some("AA".into()),
            "seed".into(),
            false,
            None,
            Some(0),
        );
        let (addr, state) = spawn_mock(MockConfig {
            clip: Some(seed),
            ..Default::default()
        })
        .await;
        let store = FakeStore::new();
        store.set(
            persist_keys::files::LAST_SYNCED_HASH.to_string(),
            b"AA".to_vec(),
        );
        let engine = engine_with(addr, store, true).await;

        let out1 = engine.push(file_content("a.pdf", b"document one")).await;
        assert!(matches!(out1, SyncOutcome::Uploaded { .. }), "{out1:?}");

        let out2 = engine.push(file_content("b.pdf", b"document two")).await;
        assert!(matches!(out2, SyncOutcome::Uploaded { .. }), "{out2:?}");

        let uploads = state
            .events()
            .iter()
            .filter(|e| e.starts_with("put-file"))
            .count();
        assert_eq!(
            uploads,
            2,
            "both files must actually upload bytes: {:?}",
            state.events()
        );
    }

    #[tokio::test]
    async fn pull_applies_server_new_file_entry_by_downloading_via_get_file() {
        let bytes = b"a brand new pdf".to_vec();
        let (entry, _) = uc_mobile_proto::publish_file("report.pdf", &bytes);
        let (addr, state) = spawn_mock(MockConfig {
            clip: Some(entry),
            file_bytes: bytes.clone(),
            ..Default::default()
        })
        .await;
        let store = FakeStore::new();
        let engine = engine_with(addr, store, true).await;

        let out = engine.pull(PullTrigger::Explicit, None).await;
        let SyncOutcome::Applied { content, meta } = out else {
            panic!("expected Applied, got {out:?}");
        };
        assert_eq!(content.kind, ClipboardKind::File);
        assert_eq!(content.data_name.as_deref(), Some("report.pdf"));
        assert_eq!(content.payload.as_deref(), Some(bytes.as_slice()));
        assert_eq!(meta.kind, ClipboardKind::File);
        assert!(
            state.events().iter().any(|e| e == "get-file:report.pdf"),
            "must download via get_file: {:?}",
            state.events()
        );
    }

    // ── Gap 2: hash-drift via `content_id` right after a push. A server that
    //    tolerates hash drift on Image/File content (re-encodes what was
    //    uploaded — same `content_id`, different `hash`; the HEIC/JPEG case
    //    this project already hits on the capture side) can make the very
    //    next pull look like brand-new content, UNLESS the client learned the
    //    `content_id` from the PUT response itself
    //    (`MobileSyncClient::put_clipboard`'s return value, echoed by a real
    //    daemon's `SyncClipboardPutAck` — see `uc-webserver`'s
    //    `put_sync_clipboard_json`). Two scenarios, both real:
    //
    //    - A daemon/server whose PUT response carries no `content_id` (a
    //      legacy build, or a third-party SyncClipboard server) leaves the
    //      client with nothing to dedup on — this is an inherent limit of the
    //      classic protocol, not a bug this crate can fix alone.
    //    - A daemon whose PUT response DOES carry it (current `uc-webserver`)
    //      lets the client recognize the re-encoded GET as already-synced.

    #[tokio::test]
    async fn pull_after_push_reapplies_reencoded_content_when_daemon_gives_no_content_id() {
        let seed = uc_mobile_proto::Clipboard::new(
            uc_mobile_proto::ClipboardKind::Text,
            Some("AA".into()),
            "seed".into(),
            false,
            None,
            Some(0),
        );
        let reencoded_bytes = b"photo one, but re-encoded by the server".to_vec();
        let (addr, state) = spawn_mock(MockConfig {
            clip: Some(seed),
            file_bytes: reencoded_bytes.clone(),
            // No `put_ack_content_id` — simulates a legacy daemon / third-party
            // SyncClipboard server whose PUT response is a bare 200.
            ..Default::default()
        })
        .await;
        let store = FakeStore::new();
        store.set(
            persist_keys::files::LAST_SYNCED_HASH.to_string(),
            b"AA".to_vec(),
        );
        let engine = engine_with(addr, store, true).await;

        let out1 = engine.push(image_content("a.png", b"photo one")).await;
        let SyncOutcome::Uploaded { meta } = out1 else {
            panic!("expected Uploaded, got {out1:?}");
        };
        let device_hash = meta.hash.expect("push always reports a hash");

        // Server-side: re-encodes what was just uploaded — same identity,
        // different bytes/hash.
        let (mut drifted, _) = uc_mobile_proto::publish_image(&reencoded_bytes, "png");
        drifted.content_id = Some("blake3v1:reencoded".into());
        state.set_current_clip(Some(drifted));

        // Device pasteboard is untouched locally — still reports the
        // pre-encode hash.
        let out2 = engine.pull(PullTrigger::Explicit, Some(device_hash)).await;
        assert!(
            matches!(out2, SyncOutcome::Applied { .. }),
            "with no content_id learned from the push, the engine has \
             nothing to fall back on but the now-mismatched hash, so it \
             re-downloads and reapplies content semantically identical to \
             what was just pushed: {out2:?}"
        );
    }

    #[tokio::test]
    async fn pull_after_push_recognizes_reencoded_content_via_content_id_learned_from_put_response()
    {
        let seed = uc_mobile_proto::Clipboard::new(
            uc_mobile_proto::ClipboardKind::Text,
            Some("AA".into()),
            "seed".into(),
            false,
            None,
            Some(0),
        );
        let reencoded_bytes = b"photo one, but re-encoded by the server".to_vec();
        let (addr, state) = spawn_mock(MockConfig {
            clip: Some(seed),
            file_bytes: reencoded_bytes.clone(),
            // The (fixed) daemon echoes back the content_id it assigned to
            // this exact upload.
            put_ack_content_id: Some("blake3v1:reencoded".into()),
            ..Default::default()
        })
        .await;
        let store = FakeStore::new();
        store.set(
            persist_keys::files::LAST_SYNCED_HASH.to_string(),
            b"AA".to_vec(),
        );
        let engine = engine_with(addr, store, true).await;

        let out1 = engine.push(image_content("a.png", b"photo one")).await;
        let SyncOutcome::Uploaded { meta } = out1 else {
            panic!("expected Uploaded, got {out1:?}");
        };
        let device_hash = meta.hash.expect("push always reports a hash");

        // Server-side: re-encodes what was just uploaded, stamped with the
        // SAME content_id the PUT response just gave us.
        let (mut drifted, _) = uc_mobile_proto::publish_image(&reencoded_bytes, "png");
        drifted.content_id = Some("blake3v1:reencoded".into());
        state.set_current_clip(Some(drifted));

        let out2 = engine.pull(PullTrigger::Explicit, Some(device_hash)).await;
        assert!(
            matches!(
                out2,
                SyncOutcome::UpToDate {
                    reason: UpToDateReason::AlreadySynced
                }
            ),
            "content_id learned straight from the push's own response must \
             recognize the re-encoded GET as the same content — no re-download, \
             no pasteboard rewrite: {out2:?}"
        );
        assert!(
            !state.events().iter().any(|e| e.starts_with("get-file")),
            "must not re-download bytes it just uploaded: {:?}",
            state.events()
        );
    }

    // ── design §9 scenario: pull-applied content, pushed back later, is
    //    recognized as self-written even after unrelated activity moved the
    //    synced watermark (anti-loop guard #1) ───────────────────────────────

    #[tokio::test]
    async fn push_previously_applied_content_after_other_activity_is_self_written() {
        let x = "applied content X";
        let (x_entry, _) = uc_mobile_proto::publish_text(x);
        let (addr, _state) = spawn_mock(MockConfig {
            clip: Some(x_entry),
            ..Default::default()
        })
        .await;
        let store = FakeStore::new();
        let engine = engine_with(addr, store, true).await;

        // 1. pull applies X.
        let out1 = engine.pull(PullTrigger::Explicit, None).await;
        assert!(matches!(out1, SyncOutcome::Applied { .. }), "{out1:?}");

        // 2. push different content Z — advances the hash watermark without
        //    touching last_applied_hash (commit_push's documented asymmetry).
        let out2 = engine.push(text_content("something else Z")).await;
        assert!(matches!(out2, SyncOutcome::Uploaded { .. }), "{out2:?}");

        // 3. push X again — the server (mock) now echoes Z, so the truth-gate
        //    can't fire; is_already_synced matches Z so it's not server-new
        //    either; falls to the push branch, where last_applied_hash still
        //    remembers X.
        let out3 = engine.push(text_content(x)).await;
        assert!(
            matches!(
                out3,
                SyncOutcome::UpToDate {
                    reason: UpToDateReason::SelfWritten
                }
            ),
            "{out3:?}"
        );
    }

    // ── design §9 scenario: ping-pong trips the loop guard ──────────────────

    #[tokio::test]
    async fn ping_pong_same_content_trips_loop_guard() {
        let (h_entry, _) = uc_mobile_proto::publish_text("content H");
        let (b_entry, _) = uc_mobile_proto::publish_text("buffer content B");
        let (addr, state) = spawn_mock(MockConfig {
            clip: Some(b_entry.clone()),
            ..Default::default()
        })
        .await;
        let store = FakeStore::new();
        // Seed the watermark to match the mock's initial clip (B) so the
        // first push(H) below sees B as "already synced" (not server-new) and
        // reaches the push branch, instead of downloading B on a fresh engine
        // whose `last_synced_hash` starts as `None`.
        store.set(
            persist_keys::files::LAST_SYNCED_HASH.to_string(),
            b_entry.hash.clone().expect("hash present").into_bytes(),
        );
        let engine = engine_with(addr, store, true).await;

        // H's loop-guard bucket needs Pushed/Pulled to alternate 3 times
        // (default flip_threshold) to trip. Each "pull B" step is purely a
        // reset: it moves last_synced_hash/last_applied_hash off H so the next
        // operation on H is seen as fresh rather than self-written/converged.

        // step 1: push(H) — genuinely new, DoPush. H bucket: [Pushed].
        let out = engine.push(text_content("content H")).await;
        assert!(
            matches!(out, SyncOutcome::Uploaded { .. }),
            "step1: {out:?}"
        );

        // step 2: reset via pull(B).
        state.set_current_clip(Some(b_entry.clone()));
        let out = engine.pull(PullTrigger::Explicit, None).await;
        assert!(matches!(out, SyncOutcome::Applied { .. }), "step2: {out:?}");

        // step 3: pull(H) — server-new relative to B, applies. H bucket:
        // [Pushed, Pulled] = 1 flip.
        state.set_current_clip(Some(h_entry.clone()));
        let out = engine.pull(PullTrigger::Explicit, None).await;
        assert!(matches!(out, SyncOutcome::Applied { .. }), "step3: {out:?}");

        // step 4: reset via pull(B) again.
        state.set_current_clip(Some(b_entry.clone()));
        let out = engine.pull(PullTrigger::Explicit, None).await;
        assert!(matches!(out, SyncOutcome::Applied { .. }), "step4: {out:?}");

        // step 5: push(H) again — last_applied_hash was just reset to B, so
        // this is DoPush, not self-written. H bucket: [Pushed, Pulled, Pushed]
        // = 2 flips.
        let out = engine.push(text_content("content H")).await;
        assert!(
            matches!(out, SyncOutcome::Uploaded { .. }),
            "step5: {out:?}"
        );

        // step 6: reset via pull(B) again.
        state.set_current_clip(Some(b_entry.clone()));
        let out = engine.pull(PullTrigger::Explicit, None).await;
        assert!(matches!(out, SyncOutcome::Applied { .. }), "step6: {out:?}");

        // step 7: pull(H) — the 4th H-bucket event, completing the 3rd flip
        // ([Pushed, Pulled, Pushed, Pulled]) — must trip.
        state.set_current_clip(Some(h_entry.clone()));
        let out = engine.pull(PullTrigger::Explicit, None).await;
        assert!(matches!(out, SyncOutcome::LoopDetected), "step7: {out:?}");

        engine.acknowledge_loop_detected().await;
    }

    // ── design §9 scenario: SSE contentId short-circuit avoids the network ──

    #[tokio::test]
    async fn pull_sse_update_matching_content_id_short_circuits_without_network() {
        let (mut entry, _) = uc_mobile_proto::publish_text("hello");
        entry.content_id = Some("blake3v1:abc".into());
        let (addr, state) = spawn_mock(MockConfig {
            clip: Some(entry),
            ..Default::default()
        })
        .await;
        let store = FakeStore::new();
        let engine = engine_with(addr, store, true).await;

        let out1 = engine.pull(PullTrigger::Explicit, None).await;
        assert!(matches!(out1, SyncOutcome::Applied { .. }), "{out1:?}");
        let events_after_first = state.events().len();

        let out2 = engine
            .pull(
                PullTrigger::SseUpdate {
                    content_id: "blake3v1:abc".into(),
                },
                None,
            )
            .await;
        assert!(
            matches!(
                out2,
                SyncOutcome::UpToDate {
                    reason: UpToDateReason::SseShortCircuit
                }
            ),
            "{out2:?}"
        );
        assert_eq!(
            state.events().len(),
            events_after_first,
            "must not hit the network: {:?}",
            state.events()
        );
    }

    // ── design §9 scenario: cross-process watermark pickup ──────────────────

    #[tokio::test]
    async fn pull_picks_up_cross_process_watermark_and_reports_already_synced() {
        let (entry, _) = uc_mobile_proto::publish_text("shared");
        let hash = entry.hash.clone().expect("hash present");
        let (addr, _state) = spawn_mock(MockConfig {
            clip: Some(entry),
            ..Default::default()
        })
        .await;
        let store = FakeStore::new();
        // Simulate the Share Extension having already synced this content.
        store.set(
            persist_keys::files::LAST_SYNCED_HASH.to_string(),
            hash.into_bytes(),
        );
        let engine = engine_with(addr, store, true).await;

        let out = engine.pull(PullTrigger::Explicit, None).await;
        assert!(
            matches!(
                out,
                SyncOutcome::UpToDate {
                    reason: UpToDateReason::AlreadySynced
                }
            ),
            "{out:?}"
        );
    }

    // ── design §9 / Q10 scenario: stale-clobber avoidance ────────────────────

    #[tokio::test]
    async fn push_yields_to_newer_server_content_instead_of_clobbering() {
        let (y_entry, _) = uc_mobile_proto::publish_text("server-side Y (newer)");
        let (addr, state) = spawn_mock(MockConfig {
            clip: Some(y_entry),
            ..Default::default()
        })
        .await;
        let store = FakeStore::new();
        // Seed a watermark distinct from Y so Y is recognized as server-new.
        store.set(
            persist_keys::files::LAST_SYNCED_HASH.to_string(),
            b"STALE".to_vec(),
        );
        let engine = engine_with(addr, store, true).await;

        let out = engine.push(text_content("local candidate X (stale)")).await;
        assert!(matches!(out, SyncOutcome::Applied { .. }), "{out:?}");
        assert!(
            !state
                .events()
                .iter()
                .any(|e| e.starts_with("put-doc") || e.starts_with("put-file")),
            "X must never be uploaded: {:?}",
            state.events()
        );
    }

    // ── design §9 scenario: sync-op backoff gates Routine but not Explicit ──

    #[tokio::test]
    async fn pull_routine_backs_off_but_explicit_punches_through() {
        let (addr, _state) = spawn_mock(MockConfig {
            forced_status: Some(500),
            ..Default::default()
        })
        .await;
        let store = FakeStore::new();
        let engine = engine_with(addr, store, true).await;

        let out1 = engine.pull(PullTrigger::Routine, None).await;
        assert!(matches!(out1, SyncOutcome::Failed { .. }), "{out1:?}");

        let out2 = engine.pull(PullTrigger::Routine, None).await;
        assert!(matches!(out2, SyncOutcome::BackingOff { .. }), "{out2:?}");

        let out3 = engine.pull(PullTrigger::Explicit, None).await;
        assert!(
            matches!(out3, SyncOutcome::Failed { .. }),
            "explicit punches through and attempts the network again: {out3:?}"
        );
    }

    // ── design §9 scenario: set_server clears the watermark ─────────────────

    #[tokio::test]
    async fn set_server_to_different_target_clears_watermark() {
        let (old_entry, _) = uc_mobile_proto::publish_text("old-server content");
        let (addr1, _s1) = spawn_mock(MockConfig {
            clip: Some(old_entry),
            ..Default::default()
        })
        .await;
        let store = FakeStore::new();
        let engine = engine_with(addr1, store.clone(), true).await;

        let out1 = engine.pull(PullTrigger::Explicit, None).await;
        assert!(matches!(out1, SyncOutcome::Applied { .. }), "{out1:?}");
        assert!(store
            .get(persist_keys::files::LAST_SYNCED_HASH.to_string())
            .is_some());

        let (new_entry, _) = uc_mobile_proto::publish_text("new-server content");
        let (addr2, _s2) = spawn_mock(MockConfig {
            clip: Some(new_entry),
            ..Default::default()
        })
        .await;
        engine.set_server(server_cfg(addr2, "p")).await;

        assert!(
            store
                .get(persist_keys::files::LAST_SYNCED_HASH.to_string())
                .is_none(),
            "watermark must be cleared on a genuine server switch"
        );

        let out2 = engine.pull(PullTrigger::Explicit, None).await;
        assert!(
            matches!(out2, SyncOutcome::Applied { .. }),
            "new server's content applies cleanly: {out2:?}"
        );
    }
}
