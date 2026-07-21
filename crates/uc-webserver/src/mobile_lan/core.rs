use std::sync::Arc;

use uc_engine::{
    AbortMobileFileUploadInput, AppendMobileFileUploadInput, ApplyMobileSyncDocumentInput,
    AuthenticateMobileRequestInput, BeginMobileFileUploadInput, Engine, EngineError,
    EngineErrorCategory, FinishMobileFileUploadInput, MobileAuthenticatedSession,
    MobileContentAvailabilityInput, MobileCredential, MobileFileUploadHandle, MobileSyncDocument,
    MobileSyncDocumentApplyOutcome, MobileSyncFileReadOutcome, Operation, OperationResult,
    ReadMobileSyncFileInput, RevalidateMobileCredentialInput, SecretString,
};

#[cfg(test)]
use tokio::sync::Mutex;
#[cfg(test)]
use uc_application::facade::{
    ApplyIncomingMobileClipOutcome, AuthenticateBasicAuthError, AuthenticateBasicAuthInput,
    GetLatestMobileSyncDocError, GetMobileSyncFileError, MobileSyncFacade, SyncClipboardItemType,
    SyncClipboardMeta,
};
#[cfg(test)]
use uc_core::mobile_sync::{MobileClientType, MobileDeviceId, StagingHandle};

const UNEXPECTED_MOBILE_LAN_RESULT_CODE: u32 = 1901;

#[derive(Clone)]
pub(crate) struct MobileLanCore {
    backend: MobileLanBackend,
}

#[derive(Clone)]
enum MobileLanBackend {
    Engine(Arc<Engine>),
    #[cfg(test)]
    Facade {
        facade: Arc<MobileSyncFacade>,
        credential: Arc<Mutex<Option<(String, String)>>>,
    },
}

#[derive(Clone)]
pub(crate) enum MobileLanUploadHandle {
    Engine(MobileFileUploadHandle),
    #[cfg(test)]
    Facade {
        handle: StagingHandle,
        data_name: String,
        source_device_id: String,
        transfer_id: String,
    },
}

impl std::fmt::Debug for MobileLanUploadHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("MobileLanUploadHandle([REDACTED])")
    }
}

impl MobileLanCore {
    pub(crate) fn new(engine: Arc<Engine>) -> Self {
        Self {
            backend: MobileLanBackend::Engine(engine),
        }
    }

    pub(crate) async fn authenticate(
        &self,
        authorization: String,
    ) -> Result<Option<MobileAuthenticatedSession>, EngineError> {
        match &self.backend {
            MobileLanBackend::Engine(engine) => match engine
                .execute(Operation::AuthenticateMobileRequest(
                    AuthenticateMobileRequestInput {
                        authorization: SecretString::new(authorization),
                    },
                ))
                .await?
            {
                OperationResult::MobileRequestAuthenticated(session) => Ok(Some(session)),
                OperationResult::MobileAuthentication(_) => Ok(None),
                _ => Err(unexpected_result()),
            },
            #[cfg(test)]
            MobileLanBackend::Facade { facade, credential } => {
                test_authenticate(facade, credential, authorization).await
            }
        }
    }

    pub(crate) async fn revalidate(
        &self,
        credential: MobileCredential,
    ) -> Result<bool, EngineError> {
        match &self.backend {
            MobileLanBackend::Engine(engine) => match engine
                .execute(Operation::RevalidateMobileCredential(
                    RevalidateMobileCredentialInput { credential },
                ))
                .await?
            {
                OperationResult::MobileCredentialCurrent { current } => Ok(current),
                _ => Err(unexpected_result()),
            },
            #[cfg(test)]
            MobileLanBackend::Facade {
                facade,
                credential: stored,
            } => test_revalidate(facade, stored).await,
        }
    }

    pub(crate) async fn check_content_available(
        &self,
        snapshot_hash: String,
    ) -> Result<bool, EngineError> {
        match &self.backend {
            MobileLanBackend::Engine(engine) => match engine
                .execute(Operation::CheckMobileContentAvailable(
                    MobileContentAvailabilityInput { snapshot_hash },
                ))
                .await?
            {
                OperationResult::MobileContentAvailability { available } => Ok(available),
                _ => Err(unexpected_result()),
            },
            #[cfg(test)]
            MobileLanBackend::Facade { facade, .. } => facade
                .check_content_available(&snapshot_hash)
                .await
                .map_err(|_| unexpected_result()),
        }
    }

