use std::fmt;

use serde::{Deserialize, Serialize};

use crate::SecretString;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MobileClientTypeSummary {
    IosShortcut,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MobileDeviceSummary {
    pub device_id: String,
    pub label: String,
    pub client_type: MobileClientTypeSummary,
    pub username: String,
    pub created_at_ms: i64,
    pub last_seen_at_ms: Option<i64>,
    pub last_seen_ip: Option<String>,
    pub reported_name: Option<String>,
    pub reported_os: Option<String>,
}

impl fmt::Debug for MobileDeviceSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MobileDeviceSummary")
            .field("client_type", &self.client_type)
            .field("created_at_ms", &self.created_at_ms)
            .field("last_seen_at_ms", &self.last_seen_at_ms)
            .field("has_last_seen_ip", &self.last_seen_ip.is_some())
            .field("has_reported_name", &self.reported_name.is_some())
            .field("has_reported_os", &self.reported_os.is_some())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct MobileDeviceInput {
    pub device_id: String,
}

impl fmt::Debug for MobileDeviceInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MobileDeviceInput([REDACTED])")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MobileDeviceRevokeOutcome {
    Revoked,
    NotFound,
}

#[derive(Clone, PartialEq, Eq)]
pub struct AuthenticateMobileRequestInput {
    pub authorization: SecretString,
}

impl fmt::Debug for AuthenticateMobileRequestInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuthenticateMobileRequestInput([REDACTED])")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct MobileCredential {
    device_id: String,
    password_proof: SecretString,
}

impl MobileCredential {
    pub fn new(device_id: impl Into<String>, password_proof: impl Into<String>) -> Self {
        let password_proof: String = password_proof.into();
        Self {
            device_id: device_id.into(),
            password_proof: SecretString::new(password_proof),
        }
    }

    pub(crate) fn device_id(&self) -> &str {
        &self.device_id
    }

    pub(crate) fn password_proof(&self) -> &str {
        self.password_proof.expose()
    }
}

impl fmt::Debug for MobileCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MobileCredential([REDACTED])")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct MobileAuthenticatedSession {
    pub device_id: String,
    pub client_type: MobileClientTypeSummary,
    pub credential: MobileCredential,
}

impl fmt::Debug for MobileAuthenticatedSession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MobileAuthenticatedSession")
            .field("client_type", &self.client_type)
            .field("credential", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RevalidateMobileCredentialInput {
    pub credential: MobileCredential,
}

impl fmt::Debug for RevalidateMobileCredentialInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RevalidateMobileCredentialInput([REDACTED])")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MobileAuthenticationOutcome {
    Rejected,
}

#[derive(Clone, PartialEq, Eq)]
pub enum MobileLanEndpointUpdate {
    Stopped,
    Listening { base_url: String },
    BindFailed { reason: String },
}

impl fmt::Debug for MobileLanEndpointUpdate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stopped => formatter.write_str("MobileLanEndpointUpdate::Stopped"),
            Self::Listening { .. } => {
                formatter.write_str("MobileLanEndpointUpdate::Listening([REDACTED])")
            }
            Self::BindFailed { .. } => {
                formatter.write_str("MobileLanEndpointUpdate::BindFailed([REDACTED])")
            }
        }
    }
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MobileSyncSettingsPatch {
    pub enabled: Option<bool>,
    pub lan_listen_enabled: Option<bool>,
    pub lan_advertise_ip: Option<Option<String>>,
    pub lan_advertise_base_url: Option<Option<String>>,
    pub lan_port: Option<Option<u16>>,
}

