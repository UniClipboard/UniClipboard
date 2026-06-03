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
    UiInstallKind, UiUpdateAction, UiUpdateActionOutcome, UiUpdatePhase,
};
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use uc_observability::analytics::{
    DialogOpenSource, DismissSource, Event, InstallKind, UpdateAction, UpdateActionOutcome,
    UpdatePhase,
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
    }
}

/// Short, body-free tag for tracing — never logs event fields.
fn event_kind_tag(req: &CaptureUiEventRequest) -> &'static str {
    match req {
        CaptureUiEventRequest::DialogOpened { .. } => "dialog_opened",
        CaptureUiEventRequest::Dismissed { .. } => "dismissed",
        CaptureUiEventRequest::ActionInvoked { .. } => "action_invoked",
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
    }
}
