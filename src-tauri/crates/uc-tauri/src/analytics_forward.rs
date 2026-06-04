//! [`DaemonForwardingAnalyticsSink`] — the GUI's `AnalyticsPort` (ADR-008 D20).
//!
//! Under D2's process split the daemon is the **single authoritative
//! product-analytics sender**: it owns the `EventContext` (device id,
//! `active_device_count`, `is_first_run`, channel) and the PostHog key. Two
//! processes each running their own sink would double-count device-level
//! signals (§5.3 #9).
//!
//! The webview already routes its UI events through the daemon over HTTP
//! (`src/api/daemon/analytics.ts`). But a handful of update-lifecycle events are
//! emitted from the GUI's own **Rust** background tasks — the updater commands,
//! the update scheduler, the notify context — which the webview never sees. This
//! sink is the `AnalyticsPort` those call sites hold: instead of a second
//! in-process PostHog sink, it maps each update `Event` to the wire
//! [`CaptureUiEventRequest`] and forwards it to the daemon's `/analytics/capture`
//! endpoint. Call sites stay unchanged — they still `capture(Event::Update…)`.
//!
//! Fire-and-forget: the POST is spawned and its result only `debug`-logged.
//! Non-update events (the GUI client never emits any) are dropped with a debug
//! line — this sink is intentionally update-only, since the GUI process does not
//! compose an `EventContext` and must not emit device-level events.

use uc_daemon_client::{DaemonAnalyticsClient, DaemonConnectionState};
use uc_daemon_contract::api::dto::analytics::{
    CaptureUiEventRequest, UiDialogOpenSource, UiDismissSource, UiInstallKind,
    UiNotificationDeliveryStatus, UiUpdateAction, UiUpdateActionOutcome, UiUpdateCheckOutcome,
    UiUpdateCheckSource, UiUpdateFailureKind, UiUpdatePhase,
};
use uc_observability::analytics::{
    AnalyticsPort, DialogOpenSource, DismissSource, Event, InstallKind, NotificationDeliveryStatus,
    UpdateAction, UpdateActionOutcome, UpdateCheckOutcome, UpdateCheckSource, UpdateFailureKind,
    UpdatePhase,
};

/// The GUI's product-analytics sink: forwards update-lifecycle events to the
/// daemon's `/analytics/capture` endpoint. See the module docs for the rationale.
pub struct DaemonForwardingAnalyticsSink {
    client: DaemonAnalyticsClient,
}

impl DaemonForwardingAnalyticsSink {
    /// Build the sink over the shared daemon connection state. The connection
    /// info may still be empty at construction time (the GUI connects to the
    /// daemon later); each `capture` reads it lazily and drops the event if the
    /// daemon is not reachable yet.
    pub fn new(connection_state: DaemonConnectionState) -> Self {
        Self {
            client: DaemonAnalyticsClient::new(connection_state),
        }
    }
}

impl AnalyticsPort for DaemonForwardingAnalyticsSink {
    fn capture(&self, event: Event) {
        let Some(request) = to_capture_request(event) else {
            return;
        };
        let client = self.client.clone();
        // `capture` is sync fire-and-forget; the HTTP POST is async. Spawn it on
        // the ambient tokio runtime (the GUI runs the updater / scheduler inside
        // it). Outside a runtime — e.g. a sync unit test — drop with a debug line
        // rather than panic.
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                handle.spawn(async move {
                    if let Err(err) = client.capture(request).await {
                        tracing::debug!(
                            error = %err,
                            "daemon-forward analytics: capture failed (best-effort)"
                        );
                    }
                });
            }
            Err(_) => {
                tracing::debug!("daemon-forward analytics: no tokio runtime; dropping event");
            }
        }
    }
}

/// Map an analytics [`Event`] to the wire [`CaptureUiEventRequest`].
///
/// Only the five update-lifecycle variants are forwardable; any other event is
/// dropped (the GUI client must not emit device-level / non-update events). The
/// reverse mapping (wire → `Event`) lives in `uc-webserver::api::analytics`;
/// `mirror_enums_share_wire_form` there locks the wire equivalence of both sides.
fn to_capture_request(event: Event) -> Option<CaptureUiEventRequest> {
    match event {
        Event::UpdateDialogOpened {
            source,
            phase,
            install_kind,
        } => Some(CaptureUiEventRequest::DialogOpened {
            source: from_dialog_source(source),
            phase: from_phase(phase),
            install_kind: from_install_kind(install_kind),
        }),
        Event::UpdateDismissed { phase, source } => Some(CaptureUiEventRequest::Dismissed {
            phase: from_phase(phase),
            source: from_dismiss_source(source),
        }),
        Event::UpdateActionInvoked {
            action,
            outcome,
            error_kind,
        } => Some(CaptureUiEventRequest::ActionInvoked {
            action: from_action(action),
            outcome: from_action_outcome(outcome),
            error_kind,
        }),
        Event::UpdateCheckPerformed {
            source,
            outcome,
            failure_kind,
            install_kind,
        } => Some(CaptureUiEventRequest::CheckPerformed {
            source: from_check_source(source),
            outcome: from_check_outcome(outcome),
            failure_kind: failure_kind.map(from_failure_kind),
            install_kind: from_install_kind(install_kind),
        }),
        Event::UpdateNotificationShown {
            version,
            delivery_status,
            install_kind,
        } => Some(CaptureUiEventRequest::NotificationShown {
            version,
            delivery_status: from_delivery_status(delivery_status),
            install_kind: from_install_kind(install_kind),
        }),
        other => {
            tracing::debug!(
                event = other.name(),
                "daemon-forward analytics: dropping non-update event (GUI is not an analytics source)"
            );
            None
        }
    }
}

