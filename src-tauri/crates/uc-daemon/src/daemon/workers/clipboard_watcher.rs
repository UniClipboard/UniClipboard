//! Real clipboard watcher service for the daemon.
//!
//! Monitors OS clipboard changes via the platform-supplied event loop
//! (`uc_platform::clipboard::build_event_loop`), persists captured entries via
//! the application clipboard capture facade, and broadcasts the
//! `clipboard.new_content` WS event.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde::Serialize;
use tokio::sync::{broadcast, mpsc};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, instrument, warn, Instrument};

use uc_application::facade::{
    ClipboardCaptureFacade, ClipboardLiveIndexFacade, ClipboardLiveIndexInput,
    ClipboardLiveIndexOutcome, ClipboardOutboundFacade, ClipboardOutboundInput,
    ClipboardOutboundOutcome,
};
use uc_core::ids::EntryId;
use uc_core::ports::{ClipboardChangeHandler, ClipboardChangeOriginPort};
use uc_core::{ClipboardChangeOrigin, SystemClipboardSnapshot};
use uc_daemon_contract::constants::{ws_event, ws_topic};

use uc_observability::FlowId;
use uc_platform::clipboard::watcher::{ClipboardWatcher, PlatformEvent, PlatformEventSender};
use uc_platform::clipboard::{build_event_loop, shutdown_channel};

use crate::daemon::service::{DaemonService, ServiceHealth};
use uc_webserver::api::types::DaemonWsEvent;

// ---------------------------------------------------------------------------
// ClipboardNewContentPayload
// ---------------------------------------------------------------------------

/// Payload for the clipboard.new_content WS event.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClipboardNewContentPayload {
    entry_id: String,
    preview: String,
    origin: String,
}

// ---------------------------------------------------------------------------
// DaemonClipboardChangeHandler
// ---------------------------------------------------------------------------

/// Clipboard change handler for the daemon.
///
/// Invoked by ClipboardWatcherWorker for each de-duplicated clipboard change.
/// Persists entries via application clipboard capture facade and broadcasts a
/// clipboard.new_content WS event through the shared event broadcast channel.
///
/// The shared `clipboard_change_origin` instance is used to detect whether a
/// clipboard change was triggered by daemon inbound sync (RemotePush) or by
/// the local user (LocalCapture), preventing write-back loops.
pub struct DaemonClipboardChangeHandler {
    event_tx: broadcast::Sender<DaemonWsEvent>,
    clipboard_change_origin: Arc<dyn ClipboardChangeOriginPort>,
    /// Gate that controls whether clipboard capture is active.
    /// When false, clipboard change events are silently dropped. Defers capture
    /// while encryption is still locked; the daemon opens the gate once it
    /// unlocks and triggers its deferred services (ADR-008 P3-3: the former
    /// `GuiInProcess` `/lifecycle/ready` path is gone — daemon is always
    /// standalone and drives this itself).
    capture_gate: Arc<AtomicBool>,
    clipboard_capture: Arc<ClipboardCaptureFacade>,
    clipboard_live_index: Arc<ClipboardLiveIndexFacade>,
    clipboard_outbound: Arc<ClipboardOutboundFacade>,
}

impl DaemonClipboardChangeHandler {
    pub fn new(
        event_tx: broadcast::Sender<DaemonWsEvent>,
        clipboard_change_origin: Arc<dyn ClipboardChangeOriginPort>,
        capture_gate: Arc<AtomicBool>,
        clipboard_capture: Arc<ClipboardCaptureFacade>,
        clipboard_live_index: Arc<ClipboardLiveIndexFacade>,
        clipboard_outbound: Arc<ClipboardOutboundFacade>,
    ) -> Self {
        Self {
            event_tx,
            clipboard_change_origin,
            capture_gate,
            clipboard_capture,
            clipboard_live_index,
            clipboard_outbound,
        }
    }
}

