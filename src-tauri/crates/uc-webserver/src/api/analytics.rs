//! HTTP route handler for `POST /analytics/capture` (ADR-008 D20).
//!
//! The daemon is the single authoritative product-analytics sender. The GUI
//! webview POSTs its UI-interaction events here instead of emitting them from
//! its own in-process sink; the daemon dispatches each through `state.analytics`
//! (a `GatedAnalyticsSink` that honours the user's telemetry preference) using
//! the daemon's own `EventContext`. This prevents the two processes from
//! double-counting device-level signals in PostHog (§5.3 #9).
//!
//! ## Mapping ownership (orphan rule)
//!
//! The wire enums live in `uc-daemon-contract` (no `uc-observability` dep). The
//! contract→analytics mapping therefore lives here as free functions, mirroring
//! how `api/mobile_sync.rs` keeps its `to_*` conversions in the webserver.
//! `mirror_enums_share_wire_form` locks the two sides' wire equivalence.

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use tracing::info;
use uc_daemon_contract::api::dto::analytics::{
    CaptureUiEventRequest, CaptureUiEventResponse, UiDialogOpenSource, UiDismissSource,
    UiInstallKind, UiNotificationDeliveryStatus, UiUpdateAction, UiUpdateActionOutcome,
    UiUpdateCheckOutcome, UiUpdateCheckSource, UiUpdateFailureKind, UiUpdatePhase,
};
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use uc_observability::analytics::{
    DialogOpenSource, DismissSource, Event, InstallKind, NotificationDeliveryStatus, UpdateAction,
    UpdateActionOutcome, UpdateCheckOutcome, UpdateCheckSource, UpdateFailureKind, UpdatePhase,
};

use crate::api::dto::error::ApiError;
use crate::api::server::DaemonApiState;

pub fn router() -> Router<DaemonApiState> {
    Router::new().route("/analytics/capture", post(capture_handler))
}

// ─── contract → analytics mappings ───────────────────────────────────────────

fn map_dialog_source(value: UiDialogOpenSource) -> DialogOpenSource {
    match value {
        UiDialogOpenSource::Notification => DialogOpenSource::Notification,
        UiDialogOpenSource::SidebarIcon => DialogOpenSource::SidebarIcon,
    }
}

fn map_phase(value: UiUpdatePhase) -> UpdatePhase {
    match value {
        UiUpdatePhase::Available => UpdatePhase::Available,
        UiUpdatePhase::Downloading => UpdatePhase::Downloading,
        UiUpdatePhase::Ready => UpdatePhase::Ready,
    }
}

fn map_dismiss_source(value: UiDismissSource) -> DismissSource {
    match value {
        UiDismissSource::DialogLater => DismissSource::DialogLater,
        UiDismissSource::DialogClosed => DismissSource::DialogClosed,
        UiDismissSource::PackageManagerDialogClosed => DismissSource::PackageManagerDialogClosed,
    }
}

fn map_action(value: UiUpdateAction) -> UpdateAction {
    match value {
        UiUpdateAction::DownloadBg => UpdateAction::DownloadBg,
        UiUpdateAction::Install => UpdateAction::Install,
    }
}

fn map_outcome(value: UiUpdateActionOutcome) -> UpdateActionOutcome {
    match value {
        UiUpdateActionOutcome::Started => UpdateActionOutcome::Started,
        UiUpdateActionOutcome::Succeeded => UpdateActionOutcome::Succeeded,
        UiUpdateActionOutcome::Failed => UpdateActionOutcome::Failed,
        UiUpdateActionOutcome::Cancelled => UpdateActionOutcome::Cancelled,
    }
}

fn map_install_kind(value: UiInstallKind) -> InstallKind {
    match value {
        UiInstallKind::Macos => InstallKind::Macos,
        UiInstallKind::Windows => InstallKind::Windows,
        UiInstallKind::WindowsPortable => InstallKind::WindowsPortable,
        UiInstallKind::AppImage => InstallKind::AppImage,
        UiInstallKind::Deb => InstallKind::Deb,
        UiInstallKind::Rpm => InstallKind::Rpm,
        UiInstallKind::Unknown => InstallKind::Unknown,
    }
}

