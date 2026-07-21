use uc_application::facade::mobile_sync::{
    AuthenticateBasicAuthError, AuthenticateBasicAuthInput, GetMobileSyncSettingsError,
    IsDeviceCredentialCurrentError, ListMobileDevicesError, MobileDevicePasswordEdit,
    MobileSyncFacade, MobileSyncSettingsView, RegisterMobileShortcutDeviceError,
    RegisterMobileShortcutDeviceInput, RevokeMobileDeviceError, RevokeMobileDeviceInput,
    ShortcutInstallMethod, ShortcutInstallMethodOption, UpdateMobileDeviceError,
    UpdateMobileDeviceInput as ApplicationUpdateMobileDeviceInput, UpdateMobileSyncSettingsError,
    UpdateMobileSyncSettingsInput as ApplicationUpdateMobileSyncSettingsInput,
    UpdateMobileSyncSettingsOutput,
};
use uc_core::mobile_sync::{LanEndpointInfo, MobileClientType, MobileDeviceId};
use uc_infra::mobile_sync::InMemoryMobileSyncEndpointInfoAdapter;

use crate::{
    AuthenticateMobileRequestInput, EngineError, EngineErrorCategory, MobileAuthenticatedSession,
    MobileAuthenticationOutcome, MobileClientTypeSummary, MobileCredential, MobileDeviceInput,
    MobileDeviceRegistration, MobileDeviceRegistrationOutcome, MobileDeviceRevokeOutcome,
    MobileDeviceSummary, MobileDeviceUpdate, MobileDeviceUpdateOutcome, MobileLanEndpointUpdate,
    MobilePasswordUpdate, MobileShortcutInstallMethod, MobileShortcutInstallMethodSummary,
    MobileSyncSettingsPatch, MobileSyncSettingsSummary, MobileSyncSettingsUpdateOutcome,
    MobileSyncSettingsUpdateSummary, OperationResult, RegisterMobileDeviceInput,
    RevalidateMobileCredentialInput, UpdateMobileDeviceInput,
};

pub const LIST_MOBILE_DEVICES_FAILED_CODE: u32 = 1431;
pub const REVOKE_MOBILE_DEVICE_FAILED_CODE: u32 = 1432;
pub const AUTHENTICATE_MOBILE_REQUEST_FAILED_CODE: u32 = 1433;
pub const REVALIDATE_MOBILE_CREDENTIAL_FAILED_CODE: u32 = 1434;
pub const REGISTER_MOBILE_DEVICE_FAILED_CODE: u32 = 1435;
pub const UPDATE_MOBILE_DEVICE_FAILED_CODE: u32 = 1436;
pub const QUERY_MOBILE_SYNC_SETTINGS_FAILED_CODE: u32 = 1437;
pub const UPDATE_MOBILE_SYNC_SETTINGS_FAILED_CODE: u32 = 1438;

pub(crate) async fn execute_query_mobile_sync_settings(
    facade: &MobileSyncFacade,
) -> Result<OperationResult, EngineError> {
    facade
        .get_settings()
        .await
        .map(|settings| OperationResult::MobileSyncSettings(Box::new(map_settings(settings))))
        .map_err(|error| match error {
            GetMobileSyncSettingsError::SettingsLoadFailed(_)
            | GetMobileSyncSettingsError::EndpointInfoFailed(_) => {
                internal_error(QUERY_MOBILE_SYNC_SETTINGS_FAILED_CODE)
            }
        })
}

pub(crate) async fn execute_update_mobile_sync_settings(
    facade: &MobileSyncFacade,
    patch: MobileSyncSettingsPatch,
) -> Result<OperationResult, EngineError> {
    match facade
        .update_settings(ApplicationUpdateMobileSyncSettingsInput {
            enabled: patch.enabled,
            lan_listen_enabled: patch.lan_listen_enabled,
            lan_advertise_ip: patch.lan_advertise_ip,
            lan_advertise_base_url: patch.lan_advertise_base_url,
            lan_port: patch.lan_port,
        })
        .await
    {
        Ok(settings) => Ok(OperationResult::MobileSyncSettingsUpdated(
            MobileSyncSettingsUpdateOutcome::Updated(Box::new(map_settings_update(settings))),
        )),
        Err(UpdateMobileSyncSettingsError::InvalidLanParameter(reason)) => {
            Ok(OperationResult::MobileSyncSettingsUpdated(
                MobileSyncSettingsUpdateOutcome::Rejected { reason },
            ))
        }
        Err(
            UpdateMobileSyncSettingsError::SettingsLoadFailed(_)
            | UpdateMobileSyncSettingsError::SettingsSaveFailed(_),
        ) => Err(internal_error(UPDATE_MOBILE_SYNC_SETTINGS_FAILED_CODE)),
    }
}

