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