fn from_dialog_source(value: DialogOpenSource) -> UiDialogOpenSource {
    match value {
        DialogOpenSource::Notification => UiDialogOpenSource::Notification,
        DialogOpenSource::SidebarIcon => UiDialogOpenSource::SidebarIcon,
    }
}

fn from_phase(value: UpdatePhase) -> UiUpdatePhase {
    match value {
        UpdatePhase::Available => UiUpdatePhase::Available,
        UpdatePhase::Downloading => UiUpdatePhase::Downloading,
        UpdatePhase::Ready => UiUpdatePhase::Ready,
    }
}

fn from_dismiss_source(value: DismissSource) -> UiDismissSource {
    match value {
        DismissSource::DialogLater => UiDismissSource::DialogLater,
        DismissSource::DialogClosed => UiDismissSource::DialogClosed,
        DismissSource::PackageManagerDialogClosed => UiDismissSource::PackageManagerDialogClosed,
    }
}

fn from_action(value: UpdateAction) -> UiUpdateAction {
    match value {
        UpdateAction::DownloadBg => UiUpdateAction::DownloadBg,
        UpdateAction::Install => UiUpdateAction::Install,
    }
}

fn from_action_outcome(value: UpdateActionOutcome) -> UiUpdateActionOutcome {
    match value {
        UpdateActionOutcome::Started => UiUpdateActionOutcome::Started,
        UpdateActionOutcome::Succeeded => UiUpdateActionOutcome::Succeeded,
        UpdateActionOutcome::Failed => UiUpdateActionOutcome::Failed,
        UpdateActionOutcome::Cancelled => UiUpdateActionOutcome::Cancelled,
    }
}

fn from_install_kind(value: InstallKind) -> UiInstallKind {
    match value {
        InstallKind::Macos => UiInstallKind::Macos,
        InstallKind::Windows => UiInstallKind::Windows,
        InstallKind::WindowsPortable => UiInstallKind::WindowsPortable,
        InstallKind::AppImage => UiInstallKind::AppImage,
        InstallKind::Deb => UiInstallKind::Deb,
        InstallKind::Rpm => UiInstallKind::Rpm,
        InstallKind::Unknown => UiInstallKind::Unknown,
    }
}

fn from_check_source(value: UpdateCheckSource) -> UiUpdateCheckSource {
    match value {
        UpdateCheckSource::Startup => UiUpdateCheckSource::Startup,
        UpdateCheckSource::Scheduled => UiUpdateCheckSource::Scheduled,
        UpdateCheckSource::Manual => UiUpdateCheckSource::Manual,
        UpdateCheckSource::WindowShow => UiUpdateCheckSource::WindowShow,
    }
}

fn from_check_outcome(value: UpdateCheckOutcome) -> UiUpdateCheckOutcome {
    match value {
        UpdateCheckOutcome::Available => UiUpdateCheckOutcome::Available,
        UpdateCheckOutcome::UpToDate => UiUpdateCheckOutcome::UpToDate,
        UpdateCheckOutcome::Failed => UiUpdateCheckOutcome::Failed,
    }
}

fn from_failure_kind(value: UpdateFailureKind) -> UiUpdateFailureKind {
    match value {
        UpdateFailureKind::Network => UiUpdateFailureKind::Network,
        UpdateFailureKind::HttpError => UiUpdateFailureKind::HttpError,
        UpdateFailureKind::ParseError => UiUpdateFailureKind::ParseError,
        UpdateFailureKind::Other => UiUpdateFailureKind::Other,
    }
}

fn from_delivery_status(value: NotificationDeliveryStatus) -> UiNotificationDeliveryStatus {
    match value {
        NotificationDeliveryStatus::Sent => UiNotificationDeliveryStatus::Sent,
        NotificationDeliveryStatus::PermissionDenied => {
            UiNotificationDeliveryStatus::PermissionDenied
        }
        NotificationDeliveryStatus::SendFailed => UiNotificationDeliveryStatus::SendFailed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_check_performed_with_failure_kind() {
        let req = to_capture_request(Event::UpdateCheckPerformed {
            source: UpdateCheckSource::Manual,
            outcome: UpdateCheckOutcome::Failed,
            failure_kind: Some(UpdateFailureKind::HttpError),
            install_kind: InstallKind::Macos,
        });
        assert_eq!(
            req,
            Some(CaptureUiEventRequest::CheckPerformed {
                source: UiUpdateCheckSource::Manual,
                outcome: UiUpdateCheckOutcome::Failed,
                failure_kind: Some(UiUpdateFailureKind::HttpError),
                install_kind: UiInstallKind::Macos,
            })
        );
    }

    #[test]
    fn maps_notification_shown() {
        let req = to_capture_request(Event::UpdateNotificationShown {
            version: "0.13.0".to_string(),
            delivery_status: NotificationDeliveryStatus::Sent,
            install_kind: InstallKind::AppImage,
        });
        assert_eq!(
            req,
            Some(CaptureUiEventRequest::NotificationShown {
                version: "0.13.0".to_string(),
                delivery_status: UiNotificationDeliveryStatus::Sent,
                install_kind: UiInstallKind::AppImage,
            })
        );
    }

    #[test]
    fn drops_non_update_event() {
        assert_eq!(to_capture_request(Event::AppOpened), None);
    }
}