fn map_check_source(value: UiUpdateCheckSource) -> UpdateCheckSource {
    match value {
        UiUpdateCheckSource::Startup => UpdateCheckSource::Startup,
        UiUpdateCheckSource::Scheduled => UpdateCheckSource::Scheduled,
        UiUpdateCheckSource::Manual => UpdateCheckSource::Manual,
        UiUpdateCheckSource::WindowShow => UpdateCheckSource::WindowShow,
    }
}

fn map_check_outcome(value: UiUpdateCheckOutcome) -> UpdateCheckOutcome {
    match value {
        UiUpdateCheckOutcome::Available => UpdateCheckOutcome::Available,
        UiUpdateCheckOutcome::UpToDate => UpdateCheckOutcome::UpToDate,
        UiUpdateCheckOutcome::Failed => UpdateCheckOutcome::Failed,
    }
}

fn map_failure_kind(value: UiUpdateFailureKind) -> UpdateFailureKind {
    match value {
        UiUpdateFailureKind::Network => UpdateFailureKind::Network,
        UiUpdateFailureKind::HttpError => UpdateFailureKind::HttpError,
        UiUpdateFailureKind::ParseError => UpdateFailureKind::ParseError,
        UiUpdateFailureKind::Other => UpdateFailureKind::Other,
    }
}

fn map_delivery_status(value: UiNotificationDeliveryStatus) -> NotificationDeliveryStatus {
    match value {
        UiNotificationDeliveryStatus::Sent => NotificationDeliveryStatus::Sent,
        UiNotificationDeliveryStatus::PermissionDenied => {
            NotificationDeliveryStatus::PermissionDenied
        }
        UiNotificationDeliveryStatus::SendFailed => NotificationDeliveryStatus::SendFailed,
    }
}

/// Convert a decoded request into the analytics `Event`. Total — the discriminant
/// is validated at deserialization, so there is no error path here.
fn into_event(req: CaptureUiEventRequest) -> Event {
    match req {
        CaptureUiEventRequest::DialogOpened {
            source,
            phase,
            install_kind,
        } => Event::UpdateDialogOpened {
            source: map_dialog_source(source),
            phase: map_phase(phase),
            install_kind: map_install_kind(install_kind),
        },
        CaptureUiEventRequest::Dismissed { phase, source } => Event::UpdateDismissed {
            phase: map_phase(phase),
            source: map_dismiss_source(source),
        },
        CaptureUiEventRequest::ActionInvoked {
            action,
            outcome,
            error_kind,
        } => Event::UpdateActionInvoked {
            action: map_action(action),
            outcome: map_outcome(outcome),
            error_kind,
        },
        CaptureUiEventRequest::CheckPerformed {
            source,
            outcome,
            failure_kind,
            install_kind,
        } => Event::UpdateCheckPerformed {
            source: map_check_source(source),
            outcome: map_check_outcome(outcome),
            failure_kind: failure_kind.map(map_failure_kind),
            install_kind: map_install_kind(install_kind),
        },
        CaptureUiEventRequest::NotificationShown {
            version,
            delivery_status,
            install_kind,
        } => Event::UpdateNotificationShown {
            version,
            delivery_status: map_delivery_status(delivery_status),
            install_kind: map_install_kind(install_kind),
        },
    }
}

/// Short, body-free tag for tracing — never logs event fields.
fn event_kind_tag(req: &CaptureUiEventRequest) -> &'static str {
    match req {
        CaptureUiEventRequest::DialogOpened { .. } => "dialog_opened",
        CaptureUiEventRequest::Dismissed { .. } => "dismissed",
        CaptureUiEventRequest::ActionInvoked { .. } => "action_invoked",
        CaptureUiEventRequest::CheckPerformed { .. } => "check_performed",
        CaptureUiEventRequest::NotificationShown { .. } => "notification_shown",
    }
}