pub(crate) async fn execute_update_mobile_lan_endpoint(
    endpoint: &InMemoryMobileSyncEndpointInfoAdapter,
    update: MobileLanEndpointUpdate,
) -> Result<OperationResult, EngineError> {
    match update {
        MobileLanEndpointUpdate::Stopped => endpoint.clear().await,
        MobileLanEndpointUpdate::Listening { base_url } => {
            endpoint.set(LanEndpointInfo { url: base_url }).await;
        }
        MobileLanEndpointUpdate::BindFailed { reason } => {
            endpoint.set_bind_failure(reason).await;
        }
    }
    Ok(OperationResult::MobileLanEndpointUpdated)
}

pub(crate) async fn execute_register_mobile_device(
    facade: &MobileSyncFacade,
    input: RegisterMobileDeviceInput,
) -> Result<OperationResult, EngineError> {
    match facade
        .register_device(RegisterMobileShortcutDeviceInput {
            label: input.label,
            username: input.username,
            password: input.password.map(|password| password.expose().to_string()),
        })
        .await
    {
        Ok(output) => Ok(OperationResult::MobileDeviceRegistered(
            MobileDeviceRegistrationOutcome::Registered(Box::new(MobileDeviceRegistration {
                device_id: output.device.device_id.into_string(),
                label: output.device.label,
                client_type: map_client_type(output.device.client_type),
                base_url: output.base_url,
                username: output.username,
                password: crate::SecretString::new(output.password),
                install_url: output.install_url,
                install_qr_code_png_bytes: output.install_qr_code_png_bytes,
                connect_uri: output.connect_uri,
                qr_code_png_bytes: output.qr_code_png_bytes,
                qr_code_ascii: output.qr_code_ascii,
            })),
        )),
        Err(error) => map_registration_error(error),
    }
}

pub(crate) async fn execute_update_mobile_device(
    facade: &MobileSyncFacade,
    input: UpdateMobileDeviceInput,
) -> Result<OperationResult, EngineError> {
    let password = match input.password {
        MobilePasswordUpdate::Keep => MobileDevicePasswordEdit::Keep,
        MobilePasswordUpdate::AutoGenerate => MobileDevicePasswordEdit::AutoGenerate,
        MobilePasswordUpdate::Custom(password) => {
            MobileDevicePasswordEdit::Custom(password.expose().to_string())
        }
    };
    match facade
        .update_device(ApplicationUpdateMobileDeviceInput {
            device_id: MobileDeviceId::new(input.device_id),
            label: input.label,
            username: input.username,
            password,
        })
        .await
    {
        Ok(output) => Ok(OperationResult::MobileDeviceUpdated(
            MobileDeviceUpdateOutcome::Updated(Box::new(MobileDeviceUpdate {
                device_id: output.device_id.into_string(),
                label: output.label,
                username: output.username,
                password: output.password.map(crate::SecretString::new),
            })),
        )),
        Err(error) => map_update_error(error),
    }
}

pub(crate) async fn execute_list_mobile_devices(
    facade: &MobileSyncFacade,
) -> Result<OperationResult, EngineError> {
    facade
        .list_devices()
        .await
        .map(|devices| {
            OperationResult::MobileDevices(
                devices
                    .into_iter()
                    .map(|device| MobileDeviceSummary {
                        device_id: device.device_id.into_string(),
                        label: device.label,
                        client_type: map_client_type(device.client_type),
                        username: device.username,
                        created_at_ms: device.created_at_ms,
                        last_seen_at_ms: device.last_seen_at_ms,
                        last_seen_ip: device.last_seen_ip,
                        reported_name: device.reported_name,
                        reported_os: device.reported_os,
                    })
                    .collect(),
            )
        })
        .map_err(|ListMobileDevicesError::PersistenceFailed(_)| {
            internal_error(LIST_MOBILE_DEVICES_FAILED_CODE)
        })
}