#[async_trait]
impl ClipboardChangeHandler for DaemonClipboardChangeHandler {
    #[instrument(
        name = "daemon.on_clipboard_changed",
        level = "info",
        skip(self, snapshot),
        fields(trace_id = %FlowId::generate())
    )]
    async fn on_clipboard_changed(&self, snapshot: SystemClipboardSnapshot) -> Result<()> {
        if !self.capture_gate.load(Ordering::Relaxed) {
            debug!("Clipboard capture gate closed, skipping clipboard change");
            return Ok(());
        }
        let flow_id = FlowId::generate().to_string();

        // 1. Compute snapshot hash for write-back loop prevention.
        let origin_guard_key = snapshot.origin_guard_key();

        // 2. Check if this clipboard change was triggered by daemon inbound sync (RemotePush)
        //    or by the local user (LocalCapture). This prevents re-capturing content that
        //    the daemon itself wrote to the OS clipboard during inbound sync.
        let origin = self
            .clipboard_change_origin
            .consume_origin_for_snapshot_or_default(
                &origin_guard_key,
                ClipboardChangeOrigin::LocalCapture,
            )
            .await;

        debug!(
            origin_guard_key = %origin_guard_key,
            rep_count = snapshot.representations.len(),
            origin = ?origin,
            flow_id = %flow_id,
            "daemon clipboard watcher resolved origin for snapshot"
        );

        // RemotePush 是 apply_inbound 写入 OS 剪切板后 watcher 收到的回声。
        // entry 落库 / search 索引 / `clipboard.new_content` WS 事件均由
        // `ApplyInboundClipboardUseCase` 自己发出（流程入口的 `IncomingPending`
        // 和成功收尾时的 `NewContent`，详见 `apply_inbound/usecase.rs`）；
        // 这里若再跑一次 capture pipeline 会产生第二条 entry，用户层表现为
        // 同一份内容出现两份。直接短路返回，与 LocalRestore 在功能效果上对称
        // （LocalRestore 在 usecase 内部短路，RemotePush 因 apply_inbound 也
        // 走同一 usecase 入口，必须在 watcher 层短路才不会破坏入站落库路径）。
        if origin.is_remote_push() {
            debug!(
                origin_guard_key = %origin_guard_key,
                flow_id = %flow_id,
                "watcher: skip duplicate capture for RemotePush echo (already handled by apply_inbound)"
            );
            return Ok(());
        }

        // 3. Determine the origin string for the WS event payload.
        let origin_str = match origin {
            ClipboardChangeOrigin::LocalCapture | ClipboardChangeOrigin::LocalRestore => "local",
            ClipboardChangeOrigin::RemotePush { .. } => "remote",
            // ADR-005 §2.5 用户主动 resend:`ResendEntryUseCase` 直接调
            // dispatch,不经 capture 链;若有快照被消费 origin 标成 Resend
            // 后落到 watcher,说明上游决策被改坏。debug build / 测试里立
            // 即 panic 让回归暴露;release 软返回 + error 日志避免拖垮
            // watcher worker。
            ClipboardChangeOrigin::Resend => {
                debug_assert!(
                    false,
                    "watcher must not see ClipboardChangeOrigin::Resend (origin_guard_key={origin_guard_key})"
                );
                tracing::error!(
                    origin_guard_key = %origin_guard_key,
                    flow_id = %flow_id,
                    "watcher: unexpected Resend origin; dropping snapshot to avoid double capture"
                );
                return Ok(());
            }
        };

        // 4. Clone snapshot before capture consumes it.
        let outbound_snapshot = snapshot.clone();

        // watcher 不预设 entry_id —— 本地 capture 让 use case 自己分配。
        // flow_id 仅用于 watcher 自己的 tracing 关联,不再传给 use case。
        match self.clipboard_capture.capture(snapshot, origin, None).await {
            Ok(Some(captured)) => {
                let entry_id = EntryId::from(captured.entry_id.as_str());
                debug!(entry_id = %entry_id, ?origin, "Daemon clipboard capture succeeded");

                let payload = ClipboardNewContentPayload {
                    entry_id: captured.entry_id,
                    preview: "New clipboard content".to_string(),
                    origin: origin_str.to_string(),
                };
                let payload_value = match serde_json::to_value(payload) {
                    Ok(v) => v,
                    Err(e) => {
                        warn!(error = %e, "Failed to serialize clipboard.new_content payload");
                        return Ok(());
                    }
                };

                let event = DaemonWsEvent {
                    topic: ws_topic::CLIPBOARD.to_string(),
                    event_type: ws_event::CLIPBOARD_NEW_CONTENT.to_string(),
                    session_id: None,
                    ts: chrono::Utc::now().timestamp_millis(),
                    payload: payload_value,
                };

                // broadcast::send returns Err only when there are no receivers;
                // that's expected when no WS clients are connected — log at debug.
                if let Err(e) = self.event_tx.send(event) {
                    debug!(error = %e, "No WS subscribers for clipboard.new_content");
                }

                let search_span = tracing::info_span!("search.live_index", entry_id = %entry_id);
                match self
                    .clipboard_live_index
                    .index_capture(ClipboardLiveIndexInput {
                        entry_id: entry_id.to_string(),
                        snapshot: outbound_snapshot.clone(),
                    })
                    .instrument(search_span)
                    .await
                {
                    Ok(ClipboardLiveIndexOutcome::Indexed) => {
                        debug!(entry_id = %entry_id, "search: indexed captured entry");
                    }
                    Ok(ClipboardLiveIndexOutcome::Skipped { reason }) => {
                        debug!(entry_id = %entry_id, reason, "search: skipped live index");
                    }
                    Err(e) => {
                        warn!(error = %e, entry_id = %entry_id, "search: live index failed");
                    }
                }

                let clipboard_outbound = Arc::clone(&self.clipboard_outbound);
                let entry_id_for_outbound = entry_id.to_string();
                tokio::spawn(
                    async move {
                        match clipboard_outbound
                            .dispatch_capture(ClipboardOutboundInput {
                                entry_id: entry_id_for_outbound,
                                snapshot: outbound_snapshot,
                                origin,
                            })
                            .await
                        {
                            Ok(ClipboardOutboundOutcome::Dispatched {
                                accepted,
                                duplicate,
                                offline,
                                errored,
                                pending,
                                blob_ref_count,
                            }) => info!(
                                accepted,
                                duplicate,
                                offline,
                                errored,
                                pending,
                                blob_ref_count,
                                "Daemon outbound clipboard sync completed"
                            ),
                            Ok(ClipboardOutboundOutcome::Skipped { reason }) => {
                                debug!(reason, "Daemon outbound clipboard sync skipped");
                            }
                            Err(e) => warn!(
                                error = %e,
                                "Daemon outbound clipboard sync failed"
                            ),
                        }
                    }
                    .in_current_span(),
                );
            }
            Ok(None) => {
                // Dedup at use-case level (e.g. unsupported representation) — skip silently.
                debug!(origin_guard_key = %origin_guard_key, ?origin, "Clipboard capture returned None");
            }
            Err(e) => {
                warn!(error = %e, origin_guard_key = %origin_guard_key, ?origin, "Daemon clipboard capture failed");
            }
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// ClipboardWatcherWorker
// ---------------------------------------------------------------------------

/// Daemon service that monitors OS clipboard changes.
///
/// Delegates the OS-specific listener to `uc_platform::clipboard::build_event_loop`
/// (Linux: native `ext`/`wlr-data-control` or native `x11rb` + XFIXES;
/// macOS / Windows: `clipboard_rs`-wrapped adapter). `uc_platform::ClipboardWatcher`
/// performs dedup before forwarding the snapshot to `DaemonClipboardChangeHandler`
/// which persists and broadcasts WS events.
pub struct ClipboardWatcherWorker {
    local_clipboard: Arc<dyn uc_core::ports::SystemClipboardPort>,
    change_handler: Arc<DaemonClipboardChangeHandler>,
}

impl ClipboardWatcherWorker {
    pub fn new(
        local_clipboard: Arc<dyn uc_core::ports::SystemClipboardPort>,
        change_handler: Arc<DaemonClipboardChangeHandler>,
    ) -> Self {
        Self {
            local_clipboard,
            change_handler,
        }
    }
}

#[async_trait]
impl DaemonService for ClipboardWatcherWorker {
    fn name(&self) -> &str {
        "clipboard-watcher"
    }

    async fn start(&self, cancel: CancellationToken) -> anyhow::Result<()> {
        info!("clipboard watcher starting");

        // Channel to receive platform events from the blocking watcher thread.
        let (platform_tx, mut platform_rx): (PlatformEventSender, _) = mpsc::channel(64);

        // Create the uc-platform ClipboardWatcher (handles dedup logic).
        let handler = ClipboardWatcher::new(self.local_clipboard.clone(), platform_tx);

        // Build the platform-specific event loop. Linux runtime-detects
        // native Wayland (ext/wlr-data-control) vs native x11rb; macOS /
        // Windows return the clipboard_rs adapter.
        let event_loop = build_event_loop()?;
        let (shutdown_tx, shutdown_rx) = shutdown_channel();

        // Run the blocking event loop on a dedicated thread.
        let event_loop_join = tokio::task::spawn_blocking(move || {
            info!("clipboard watcher thread started");
            if let Err(e) = event_loop.run(handler, shutdown_rx) {
                warn!(error = %e, "Clipboard event loop returned error");
            }
            info!("clipboard watcher thread stopped");
        });

        let change_handler = self.change_handler.clone();

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    info!("clipboard watcher cancellation received");
                    shutdown_tx.signal();
                    break;
                }
                event = platform_rx.recv() => {
                    match event {
                        Some(PlatformEvent::ClipboardChanged { snapshot }) => {
                            if snapshot.is_empty() {
                                debug!("Clipboard changed event had no representations; skipping");
                                continue;
                            }
                            if let Err(e) = change_handler.on_clipboard_changed(snapshot).await {
                                warn!(error = %e, "Failed to handle clipboard change in daemon");
                            }
                        }
                        None => {
                            // Channel closed (watcher thread exited).
                            info!("Clipboard watcher platform channel closed");
                            break;
                        }
                    }
                }
            }
        }

        // Wait for the event loop thread to finish so we don't leave a
        // half-shut-down listener clinging to the OS clipboard.
        if let Err(e) = event_loop_join.await {
            warn!(error = %e, "Clipboard watcher join failed");
        }

        info!("clipboard watcher stopped");
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        // Cancellation is handled via CancellationToken in start().
        Ok(())
    }

    fn health_check(&self) -> ServiceHealth {
        ServiceHealth::Healthy
    }
}