/// POST /analytics/capture
///
/// Decode a GUI UI-interaction event and hand it to the daemon's analytics sink.
/// Fire-and-forget: `accepted: true` only confirms the event was decoded and
/// dispatched, not that it reached PostHog.
#[utoipa::path(
    post,
    path = "/analytics/capture",
    operation_id = "captureUiEvent",
    tag = "analytics",
    request_body = CaptureUiEventRequest,
    responses(
        (status = 200, description = "Event accepted for dispatch", body = CaptureUiEventEnvelope),
    )
)]
async fn capture_handler(
    State(state): State<DaemonApiState>,
    Json(req): Json<CaptureUiEventRequest>,
) -> Result<Json<ApiEnvelope<CaptureUiEventResponse>>, ApiError> {
    // Tag only — never log the event body.
    info!(kind = event_kind_tag(&req), "analytics ui event captured");
    state.analytics.capture(into_event(req));
    Ok(Json(ApiEnvelope::now(CaptureUiEventResponse {
        accepted: true,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dialog_opened_maps_to_event_with_install_kind() {
        let req = CaptureUiEventRequest::DialogOpened {
            source: UiDialogOpenSource::SidebarIcon,
            phase: UiUpdatePhase::Downloading,
            install_kind: UiInstallKind::Deb,
        };
        match into_event(req) {
            Event::UpdateDialogOpened {
                source,
                phase,
                install_kind,
            } => {
                assert_eq!(source, DialogOpenSource::SidebarIcon);
                assert_eq!(phase, UpdatePhase::Downloading);
                assert_eq!(install_kind, InstallKind::Deb);
            }
            other => panic!("expected UpdateDialogOpened, got {other:?}"),
        }
    }

    #[test]
    fn action_invoked_preserves_error_kind() {
        let req = CaptureUiEventRequest::ActionInvoked {
            action: UiUpdateAction::Install,
            outcome: UiUpdateActionOutcome::Failed,
            error_kind: Some("signature_mismatch".into()),
        };
        match into_event(req) {
            Event::UpdateActionInvoked {
                action,
                outcome,
                error_kind,
            } => {
                assert_eq!(action, UpdateAction::Install);
                assert_eq!(outcome, UpdateActionOutcome::Failed);
                assert_eq!(error_kind.as_deref(), Some("signature_mismatch"));
            }
            other => panic!("expected UpdateActionInvoked, got {other:?}"),
        }
    }

    #[test]
    fn unknown_discriminator_is_rejected() {
        let result: Result<CaptureUiEventRequest, _> =
            serde_json::from_value(json!({ "kind": "totally_made_up", "phase": "available" }));
        assert!(result.is_err(), "unknown kind must fail to deserialize");
    }

    /// Lock the wire equivalence between the contract mirror enums and the
    /// analytics enums — serde must produce identical JSON for both sides so a
    /// future `rename_all` drift on either side is caught here.
    #[test]
    fn mirror_enums_share_wire_form() {
        fn same<U: serde::Serialize, A: serde::Serialize>(ui: U, analytics: A) {
            assert_eq!(
                serde_json::to_value(ui).unwrap(),
                serde_json::to_value(analytics).unwrap()
            );
        }

        same(
            UiDialogOpenSource::Notification,
            DialogOpenSource::Notification,
        );
        same(
            UiDialogOpenSource::SidebarIcon,
            DialogOpenSource::SidebarIcon,
        );

        same(UiUpdatePhase::Available, UpdatePhase::Available);
        same(UiUpdatePhase::Downloading, UpdatePhase::Downloading);
        same(UiUpdatePhase::Ready, UpdatePhase::Ready);

        same(UiDismissSource::DialogLater, DismissSource::DialogLater);
        same(UiDismissSource::DialogClosed, DismissSource::DialogClosed);
        same(
            UiDismissSource::PackageManagerDialogClosed,
            DismissSource::PackageManagerDialogClosed,
        );

        same(UiUpdateAction::DownloadBg, UpdateAction::DownloadBg);
        same(UiUpdateAction::Install, UpdateAction::Install);

        same(UiUpdateActionOutcome::Started, UpdateActionOutcome::Started);
        same(
            UiUpdateActionOutcome::Succeeded,
            UpdateActionOutcome::Succeeded,
        );
        same(UiUpdateActionOutcome::Failed, UpdateActionOutcome::Failed);
        same(
            UiUpdateActionOutcome::Cancelled,
            UpdateActionOutcome::Cancelled,
        );

        same(UiInstallKind::Macos, InstallKind::Macos);
        same(UiInstallKind::Windows, InstallKind::Windows);
        same(UiInstallKind::WindowsPortable, InstallKind::WindowsPortable);
        same(UiInstallKind::AppImage, InstallKind::AppImage);
        same(UiInstallKind::Deb, InstallKind::Deb);
        same(UiInstallKind::Rpm, InstallKind::Rpm);
        same(UiInstallKind::Unknown, InstallKind::Unknown);

        same(UiUpdateCheckSource::Startup, UpdateCheckSource::Startup);
        same(UiUpdateCheckSource::Scheduled, UpdateCheckSource::Scheduled);
        same(UiUpdateCheckSource::Manual, UpdateCheckSource::Manual);
        same(
            UiUpdateCheckSource::WindowShow,
            UpdateCheckSource::WindowShow,
        );

        same(
            UiUpdateCheckOutcome::Available,
            UpdateCheckOutcome::Available,
        );
        same(UiUpdateCheckOutcome::UpToDate, UpdateCheckOutcome::UpToDate);
        same(UiUpdateCheckOutcome::Failed, UpdateCheckOutcome::Failed);

        same(UiUpdateFailureKind::Network, UpdateFailureKind::Network);
        same(UiUpdateFailureKind::HttpError, UpdateFailureKind::HttpError);
        same(
            UiUpdateFailureKind::ParseError,
            UpdateFailureKind::ParseError,
        );
        same(UiUpdateFailureKind::Other, UpdateFailureKind::Other);

        same(
            UiNotificationDeliveryStatus::Sent,
            NotificationDeliveryStatus::Sent,
        );
        same(
            UiNotificationDeliveryStatus::PermissionDenied,
            NotificationDeliveryStatus::PermissionDenied,
        );
        same(
            UiNotificationDeliveryStatus::SendFailed,
            NotificationDeliveryStatus::SendFailed,
        );
    }

    #[test]
    fn check_performed_drops_failure_kind_when_succeeding() {
        let req = CaptureUiEventRequest::CheckPerformed {
            source: UiUpdateCheckSource::Scheduled,
            outcome: UiUpdateCheckOutcome::UpToDate,
            failure_kind: None,
            install_kind: UiInstallKind::AppImage,
        };
        match into_event(req) {
            Event::UpdateCheckPerformed {
                source,
                outcome,
                failure_kind,
                install_kind,
            } => {
                assert_eq!(source, UpdateCheckSource::Scheduled);
                assert_eq!(outcome, UpdateCheckOutcome::UpToDate);
                assert_eq!(failure_kind, None);
                assert_eq!(install_kind, InstallKind::AppImage);
            }
            other => panic!("expected UpdateCheckPerformed, got {other:?}"),
        }
    }

    #[test]
    fn check_performed_preserves_failure_kind() {
        let req = CaptureUiEventRequest::CheckPerformed {
            source: UiUpdateCheckSource::Manual,
            outcome: UiUpdateCheckOutcome::Failed,
            failure_kind: Some(UiUpdateFailureKind::Network),
            install_kind: UiInstallKind::Macos,
        };
        match into_event(req) {
            Event::UpdateCheckPerformed {
                outcome,
                failure_kind,
                ..
            } => {
                assert_eq!(outcome, UpdateCheckOutcome::Failed);
                assert_eq!(failure_kind, Some(UpdateFailureKind::Network));
            }
            other => panic!("expected UpdateCheckPerformed, got {other:?}"),
        }
    }

    #[test]
    fn notification_shown_maps_version_and_status() {
        let req = CaptureUiEventRequest::NotificationShown {
            version: "0.13.0-alpha.1".to_string(),
            delivery_status: UiNotificationDeliveryStatus::Sent,
            install_kind: UiInstallKind::Rpm,
        };
        match into_event(req) {
            Event::UpdateNotificationShown {
                version,
                delivery_status,
                install_kind,
            } => {
                assert_eq!(version, "0.13.0-alpha.1");
                assert_eq!(delivery_status, NotificationDeliveryStatus::Sent);
                assert_eq!(install_kind, InstallKind::Rpm);
            }
            other => panic!("expected UpdateNotificationShown, got {other:?}"),
        }
    }
}