pub(crate) async fn execute_revoke_mobile_device(
    facade: &MobileSyncFacade,
    input: MobileDeviceInput,
) -> Result<OperationResult, EngineError> {
    match facade
        .revoke_device(RevokeMobileDeviceInput {
            device_id: MobileDeviceId::new(input.device_id),
        })
        .await
    {
        Ok(()) => Ok(OperationResult::MobileDeviceRevoked(
            MobileDeviceRevokeOutcome::Revoked,
        )),
        Err(RevokeMobileDeviceError::NotFound(_)) => Ok(OperationResult::MobileDeviceRevoked(
            MobileDeviceRevokeOutcome::NotFound,
        )),
        Err(RevokeMobileDeviceError::PersistenceFailed(_)) => {
            Err(internal_error(REVOKE_MOBILE_DEVICE_FAILED_CODE))
        }
    }
}

pub(crate) async fn execute_authenticate_mobile_request(
    facade: &MobileSyncFacade,
    input: AuthenticateMobileRequestInput,
) -> Result<OperationResult, EngineError> {
    match facade
        .authenticate_basic(AuthenticateBasicAuthInput {
            authorization_header: input.authorization.expose().to_string(),
        })
        .await
    {
        Ok(authenticated) => {
            let device = authenticated.device;
            let device_id = device.device_id.into_string();
            Ok(OperationResult::MobileRequestAuthenticated(
                MobileAuthenticatedSession {
                    credential: MobileCredential::new(&device_id, device.password_hash),
                    device_id,
                    client_type: map_client_type(device.client_type),
                },
            ))
        }
        Err(AuthenticateBasicAuthError::InvalidCredentials) => Ok(
            OperationResult::MobileAuthentication(MobileAuthenticationOutcome::Rejected),
        ),
        Err(
            AuthenticateBasicAuthError::PersistenceFailed(_)
            | AuthenticateBasicAuthError::Internal(_),
        ) => Err(internal_error(AUTHENTICATE_MOBILE_REQUEST_FAILED_CODE)),
    }
}

pub(crate) async fn execute_revalidate_mobile_credential(
    facade: &MobileSyncFacade,
    input: RevalidateMobileCredentialInput,
) -> Result<OperationResult, EngineError> {
    facade
        .is_device_credential_current(
            &MobileDeviceId::new(input.credential.device_id()),
            input.credential.password_proof(),
        )
        .await
        .map(|current| OperationResult::MobileCredentialCurrent { current })
        .map_err(|IsDeviceCredentialCurrentError::Repository(_)| {
            internal_error(REVALIDATE_MOBILE_CREDENTIAL_FAILED_CODE)
        })
}

fn map_client_type(client_type: MobileClientType) -> MobileClientTypeSummary {
    match client_type {
        MobileClientType::IosShortcut => MobileClientTypeSummary::IosShortcut,
    }
}

fn map_settings(settings: MobileSyncSettingsView) -> MobileSyncSettingsSummary {
    MobileSyncSettingsSummary {
        enabled: settings.enabled,
        lan_listen_enabled: settings.lan_listen_enabled,
        lan_advertise_ip: settings.lan_advertise_ip,
        lan_advertise_base_url: settings.lan_advertise_base_url,
        lan_port: settings.lan_port,
        lan_listener_error: settings.lan_listener_error,
        shortcut_install_methods: settings
            .shortcut_install_methods
            .into_iter()
            .map(map_install_method)
            .collect(),
    }
}

fn map_install_method(option: ShortcutInstallMethodOption) -> MobileShortcutInstallMethodSummary {
    MobileShortcutInstallMethodSummary {
        method: match option.method {
            ShortcutInstallMethod::TokenInjected => MobileShortcutInstallMethod::TokenInjected,
            ShortcutInstallMethod::IcloudGeneric => MobileShortcutInstallMethod::IcloudGeneric,
        },
        available: option.available,
        disabled_reason: option.disabled_reason,
    }
}

fn map_settings_update(
    settings: UpdateMobileSyncSettingsOutput,
) -> MobileSyncSettingsUpdateSummary {
    MobileSyncSettingsUpdateSummary {
        enabled: settings.enabled,
        lan_listen_enabled: settings.lan_listen_enabled,
        lan_advertise_ip: settings.lan_advertise_ip,
        lan_advertise_base_url: settings.lan_advertise_base_url,
        lan_port: settings.lan_port,
        changed: settings.restart_required,
    }
}

