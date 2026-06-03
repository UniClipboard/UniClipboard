//! DTOs for the mobile-sync loopback endpoints (ADR-008 P3-b).
//!
//! Ported from the former GUI-only `mobile_sync` Tauri command DTOs when the
//! GUI moved onto the daemon HTTP API. `tag` literals + camelCase field names
//! are wire-identical to the previous tauri-specta DTOs so the frontend types
//! are unchanged. The domain → DTO conversions live in `uc-webserver` (this
//! contract crate does not depend on `uc-application`).
//!
//! The error wire form is the canonical [`ApiErrorResponse`](crate::api::dto::error::ApiErrorResponse):
//! `MobileSyncError`'s `{ code, ...fields }` shape is reconstructed by the FE
//! translator from `code` + `details`, so no error type is defined here.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

// ── Requests ────────────────────────────────────────────────────────────────

/// Request body for `POST /mobile-sync/devices`.
///
/// `username` / `password` absent (missing field or explicit null) routes
/// through the auto-mint path; a value is strictly validated.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RegisterMobileDeviceRequest {
    pub label: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
}

/// Request body for `POST /mobile-sync/devices/{device_id}/rotate-password`.
/// `password` absent → auto-mint a new plaintext; a value is validated.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RotateMobilePasswordRequest {
    #[serde(default)]
    pub password: Option<String>,
}

/// Request body (patch) for `PATCH /mobile-sync/settings`.
///
/// `lanAdvertiseIp` / `lanPort` are three-state: field absent = leave
/// untouched; explicit `null` = clear; value = set. The frontend's
/// `JSON.stringify` drops `undefined` (absent) and serializes `null`
/// explicitly. The `Option<Option<T>>` Rust type preserves the distinction;
/// the wire type is just `T | null` optional (declared via `schema(value_type)`).
#[derive(Debug, Clone, Default, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMobileSyncSettingsRequest {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub lan_listen_enabled: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_optional_optional_string")]
    #[schema(value_type = Option<String>)]
    pub lan_advertise_ip: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional_optional_u16")]
    #[schema(value_type = Option<u16>)]
    pub lan_port: Option<Option<u16>>,
}

// ── Responses ───────────────────────────────────────────────────────────────

/// Result of registering an iPhone Shortcut device. `password` is the **one
/// and only** plaintext echo to the frontend — afterwards it exists solely as
/// a PHC hash server-side. The two QR PNGs arrive base64-encoded (encoded
/// daemon-side) ready for `<img src="data:image/png;base64,...">`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RegisterMobileDeviceResultDto {
    pub device_id: String,
    pub label: String,
    pub client_type: String,
    pub created_at_ms: i64,
    pub base_url: String,
    pub username: String,
    pub password: String,
    pub install_url: String,
    /// Base64 PNG of the iCloud shortcut-install URL.
    pub install_qr_code_png_base64: String,
    /// `uniclipboard://connect?...` deep link (the main QR content).
    pub connect_uri: String,
    /// Base64 PNG encoding `connectUri`.
    pub qr_code_png_base64: String,
}

/// Result of rotating a device password. `password` is the one-time plaintext
/// echo; the old password is immediately invalidated.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RotateMobilePasswordResultDto {
    pub device_id: String,
    pub username: String,
    pub password: String,
}

/// Result of revoking a device. Enveloped `{ success: true }` so every 200
/// carries a `{ data, ts }` body per §0.1; the FE wrapper discards it.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MobileSyncActionResultDto {
    pub success: bool,
}

/// One registered device (no password hash; `username` is an identifier aid).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MobileDeviceViewDto {
    pub device_id: String,
    pub label: String,
    pub client_type: String,
    pub username: String,
    pub created_at_ms: i64,
    pub last_seen_at_ms: Option<i64>,
    pub last_seen_ip: Option<String>,
    pub reported_name: Option<String>,
    pub reported_os: Option<String>,
}

