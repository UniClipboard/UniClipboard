use uc_application::facade::mobile_sync::{
    AuthenticateBasicAuthError, AuthenticateBasicAuthInput, IsDeviceCredentialCurrentError,
    ListMobileDevicesError, MobileSyncFacade, RevokeMobileDeviceError, RevokeMobileDeviceInput,
};
use uc_core::mobile_sync::{MobileClientType, MobileDeviceId};

use crate::{
    AuthenticateMobileRequestInput, EngineError, EngineErrorCategory, MobileAuthenticatedSession,
    MobileAuthenticationOutcome, MobileClientTypeSummary, MobileCredential, MobileDeviceInput,
    MobileDeviceRevokeOutcome, MobileDeviceSummary, OperationResult,
    RevalidateMobileCredentialInput,
};

pub const LIST_MOBILE_DEVICES_FAILED_CODE: u32 = 1431;
pub const REVOKE_MOBILE_DEVICE_FAILED_CODE: u32 = 1432;
pub const AUTHENTICATE_MOBILE_REQUEST_FAILED_CODE: u32 = 1433;
pub const REVALIDATE_MOBILE_CREDENTIAL_FAILED_CODE: u32 = 1434;

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

fn internal_error(code: u32) -> EngineError {
    EngineError::new(code, EngineErrorCategory::Internal, true)
}