fn map_registration_error(
    error: RegisterMobileShortcutDeviceError,
) -> Result<OperationResult, EngineError> {
    let outcome = match error {
        RegisterMobileShortcutDeviceError::LabelEmpty => {
            MobileDeviceRegistrationOutcome::LabelEmpty
        }
        RegisterMobileShortcutDeviceError::LabelTooLong => {
            MobileDeviceRegistrationOutcome::LabelTooLong
        }
        RegisterMobileShortcutDeviceError::LanListenerDisabled => {
            MobileDeviceRegistrationOutcome::LanListenerDisabled
        }
        RegisterMobileShortcutDeviceError::UsernameTaken(username) => {
            MobileDeviceRegistrationOutcome::UsernameTaken { username }
        }
        RegisterMobileShortcutDeviceError::UsernameTooShort { min, got } => {
            MobileDeviceRegistrationOutcome::UsernameTooShort { min, got }
        }
        RegisterMobileShortcutDeviceError::UsernameTooLong { max, got } => {
            MobileDeviceRegistrationOutcome::UsernameTooLong { max, got }
        }
        RegisterMobileShortcutDeviceError::UsernameMustStartWithLetter => {
            MobileDeviceRegistrationOutcome::UsernameMustStartWithLetter
        }
        RegisterMobileShortcutDeviceError::UsernameContainsForbiddenChars => {
            MobileDeviceRegistrationOutcome::UsernameContainsForbiddenChars
        }
        RegisterMobileShortcutDeviceError::PasswordTooShort { min } => {
            MobileDeviceRegistrationOutcome::PasswordTooShort { min }
        }
        RegisterMobileShortcutDeviceError::PasswordTooLong { max } => {
            MobileDeviceRegistrationOutcome::PasswordTooLong { max }
        }
        RegisterMobileShortcutDeviceError::NoLanInterfaceAvailable => {
            MobileDeviceRegistrationOutcome::NoLanInterfaceAvailable
        }
        RegisterMobileShortcutDeviceError::PasswordHashFailed(_)
        | RegisterMobileShortcutDeviceError::PersistenceFailed(_)
        | RegisterMobileShortcutDeviceError::QrRenderFailed(_)
        | RegisterMobileShortcutDeviceError::SettingsLoadFailed(_)
        | RegisterMobileShortcutDeviceError::LanInterfaceProbeFailed(_) => {
            return Err(internal_error(REGISTER_MOBILE_DEVICE_FAILED_CODE));
        }
    };
    Ok(OperationResult::MobileDeviceRegistered(outcome))
}

fn map_update_error(error: UpdateMobileDeviceError) -> Result<OperationResult, EngineError> {
    let outcome = match error {
        UpdateMobileDeviceError::NotFound(_) => MobileDeviceUpdateOutcome::NotFound,
        UpdateMobileDeviceError::LabelEmpty => MobileDeviceUpdateOutcome::LabelEmpty,
        UpdateMobileDeviceError::LabelTooLong => MobileDeviceUpdateOutcome::LabelTooLong,
        UpdateMobileDeviceError::UsernameTaken(username) => {
            MobileDeviceUpdateOutcome::UsernameTaken { username }
        }
        UpdateMobileDeviceError::UsernameTooShort { min, got } => {
            MobileDeviceUpdateOutcome::UsernameTooShort { min, got }
        }
        UpdateMobileDeviceError::UsernameTooLong { max, got } => {
            MobileDeviceUpdateOutcome::UsernameTooLong { max, got }
        }
        UpdateMobileDeviceError::UsernameMustStartWithLetter => {
            MobileDeviceUpdateOutcome::UsernameMustStartWithLetter
        }
        UpdateMobileDeviceError::UsernameContainsForbiddenChars => {
            MobileDeviceUpdateOutcome::UsernameContainsForbiddenChars
        }
        UpdateMobileDeviceError::PasswordTooShort { min } => {
            MobileDeviceUpdateOutcome::PasswordTooShort { min }
        }
        UpdateMobileDeviceError::PasswordTooLong { max } => {
            MobileDeviceUpdateOutcome::PasswordTooLong { max }
        }
        UpdateMobileDeviceError::PasswordHashFailed(_)
        | UpdateMobileDeviceError::PersistenceFailed(_) => {
            return Err(internal_error(UPDATE_MOBILE_DEVICE_FAILED_CODE));
        }
    };
    Ok(OperationResult::MobileDeviceUpdated(outcome))
}

fn internal_error(code: u32) -> EngineError {
    EngineError::new(code, EngineErrorCategory::Internal, true)
}