/// Synthesized mobile-sync settings view (settings + current LAN URL parts +
/// available install methods).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MobileSyncSettingsViewDto {
    pub enabled: bool,
    pub lan_listen_enabled: bool,
    pub lan_advertise_ip: Option<String>,
    pub lan_port: Option<u16>,
    /// Why the daemon's LAN listener failed to bind (port in use / IP absent /
    /// permission). `Some` means a bind was actually attempted and failed.
    pub lan_listener_error: Option<String>,
    pub shortcut_install_methods: Vec<ShortcutInstallMethodViewDto>,
}

/// One shortcut-install method option (`tokenInjected` / `icloudGeneric`).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutInstallMethodViewDto {
    pub method: String,
    pub available: bool,
    pub disabled_reason: Option<String>,
}

/// Result of updating mobile-sync settings.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMobileSyncSettingsResultDto {
    pub enabled: bool,
    pub lan_listen_enabled: bool,
    pub lan_advertise_ip: Option<String>,
    pub lan_port: Option<u16>,
    /// Wire-compat historical flag. In the GUI/daemon path settings take effect
    /// immediately so this is always false; the CLI fallback assembly still
    /// returns "any field actually changed → true" to express the old
    /// "next daemon restart" semantics. The frontend shows a restart banner
    /// only when true.
    pub restart_required: bool,
    /// Reason the LAN listener failed to bind under the immediate-apply path
    /// (port in use, permission, unassignable IP). `None` in the CLI fallback /
    /// no-lifecycle assembly.
    pub lan_listener_bind_error: Option<String>,
}

/// One usable IPv4 LAN interface candidate for the QR URL.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LanInterfaceViewDto {
    pub name: String,
    pub ipv4: String,
}

// ── Three-state `Option<Option<T>>` deserializers ───────────────────────────
//
// serde collapses `null` and a missing field into the outer `None` for
// `Option<Option<T>>`, losing "explicit clear". The standard trick: parse the
// inner `Option` (null → None, value → Some), then wrap in `Some`. Combined
// with `#[serde(default)]` on the field the three states line up:
// - field absent → default → outer None
// - explicit null → Some(None)
// - value → Some(Some(value))

fn deserialize_optional_optional_string<'de, D>(
    deserializer: D,
) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<String>::deserialize(deserializer)?))
}

fn deserialize_optional_optional_u16<'de, D>(
    deserializer: D,
) -> Result<Option<Option<u16>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<u16>::deserialize(deserializer)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_request_three_state_lan_advertise_ip_absent() {
        let args: UpdateMobileSyncSettingsRequest = serde_json::from_str("{}").unwrap();
        assert!(args.lan_advertise_ip.is_none());
    }

    #[test]
    fn update_request_three_state_lan_advertise_ip_explicit_null() {
        let args: UpdateMobileSyncSettingsRequest =
            serde_json::from_str(r#"{"lanAdvertiseIp": null}"#).unwrap();
        assert_eq!(args.lan_advertise_ip, Some(None));
    }

    #[test]
    fn update_request_three_state_lan_advertise_ip_with_value() {
        let args: UpdateMobileSyncSettingsRequest =
            serde_json::from_str(r#"{"lanAdvertiseIp": "192.168.1.5"}"#).unwrap();
        assert_eq!(args.lan_advertise_ip, Some(Some("192.168.1.5".to_string())));
    }

    #[test]
    fn update_request_three_state_lan_port_explicit_null() {
        let args: UpdateMobileSyncSettingsRequest =
            serde_json::from_str(r#"{"lanPort": null}"#).unwrap();
        assert_eq!(args.lan_port, Some(None));
    }

    #[test]
    fn register_request_username_password_optional() {
        let args: RegisterMobileDeviceRequest =
            serde_json::from_str(r#"{"label": "iPhone"}"#).unwrap();
        assert_eq!(args.label, "iPhone");
        assert!(args.username.is_none());
        assert!(args.password.is_none());
    }
}
