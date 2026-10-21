//! Settings change events on the WS topic `settings` (ADR-013 section 7).
//!
//! Two owners persist settings: the daemon's product store and the Engine. Each change becomes one
//! `settings.changed { owner, sections, revision }` without values; clients read `GET /settings`
//! again. Revisions are per owner and live in daemon memory only.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};
use uc_daemon_contract::constants::{ws_event, ws_topic};
use uc_desktop_settings::{DesktopSettingsStore, SettingsSection};
use uc_engine::{SettingsChanged, SettingsSectionSummary};

use crate::api::dto::settings::{
    SettingsChangedPayload, SettingsOwnerDto, SettingsRevisionsDto, SettingsSectionDto,
    SettingsSnapshotPayload,
};
use crate::api::server::DaemonApiState;
use crate::api::types::DaemonWsEvent;

/// The revision the daemon assigns to Engine settings changes. The Engine event has none, so the
/// daemon counts the `SettingsChanged` events it maps. Shared by every clone of the API state.
#[derive(Clone, Debug, Default)]
pub struct EngineSettingsRevision(Arc<AtomicU64>);

impl EngineSettingsRevision {
    pub fn current(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }

    fn advance(&self) -> u64 {
        self.0.fetch_add(1, Ordering::SeqCst) + 1
    }
}

/// Maps an Engine `SettingsChanged` to `settings.changed` with the next Engine revision.
pub fn engine_settings_changed_event(
    revision: &EngineSettingsRevision,
    changed: SettingsChanged,
) -> Option<DaemonWsEvent> {
    // No wildcard: a section added by a future Engine pin must fail to compile, not vanish.
    let sections = changed
        .sections
        .into_iter()
        .map(|section| match section {
            SettingsSectionSummary::General => SettingsSectionDto::General,
            SettingsSectionSummary::Sync => SettingsSectionDto::Sync,
            SettingsSectionSummary::RetentionPolicy => SettingsSectionDto::RetentionPolicy,
            SettingsSectionSummary::Security => SettingsSectionDto::Security,
            SettingsSectionSummary::Pairing => SettingsSectionDto::Pairing,
            SettingsSectionSummary::KeyboardShortcuts => SettingsSectionDto::KeyboardShortcuts,
            SettingsSectionSummary::FileSync => SettingsSectionDto::FileSync,
            SettingsSectionSummary::Network => SettingsSectionDto::Network,
            SettingsSectionSummary::MobileSync => SettingsSectionDto::MobileSync,
            SettingsSectionSummary::QuickPanel => SettingsSectionDto::QuickPanel,
        })
        .collect();
    changed_event(SettingsOwnerDto::Engine, sections, revision.advance())
}

fn desktop_section(section: SettingsSection) -> SettingsSectionDto {
    match section {
        SettingsSection::General => SettingsSectionDto::General,
    }
}

fn changed_event(
    owner: SettingsOwnerDto,
    sections: Vec<SettingsSectionDto>,
    revision: u64,
) -> Option<DaemonWsEvent> {
    let payload = SettingsChangedPayload {
        owner,
        sections,
        revision,
    };
    match serde_json::to_value(payload) {
        Ok(payload) => Some(DaemonWsEvent {
            topic: ws_topic::SETTINGS.to_string(),
            event_type: ws_event::SETTINGS_CHANGED.to_string(),
            session_id: None,
            ts: chrono::Utc::now().timestamp_millis(),
            payload,
        }),
        Err(error) => {
            warn!(error = %error, "failed to encode settings.changed");
            None
        }
    }
}

/// Each owner's current revision, sent as `settings.snapshot` when a client subscribes.
pub async fn snapshot_payload(state: &DaemonApiState) -> SettingsSnapshotPayload {
    SettingsSnapshotPayload {
        revisions: SettingsRevisionsDto {
            desktop: state.desktop_settings.snapshot().await.revision,
            engine: state.engine_settings_revision.current(),
        },
    }
}

/// Subscribes to the product store now and returns the task that forwards its notifications as
/// `settings.changed`. Call it before the HTTP server starts so no change is missed.
pub fn forward_desktop_settings(
    store: Arc<DesktopSettingsStore>,
    event_tx: broadcast::Sender<DaemonWsEvent>,
    cancel: CancellationToken,
) -> impl std::future::Future<Output = ()> + Send + 'static {
    let mut changes = store.subscribe();
    async move {
        loop {
            let received = tokio::select! {
                _ = cancel.cancelled() => return,
                received = changes.recv() => received,
            };
            let event = match received {
                Ok(change) => changed_event(
                    SettingsOwnerDto::Desktop,
                    change.sections.into_iter().map(desktop_section).collect(),
                    change.revision,
                ),
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    // Missed notifications cannot be replayed: report every section at the current
                    // revision so each client reads again.
                    warn!(
                        skipped,
                        "settings forwarder lagged; reporting every product section"
                    );
                    let revision = store.snapshot().await.revision;
                    changed_event(
                        SettingsOwnerDto::Desktop,
                        SettingsSection::ALL
                            .iter()
                            .copied()
                            .map(desktop_section)
                            .collect(),
                        revision,
                    )
                }
                Err(broadcast::error::RecvError::Closed) => return,
            };
            if let Some(event) = event {
                debug!(revision = %event.payload["revision"], "forwarding product settings change");
                let _ = event_tx.send(event);
            }
        }
    }
}
