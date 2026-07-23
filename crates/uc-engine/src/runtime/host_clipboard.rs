use std::sync::Arc;

use tokio::sync::Mutex;
use tracing::{error, warn};
use uc_application::clipboard_write::LocalActiveRegisterAdvancer;
use uc_application::facade::{
    ClipboardHostEvent, ClipboardLiveIndexInput, ClipboardLiveIndexOutcome, ClipboardOriginKind,
    ClipboardOutboundInput, ClipboardOutboundOutcome, HostEvent, HostEventBus,
};
use uc_core::ports::{SelfWriteLedgerPort, SystemClipboardPort};
use uc_core::{ClipboardChangeOrigin, TaskRegistry};

use super::ProductionSession;
use crate::{HostClipboardChange, HostClipboardChangeStream};

pub(super) struct HostClipboardChangeRuntime {
    pub(super) session: Arc<Mutex<Option<ProductionSession>>>,
    pub(super) system_clipboard: Arc<dyn SystemClipboardPort>,
    pub(super) change_origin: Arc<dyn SelfWriteLedgerPort>,
    pub(super) active_register: LocalActiveRegisterAdvancer,
    pub(super) host_events: Arc<HostEventBus>,
}

pub(super) async fn spawn_host_clipboard_change_task(
    mut changes: Box<dyn HostClipboardChangeStream>,
    runtime: HostClipboardChangeRuntime,
    tasks: Arc<TaskRegistry>,
) {
    tasks
        .spawn("host_clipboard_changes", move |cancel| async move {
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        if let Err(error) = changes.shutdown().await {
                            warn!(error = %error, "host clipboard change stream shutdown failed");
                        }
                        return;
                    }
                    change = changes.next() => match change {
                        Ok(HostClipboardChange::Changed) => {
                            if let Err(error) = runtime.process_change().await {
                                warn!(error = %error, "host clipboard change processing failed");
                            }
                        }
                        Ok(HostClipboardChange::Closed) => return,
                        Err(error) => {
                            warn!(error = %error, "host clipboard change stream failed");
                            return;
                        }
                    }
                }
            }
        })
        .await;
}

impl HostClipboardChangeRuntime {
    async fn process_change(&self) -> anyhow::Result<()> {
        let session = self.session.lock().await;
        let Some(session) = session.as_ref() else {
            return Ok(());
        };
        let encryption = session
            .facade
            .encryption
            .state()
            .await
            .map_err(|_| anyhow::anyhow!("host clipboard encryption state unavailable"))?;
        if !encryption.session_ready {
            return Ok(());
        }

        let snapshot = self
            .system_clipboard
            .read_snapshot()
            .map_err(|_| anyhow::anyhow!("host clipboard snapshot read failed"))?;
        if snapshot.is_empty() {
            return Ok(());
        }
        let origin_guard_key = snapshot.origin_guard_key();
        let origin = self
            .change_origin
            .attribute_observed_change(&origin_guard_key)
            .await;
        if origin.is_remote_push() {
            return Ok(());
        }
        if origin == ClipboardChangeOrigin::Resend {
            error!("host clipboard watcher observed an invalid resend origin");
            return Ok(());
        }

        let outbound_snapshot = Arc::new(snapshot.clone());
        let Some(captured) = session
            .clipboard
            .capture
            .capture(snapshot, origin, None)
            .await
            .map_err(|_| anyhow::anyhow!("host clipboard capture failed"))?
        else {
            return Ok(());
        };
        let entry_id = uc_core::ids::EntryId::from(captured.entry_id.as_str());
        self.active_register
            .advance_local(captured.snapshot_hash, entry_id)
            .await;
        self.host_events
            .emit_or_warn(HostEvent::Clipboard(ClipboardHostEvent::NewContent {
                entry_id: captured.entry_id.clone(),
                attempt_id: None,
                preview: "New clipboard content".to_string(),
                origin: ClipboardOriginKind::Local,
            }));

        if !captured.deduplicated {
            match session
                .clipboard
                .live_index
                .index_capture(ClipboardLiveIndexInput {
                    entry_id: captured.entry_id.clone(),
                    snapshot: Arc::clone(&outbound_snapshot),
                })
                .await
            {
                Ok(ClipboardLiveIndexOutcome::Indexed) => {}
                Ok(ClipboardLiveIndexOutcome::Skipped { reason }) => {
                    tracing::debug!(reason, "host clipboard live index skipped");
                }
                Err(error) => warn!(error = %error, "host clipboard live index failed"),
            }
        }

        let dispatch_snapshot =
            Arc::try_unwrap(outbound_snapshot).unwrap_or_else(|shared| (*shared).clone());
        let outbound = Arc::clone(&session.clipboard.outbound);
        session
            .tasks
            .spawn("host_clipboard_outbound", move |cancel| async move {
                let outcome = tokio::select! {
                    _ = cancel.cancelled() => return,
                    outcome = outbound.dispatch_capture(ClipboardOutboundInput {
                        entry_id: captured.entry_id,
                        snapshot: dispatch_snapshot,
                        origin,
                    }) => outcome,
                };
                match outcome {
                    Ok(ClipboardOutboundOutcome::Dispatched {
                        accepted,
                        duplicate,
                        offline,
                        errored,
                        pending,
                        ..
                    }) => tracing::info!(
                        accepted,
                        duplicate,
                        offline,
                        errored,
                        pending,
                        "host clipboard outbound sync completed"
                    ),
                    Ok(ClipboardOutboundOutcome::Skipped { reason }) => {
                        tracing::debug!(reason, "host clipboard outbound sync skipped");
                    }
                    Err(error) => warn!(error = %error, "host clipboard outbound sync failed"),
                }
            })
            .await;
        Ok(())
    }
}