    pub(crate) async fn latest_document(&self) -> Result<Option<MobileSyncDocument>, EngineError> {
        match &self.backend {
            MobileLanBackend::Engine(engine) => {
                match engine
                    .execute(Operation::QueryLatestMobileSyncDocument)
                    .await?
                {
                    OperationResult::MobileSyncDocument(document) => {
                        Ok(document.map(|value| *value))
                    }
                    _ => Err(unexpected_result()),
                }
            }
            #[cfg(test)]
            MobileLanBackend::Facade { facade, .. } => test_latest_document(facade).await,
        }
    }

    pub(crate) async fn apply_document(
        &self,
        input: ApplyMobileSyncDocumentInput,
    ) -> Result<MobileSyncDocumentApplyOutcome, EngineError> {
        match &self.backend {
            MobileLanBackend::Engine(engine) => match engine
                .execute(Operation::ApplyMobileSyncDocument(Box::new(input)))
                .await?
            {
                OperationResult::MobileSyncDocumentApplied(outcome) => Ok(outcome),
                _ => Err(unexpected_result()),
            },
            #[cfg(test)]
            MobileLanBackend::Facade { facade, .. } => test_apply_document(facade, input).await,
        }
    }

    pub(crate) async fn read_file(
        &self,
        data_name: String,
    ) -> Result<MobileSyncFileReadOutcome, EngineError> {
        match &self.backend {
            MobileLanBackend::Engine(engine) => match engine
                .execute(Operation::ReadMobileSyncFile(ReadMobileSyncFileInput {
                    data_name,
                }))
                .await?
            {
                OperationResult::MobileSyncFile(outcome) => Ok(outcome),
                _ => Err(unexpected_result()),
            },
            #[cfg(test)]
            MobileLanBackend::Facade { facade, .. } => test_read_file(facade, data_name).await,
        }
    }

    pub(crate) async fn begin_upload(
        &self,
        data_name: String,
        media_type: String,
        source_device_id: String,
        transfer_id: String,
        total_bytes: Option<u64>,
    ) -> Result<MobileLanUploadHandle, EngineError> {
        match &self.backend {
            MobileLanBackend::Engine(engine) => match engine
                .execute(Operation::BeginMobileFileUpload(
                    BeginMobileFileUploadInput {
                        data_name,
                        media_type,
                        source_device_id,
                        transfer_id,
                        total_bytes,
                    },
                ))
                .await?
            {
                OperationResult::MobileFileUploadStarted(handle) => {
                    Ok(MobileLanUploadHandle::Engine(handle))
                }
                _ => Err(unexpected_result()),
            },
            #[cfg(test)]
            MobileLanBackend::Facade { facade, .. } => {
                let scope = uc_application::facade::mobile_sync_streaming_scope_nonce();
                facade
                    .begin_file_upload(&scope, &data_name, &media_type)
                    .await
                    .map(|handle| MobileLanUploadHandle::Facade {
                        handle,
                        data_name,
                        source_device_id,
                        transfer_id,
                    })
                    .map_err(|_| unexpected_result())
            }
        }
    }

    pub(crate) async fn append_upload(
        &self,
        handle: MobileLanUploadHandle,
        bytes: Vec<u8>,
    ) -> Result<(), EngineError> {
        #[allow(unreachable_patterns)]
        match (&self.backend, handle) {
            (MobileLanBackend::Engine(engine), MobileLanUploadHandle::Engine(handle)) => {
                match engine
                    .execute(Operation::AppendMobileFileUpload(
                        AppendMobileFileUploadInput { handle, bytes },
                    ))
                    .await?
                {
                    OperationResult::MobileFileUploadChunkAppended => Ok(()),
                    _ => Err(unexpected_result()),
                }
            }
            #[cfg(test)]
            (
                MobileLanBackend::Facade { facade, .. },
                MobileLanUploadHandle::Facade { handle, .. },
            ) => facade
                .append_file_chunk(&handle, &bytes)
                .await
                .map_err(|_| unexpected_result()),
            _ => Err(unexpected_result()),
        }
    }

