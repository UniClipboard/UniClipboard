//! DTOs for `POST /analytics/capture` (ADR-008 D20).
//!
//! The GUI webview no longer captures product-analytics events through an
//! in-process sink. Under D2's process split the daemon is the **single
//! authoritative analytics sender** — two processes each emitting device-level
//! signals would double-count PostHog DAU / device counts (§5.3 #9). The
//! webview therefore POSTs its UI-interaction events here and the daemon
//! dispatches them through its own gated sink + `EventContext`.
//!
//! ## Mirror enums
//!
//! These enums mirror the wire form of `uc_observability::analytics::*` but are
//! redeclared here so the contract crate keeps zero dependency on
//! `uc-observability`. The webserver owns the contract→analytics mapping (free
//! functions in `api/analytics.rs`, per the orphan rule). Wire equivalence is
//! locked by a unit test there. Evolution policy (telemetry schema doc §8):
//! adding a variant is allowed (sync both sides), renaming is forbidden.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Mirrors `analytics::DialogOpenSource`. wire: `notification` | `sidebar_icon`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiDialogOpenSource {
    Notification,
    SidebarIcon,
}

/// Mirrors `analytics::UpdatePhase`. wire: `available` | `downloading` | `ready`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiUpdatePhase {
    Available,
    Downloading,
    Ready,
}

/// Mirrors `analytics::DismissSource`. wire: `dialog_later` | `dialog_closed` |
/// `package_manager_dialog_closed`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiDismissSource {
    DialogLater,
    DialogClosed,
    PackageManagerDialogClosed,
}

/// Mirrors `analytics::UpdateAction`. wire: `download_bg` | `install`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiUpdateAction {
    DownloadBg,
    Install,
}

/// Mirrors `analytics::UpdateActionOutcome`. wire: `started` | `succeeded` |
/// `failed` | `cancelled`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiUpdateActionOutcome {
    Started,
    Succeeded,
    Failed,
    Cancelled,
}

/// Mirrors `analytics::InstallKind`. wire (lowercase): `macos` | `windows` |
/// `windowsportable` | `appimage` | `deb` | `rpm` | `unknown`.
///
/// Cross-process note (ADR-008 D20): `install_kind` was historically probed
/// backend-side and the webview "knew nothing". After the process split the
/// install provenance of the *running app* is owned by the native GUI shell
/// (it holds `current_exe` / the portable marker / the `APPIMAGE` env), while
/// the daemon has no install-detection code. So the webview now supplies it —
/// it reads `get_install_kind` (native, cached) and forwards the result here.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum UiInstallKind {
    Macos,
    Windows,
    WindowsPortable,
    AppImage,
    Deb,
    Rpm,
    Unknown,
}

/// Mirrors `analytics::UpdateCheckSource`. wire: `startup` | `scheduled` |
/// `manual` | `window_show`.
///
/// Cross-process note (ADR-008 D20): the update *check* runs in the GUI process
/// (its updater background task / tray / settings button), not the daemon. The
/// GUI therefore forwards the check outcome here so the daemon — the single
/// authoritative sender — dispatches it with its own `EventContext`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiUpdateCheckSource {
    Startup,
    Scheduled,
    Manual,
    WindowShow,
}

/// Mirrors `analytics::UpdateCheckOutcome`. wire: `available` | `up_to_date` |
/// `failed`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiUpdateCheckOutcome {
    Available,
    UpToDate,
    Failed,
}

/// Mirrors `analytics::UpdateFailureKind`. wire: `network` | `http_error` |
/// `parse_error` | `other`. Only present when the check outcome is `failed`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiUpdateFailureKind {
    Network,
    HttpError,
    ParseError,
    Other,
}

/// Mirrors `analytics::NotificationDeliveryStatus`. wire: `sent` |
/// `permission_denied` | `send_failed`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiNotificationDeliveryStatus {
    Sent,
    PermissionDenied,
    SendFailed,
}

/// Tagged union of GUI-originated UI analytics events (discriminated by `kind`).
///
/// Wire-compatible with the retired `capture_update_ui_event` Tauri command's
/// `UpdateUiEvent` (same discriminators / field names), except `DialogOpened`
/// now carries `install_kind` explicitly (see [`UiInstallKind`]).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CaptureUiEventRequest {
    /// User opened `UpdateDialog` / `PackageManagerUpdateDialog`.
    DialogOpened {
        source: UiDialogOpenSource,
        phase: UiUpdatePhase,
        install_kind: UiInstallKind,
    },
    /// User dismissed the dialog (later / closed / cancelled).
    Dismissed {
        phase: UiUpdatePhase,
        source: UiDismissSource,
    },
    /// A pure-UI action path (e.g. `Cancelled`). `error_kind` must be a short
    /// identifier (< 32 chars, e.g. `user_cancelled`) and MUST NOT contain
    /// paths / URLs / IPs (telemetry schema doc §6.1).
    ActionInvoked {
        action: UiUpdateAction,
        outcome: UiUpdateActionOutcome,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error_kind: Option<String>,
    },
    /// An update check completed (any source). Emitted by the GUI updater
    /// background task / tray / settings button — the daemon has no
    /// update-check code, so the GUI forwards the outcome here. `failure_kind`
    /// is present only when `outcome` is `failed` and disappears from the wire
    /// otherwise (no `null`).
    CheckPerformed {
        source: UiUpdateCheckSource,
        outcome: UiUpdateCheckOutcome,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        failure_kind: Option<UiUpdateFailureKind>,
        install_kind: UiInstallKind,
    },
    /// An update prompt was delivered to the user (already same-version
    /// deduplicated). Emitted by the GUI `update_scheduler` after opening the
    /// Sparkle-style updater window. `version` is the raw manifest version
    /// string (low cardinality — one new version per channel at a time).
    NotificationShown {
        version: String,
        delivery_status: UiNotificationDeliveryStatus,
        install_kind: UiInstallKind,
    },
}

/// Response for `POST /analytics/capture`. `capture` is fire-and-forget, so
/// `accepted` only confirms the daemon decoded the event and handed it to the
/// sink — not that it reached PostHog.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CaptureUiEventResponse {
    pub accepted: bool,
}