impl fmt::Debug for MobileSyncSettingsPatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MobileSyncSettingsPatch")
            .field("enabled_changed", &self.enabled.is_some())
            .field(
                "lan_listen_enabled_changed",
                &self.lan_listen_enabled.is_some(),
            )
            .field("lan_advertise_ip_changed", &self.lan_advertise_ip.is_some())
            .field(
                "lan_advertise_base_url_changed",
                &self.lan_advertise_base_url.is_some(),
            )
            .field("lan_port_changed", &self.lan_port.is_some())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MobileShortcutInstallMethod {
    TokenInjected,
    IcloudGeneric,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MobileShortcutInstallMethodSummary {
    pub method: MobileShortcutInstallMethod,
    pub available: bool,
    pub disabled_reason: Option<String>,
}

impl fmt::Debug for MobileShortcutInstallMethodSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MobileShortcutInstallMethodSummary")
            .field("method", &self.method)
            .field("available", &self.available)
            .field("has_disabled_reason", &self.disabled_reason.is_some())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MobileSyncSettingsSummary {
    pub enabled: bool,
    pub lan_listen_enabled: bool,
    pub lan_advertise_ip: Option<String>,
    pub lan_advertise_base_url: Option<String>,
    pub lan_port: Option<u16>,
    pub lan_listener_error: Option<String>,
    pub shortcut_install_methods: Vec<MobileShortcutInstallMethodSummary>,
}

impl fmt::Debug for MobileSyncSettingsSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MobileSyncSettingsSummary")
            .field("enabled", &self.enabled)
            .field("lan_listen_enabled", &self.lan_listen_enabled)
            .field("has_lan_advertise_ip", &self.lan_advertise_ip.is_some())
            .field(
                "has_lan_advertise_base_url",
                &self.lan_advertise_base_url.is_some(),
            )
            .field("lan_port", &self.lan_port)
            .field("has_lan_listener_error", &self.lan_listener_error.is_some())
            .field(
                "shortcut_install_method_count",
                &self.shortcut_install_methods.len(),
            )
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MobileSyncSettingsUpdateSummary {
    pub enabled: bool,
    pub lan_listen_enabled: bool,
    pub lan_advertise_ip: Option<String>,
    pub lan_advertise_base_url: Option<String>,
    pub lan_port: Option<u16>,
    pub changed: bool,
}

impl fmt::Debug for MobileSyncSettingsUpdateSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MobileSyncSettingsUpdateSummary")
            .field("enabled", &self.enabled)
            .field("lan_listen_enabled", &self.lan_listen_enabled)
            .field("has_lan_advertise_ip", &self.lan_advertise_ip.is_some())
            .field(
                "has_lan_advertise_base_url",
                &self.lan_advertise_base_url.is_some(),
            )
            .field("lan_port", &self.lan_port)
            .field("changed", &self.changed)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MobileSyncSettingsUpdateOutcome {
    Updated(Box<MobileSyncSettingsUpdateSummary>),
    Rejected { reason: String },
}

impl fmt::Debug for MobileSyncSettingsUpdateOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Updated(summary) => formatter
                .debug_tuple("MobileSyncSettingsUpdateOutcome::Updated")
                .field(summary)
                .finish(),
            Self::Rejected { .. } => {
                formatter.write_str("MobileSyncSettingsUpdateOutcome::Rejected([REDACTED])")
            }
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RegisterMobileDeviceInput {
    pub label: String,
    pub username: Option<String>,
    pub password: Option<SecretString>,
}

impl fmt::Debug for RegisterMobileDeviceInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RegisterMobileDeviceInput")
            .field("has_username", &self.username.is_some())
            .field("has_password", &self.password.is_some())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct MobileDeviceRegistration {
    pub device_id: String,
    pub label: String,
    pub client_type: MobileClientTypeSummary,
    pub created_at_ms: i64,
    pub base_url: String,
    pub username: String,
    pub password: SecretString,
    pub install_url: String,
    pub install_qr_code_png_bytes: Vec<u8>,
    pub connect_uri: String,
    pub qr_code_png_bytes: Vec<u8>,
    pub qr_code_ascii: String,
}

impl fmt::Debug for MobileDeviceRegistration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MobileDeviceRegistration")
            .field("client_type", &self.client_type)
            .field("created_at_ms", &self.created_at_ms)
            .field("install_qr_bytes", &self.install_qr_code_png_bytes.len())
            .field("connect_qr_bytes", &self.qr_code_png_bytes.len())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum MobileDeviceRegistrationOutcome {
    Registered(Box<MobileDeviceRegistration>),
    LabelEmpty,
    LabelTooLong,
    LanListenerDisabled,
    UsernameTaken { username: String },
    UsernameTooShort { min: usize, got: usize },
    UsernameTooLong { max: usize, got: usize },
    UsernameMustStartWithLetter,
    UsernameContainsForbiddenChars,
    PasswordTooShort { min: usize },
    PasswordTooLong { max: usize },
    NoLanInterfaceAvailable,
}

impl fmt::Debug for MobileDeviceRegistrationOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self {
            Self::Registered(_) => "registered",
            Self::LabelEmpty => "label_empty",
            Self::LabelTooLong => "label_too_long",
            Self::LanListenerDisabled => "lan_listener_disabled",
            Self::UsernameTaken { .. } => "username_taken",
            Self::UsernameTooShort { .. } => "username_too_short",
            Self::UsernameTooLong { .. } => "username_too_long",
            Self::UsernameMustStartWithLetter => "username_must_start_with_letter",
            Self::UsernameContainsForbiddenChars => "username_contains_forbidden_chars",
            Self::PasswordTooShort { .. } => "password_too_short",
            Self::PasswordTooLong { .. } => "password_too_long",
            Self::NoLanInterfaceAvailable => "no_lan_interface_available",
        };
        formatter
            .debug_struct("MobileDeviceRegistrationOutcome")
            .field("kind", &kind)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum MobilePasswordUpdate {
    Keep,
    AutoGenerate,
    Custom(SecretString),
}

impl fmt::Debug for MobilePasswordUpdate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Keep => formatter.write_str("MobilePasswordUpdate::Keep"),
            Self::AutoGenerate => formatter.write_str("MobilePasswordUpdate::AutoGenerate"),
            Self::Custom(_) => formatter.write_str("MobilePasswordUpdate::Custom([REDACTED])"),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct UpdateMobileDeviceInput {
    pub device_id: String,
    pub label: Option<String>,
    pub username: Option<String>,
    pub password: MobilePasswordUpdate,
}

impl fmt::Debug for UpdateMobileDeviceInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UpdateMobileDeviceInput")
            .field("label_changed", &self.label.is_some())
            .field("username_changed", &self.username.is_some())
            .field("password", &self.password)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct MobileDeviceUpdate {
    pub device_id: String,
    pub label: String,
    pub username: String,
    pub password: Option<SecretString>,
}

impl fmt::Debug for MobileDeviceUpdate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MobileDeviceUpdate")
            .field("has_password", &self.password.is_some())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum MobileDeviceUpdateOutcome {
    Updated(Box<MobileDeviceUpdate>),
    NotFound,
    LabelEmpty,
    LabelTooLong,
    UsernameTaken { username: String },
    UsernameTooShort { min: usize, got: usize },
    UsernameTooLong { max: usize, got: usize },
    UsernameMustStartWithLetter,
    UsernameContainsForbiddenChars,
    PasswordTooShort { min: usize },
    PasswordTooLong { max: usize },
}

impl fmt::Debug for MobileDeviceUpdateOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self {
            Self::Updated(_) => "updated",
            Self::NotFound => "not_found",
            Self::LabelEmpty => "label_empty",
            Self::LabelTooLong => "label_too_long",
            Self::UsernameTaken { .. } => "username_taken",
            Self::UsernameTooShort { .. } => "username_too_short",
            Self::UsernameTooLong { .. } => "username_too_long",
            Self::UsernameMustStartWithLetter => "username_must_start_with_letter",
            Self::UsernameContainsForbiddenChars => "username_contains_forbidden_chars",
            Self::PasswordTooShort { .. } => "password_too_short",
            Self::PasswordTooLong { .. } => "password_too_long",
        };
        formatter
            .debug_struct("MobileDeviceUpdateOutcome")
            .field("kind", &kind)
            .finish()
    }
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MobileLanInterfaceSummary {
    pub name: String,
    pub ipv4: String,
}

impl fmt::Debug for MobileLanInterfaceSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MobileLanInterfaceSummary")
            .field("has_name", &!self.name.is_empty())
            .field("has_ipv4", &!self.ipv4.is_empty())
            .finish()
    }
}
