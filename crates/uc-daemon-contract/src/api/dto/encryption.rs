use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProfileRecoveryStateDto {
    NotRequired,
    AwaitingPassphrase,
    Recovering,
    Recovered,
    PartiallyRecoverable,
    Failed,
    AdmissionRecoveryRequired,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionRecoveryCategoryDto {
    CredentialMissing,
    AuthenticationMismatch,
    CurrentMetadataInvalid,
    LegacyFallbackInvalid,
    LegacyMigrationFailed,
    RecordRelationIncomplete,
    DerivedSummaryInvalid,
    GenerationMismatch,
    OtherStorageError,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionRecoveryStageDto {
    Credential,
    RepositoryMetadata,
    LegacyRepository,
    RepositoryRecord,
    RecoverySummary,
    Storage,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionRecoveryActionDto {
    RestoreCredential,
    ChooseBackup,
    RebuildDerivedState,
    ExportDiagnostics,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdmissionRecoveryDto {
    pub category: AdmissionRecoveryCategoryDto,
    pub stage: AdmissionRecoveryStageDto,
    pub action: AdmissionRecoveryActionDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProfileRecoveryLossDto {
    LocalHistory,
    LocalControlState,
    DeviceIdentity,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProfileRecoveryResponse {
    pub state: ProfileRecoveryStateDto,
    pub can_submit_passphrase: bool,
    pub restart_required: bool,
    pub background_ready: bool,
    pub cleanup_pending: bool,
    pub losses: Vec<ProfileRecoveryLossDto>,
    pub admission: Option<AdmissionRecoveryDto>,
}

/// Response payload for GET /encryption/state.
///
/// `Deserialize` is required by the Rust daemon-client (`DaemonQueryClient`),
/// which polls this endpoint during cold-launch startup restore to wait until
/// the encryption session is unlocked before reading/writing encrypted
/// history (issue #1169).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EncryptionStateResponse {
    pub initialized: bool,
    pub session_ready: bool,
}

/// Internal event payload for the encryption.session_ready WS event.
/// Serialized as part of DaemonWsEvent payload.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EncryptionSessionReadyPayload {
    pub ts: i64,
}

/// Response payload for GET /encryption/keychain-access.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KeychainAccessResponse {
    /// Whether Keychain access is granted (Always Allow permission).
    pub granted: bool,
}

/// Shared response payload for `POST /encryption/unlock` and
/// `POST /encryption/lock`.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EncryptionActionResponse {
    pub success: bool,
}

/// Request body for `POST /encryption/unlock-with-passphrase` (ADR-008 D15).
///
/// Carries the user's plaintext passphrase over the loopback API. Per D14 the
/// endpoint is session-JWT gated (not in `PUBLIC_PATHS`) and the handler MUST
/// never log this body — see the rule in `uc-webserver` `api/encryption.rs`.
/// This formally retires the historical "passphrase 不出进程" invariant: under
/// the "same UID = trusted" model (D14) an attacker who can sniff loopback can
/// already dump the master key from daemon memory, so loopback transport adds
/// zero incremental exposure.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UnlockSpaceRequest {
    pub passphrase: String,
}

/// Response payload for `POST /encryption/unlock-with-passphrase`.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UnlockSpaceResponse {
    pub space_id: String,
}

/// Request body for `POST /encryption/passphrase`.
///
/// Both fields contain plaintext secrets and must never be logged.
#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChangeEncryptionPassphraseRequest {
    pub passphrase: String,
    pub passphrase_confirmation: String,
}

#[cfg(test)]
mod tests {
    use super::{
        AdmissionRecoveryActionDto, AdmissionRecoveryCategoryDto, AdmissionRecoveryDto,
        AdmissionRecoveryStageDto, ChangeEncryptionPassphraseRequest, ProfileRecoveryResponse,
        ProfileRecoveryStateDto,
    };

    #[test]
    fn change_passphrase_request_uses_camel_case_wire_fields() {
        let request = ChangeEncryptionPassphraseRequest {
            passphrase: "new secret".to_string(),
            passphrase_confirmation: "new secret".to_string(),
        };

        let value = serde_json::to_value(request).expect("request serializes");

        assert_eq!(value["passphrase"], "new secret");
        assert_eq!(value["passphraseConfirmation"], "new secret");
        assert!(value.get("passphrase_confirmation").is_none());
    }

    #[test]
    fn admission_recovery_guidance_survives_the_wire_contract() {
        let response = ProfileRecoveryResponse {
            state: ProfileRecoveryStateDto::AdmissionRecoveryRequired,
            can_submit_passphrase: false,
            restart_required: false,
            background_ready: false,
            cleanup_pending: false,
            losses: Vec::new(),
            admission: Some(AdmissionRecoveryDto {
                category: AdmissionRecoveryCategoryDto::LegacyFallbackInvalid,
                stage: AdmissionRecoveryStageDto::LegacyRepository,
                action: AdmissionRecoveryActionDto::ChooseBackup,
            }),
        };

        let value = serde_json::to_value(response).expect("response serializes");
        assert_eq!(value["state"], "admission_recovery_required");
        assert_eq!(value["admission"]["category"], "legacy_fallback_invalid");
        assert_eq!(value["admission"]["stage"], "legacy_repository");
        assert_eq!(value["admission"]["action"], "choose_backup");
    }
}