    pub(crate) async fn finish_upload(
        &self,
        handle: MobileLanUploadHandle,
        media_type: String,
    ) -> Result<MobileSyncDocumentApplyOutcome, EngineError> {
        #[allow(unreachable_patterns)]
        match (&self.backend, handle) {
            (MobileLanBackend::Engine(engine), MobileLanUploadHandle::Engine(handle)) => {
                match engine
                    .execute(Operation::FinishMobileFileUpload(
                        FinishMobileFileUploadInput { handle, media_type },
                    ))
                    .await?
                {
                    OperationResult::MobileFileUploadFinished(outcome) => Ok(outcome),
                    _ => Err(unexpected_result()),
                }
            }
            #[cfg(test)]
            (
                MobileLanBackend::Facade { facade, .. },
                MobileLanUploadHandle::Facade {
                    handle,
                    data_name,
                    source_device_id,
                    transfer_id,
                },
            ) => facade
                .finalize_file_upload(
                    handle,
                    data_name,
                    media_type,
                    uc_core::mobile_sync::MobileDeviceId::new(source_device_id),
                    transfer_id,
                )
                .await
                .map(test_map_apply_outcome)
                .map_err(|_| unexpected_result()),
            _ => Err(unexpected_result()),
        }
    }

    pub(crate) async fn abort_upload(
        &self,
        handle: MobileLanUploadHandle,
    ) -> Result<bool, EngineError> {
        #[allow(unreachable_patterns)]
        match (&self.backend, handle) {
            (MobileLanBackend::Engine(engine), MobileLanUploadHandle::Engine(handle)) => {
                match engine
                    .execute(Operation::AbortMobileFileUpload(
                        AbortMobileFileUploadInput { handle },
                    ))
                    .await?
                {
                    OperationResult::MobileFileUploadAborted { existed } => Ok(existed),
                    _ => Err(unexpected_result()),
                }
            }
            #[cfg(test)]
            (
                MobileLanBackend::Facade { facade, .. },
                MobileLanUploadHandle::Facade { handle, .. },
            ) => {
                facade.abort_file_upload(handle).await;
                Ok(true)
            }
            _ => Err(unexpected_result()),
        }
    }
}

impl From<Arc<Engine>> for MobileLanCore {
    fn from(engine: Arc<Engine>) -> Self {
        Self::new(engine)
    }
}

#[cfg(test)]
impl From<Arc<MobileSyncFacade>> for MobileLanCore {
    fn from(facade: Arc<MobileSyncFacade>) -> Self {
        Self {
            backend: MobileLanBackend::Facade {
                facade,
                credential: Arc::new(Mutex::new(None)),
            },
        }
    }
}

#[cfg(test)]
async fn test_authenticate(
    facade: &MobileSyncFacade,
    stored: &Mutex<Option<(String, String)>>,
    authorization: String,
) -> Result<Option<MobileAuthenticatedSession>, EngineError> {
    match facade
        .authenticate_basic(AuthenticateBasicAuthInput {
            authorization_header: authorization,
        })
        .await
    {
        Ok(authenticated) => {
            let device = authenticated.device;
            let device_id = device.device_id.into_string();
            let password_hash = device.password_hash;
            *stored.lock().await = Some((device_id.clone(), password_hash.clone()));
            Ok(Some(MobileAuthenticatedSession {
                device_id: device_id.clone(),
                client_type: match device.client_type {
                    MobileClientType::IosShortcut => {
                        uc_engine::MobileClientTypeSummary::IosShortcut
                    }
                },
                credential: MobileCredential::new(device_id, password_hash),
            }))
        }
        Err(AuthenticateBasicAuthError::InvalidCredentials) => Ok(None),
        Err(
            AuthenticateBasicAuthError::PersistenceFailed(_)
            | AuthenticateBasicAuthError::Internal(_),
        ) => Err(unexpected_result()),
    }
}

#[cfg(test)]
async fn test_revalidate(
    facade: &MobileSyncFacade,
    stored: &Mutex<Option<(String, String)>>,
) -> Result<bool, EngineError> {
    let Some((device_id, password_hash)) = stored.lock().await.clone() else {
        return Ok(false);
    };
    facade
        .is_device_credential_current(&MobileDeviceId::new(device_id), &password_hash)
        .await
        .map_err(|_| unexpected_result())
}

#[cfg(test)]
async fn test_latest_document(
    facade: &MobileSyncFacade,
) -> Result<Option<MobileSyncDocument>, EngineError> {
    match facade.get_latest_sync_doc().await {
        Ok(document) => Ok(Some(test_map_document(document))),
        Err(GetLatestMobileSyncDocError::NotFound) => Ok(None),
        Err(GetLatestMobileSyncDocError::Port(_)) => Err(unexpected_result()),
    }
}

#[cfg(test)]
async fn test_apply_document(
    facade: &MobileSyncFacade,
    input: ApplyMobileSyncDocumentInput,
) -> Result<MobileSyncDocumentApplyOutcome, EngineError> {
    facade
        .put_sync_doc(
            test_unmap_document(input.document),
            MobileDeviceId::new(input.source_device_id),
        )
        .await
        .map(test_map_apply_outcome)
        .map_err(|_| unexpected_result())
}

#[cfg(test)]
async fn test_read_file(
    facade: &MobileSyncFacade,
    data_name: String,
) -> Result<MobileSyncFileReadOutcome, EngineError> {
    match facade.get_clipboard_file(&data_name).await {
        Ok(file) => Ok(MobileSyncFileReadOutcome::Found(Box::new(
            uc_engine::MobileSyncFile {
                media_type: file.mime,
                bytes: file.bytes,
            },
        ))),
        Err(GetMobileSyncFileError::NotFound) => Ok(MobileSyncFileReadOutcome::NotFound),
        Err(GetMobileSyncFileError::Port(_) | GetMobileSyncFileError::Staging(_)) => {
            Err(unexpected_result())
        }
    }
}

#[cfg(test)]
fn test_map_document(document: SyncClipboardMeta) -> MobileSyncDocument {
    MobileSyncDocument {
        item_type: test_map_item_type(document.item_type),
        text: document.text,
        data_name: document.data_name,
        has_data: document.has_data,
        size: document.size,
        hash: document.hash,
        content_id: document.content_id,
    }
}

#[cfg(test)]
fn test_unmap_document(document: MobileSyncDocument) -> SyncClipboardMeta {
    SyncClipboardMeta {
        item_type: match document.item_type {
            uc_engine::MobileSyncItemType::Text => SyncClipboardItemType::Text,
            uc_engine::MobileSyncItemType::Image => SyncClipboardItemType::Image,
            uc_engine::MobileSyncItemType::File => SyncClipboardItemType::File,
            uc_engine::MobileSyncItemType::Group => SyncClipboardItemType::Group,
        },
        text: document.text,
        data_name: document.data_name,
        has_data: document.has_data,
        size: document.size,
        hash: document.hash,
        content_id: document.content_id,
    }
}

#[cfg(test)]
fn test_map_item_type(item_type: SyncClipboardItemType) -> uc_engine::MobileSyncItemType {
    match item_type {
        SyncClipboardItemType::Text => uc_engine::MobileSyncItemType::Text,
        SyncClipboardItemType::Image => uc_engine::MobileSyncItemType::Image,
        SyncClipboardItemType::File => uc_engine::MobileSyncItemType::File,
        SyncClipboardItemType::Group => uc_engine::MobileSyncItemType::Group,
    }
}

#[cfg(test)]
fn test_map_apply_outcome(
    outcome: ApplyIncomingMobileClipOutcome,
) -> MobileSyncDocumentApplyOutcome {
    match outcome {
        ApplyIncomingMobileClipOutcome::Applied {
            entry_id,
            content_id,
        } => MobileSyncDocumentApplyOutcome::Applied {
            entry_id: entry_id.to_string(),
            content_id,
        },
        ApplyIncomingMobileClipOutcome::Resurfaced {
            snapshot_hash,
            existing_entry_id,
            os_write_succeeded,
        } => MobileSyncDocumentApplyOutcome::Resurfaced {
            snapshot_hash,
            existing_entry_id: existing_entry_id.to_string(),
            os_write_succeeded,
        },
        ApplyIncomingMobileClipOutcome::DuplicateSkipped {
            snapshot_hash,
            existing_entry_id,
        } => MobileSyncDocumentApplyOutcome::DuplicateSkipped {
            snapshot_hash,
            existing_entry_id: existing_entry_id.to_string(),
        },
        ApplyIncomingMobileClipOutcome::DecodeFailed { reason } => {
            MobileSyncDocumentApplyOutcome::DecodeFailed { reason }
        }
        ApplyIncomingMobileClipOutcome::Buffered => MobileSyncDocumentApplyOutcome::Buffered,
    }
}

fn unexpected_result() -> EngineError {
    EngineError::new(
        UNEXPECTED_MOBILE_LAN_RESULT_CODE,
        EngineErrorCategory::Internal,
        false,
    )
}
