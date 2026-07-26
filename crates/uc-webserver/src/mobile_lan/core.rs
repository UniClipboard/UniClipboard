use std::sync::Arc;

use uc_engine::{
    AbortMobileFileUploadInput, AppendMobileFileUploadInput, ApplyMobileSyncDocumentInput,
    AuthenticateMobileRequestInput, BeginMobileFileUploadInput, Engine, EngineError,
    EngineErrorCategory, FinishMobileFileUploadInput, MobileAuthenticatedSession,
    MobileContentAvailabilityInput, MobileCredential, MobileFileUploadHandle, MobileSyncDocument,
    MobileSyncDocumentApplyOutcome, MobileSyncFileReadOutcome, Operation, OperationResult,
    ReadMobileSyncFileInput, RevalidateMobileCredentialInput, SecretString,
};

const UNEXPECTED_MOBILE_LAN_RESULT_CODE: u32 = 1901;

#[cfg(test)]
#[async_trait::async_trait]
pub(crate) trait MobileLanTestBackend: Send + Sync {
    async fn authenticate(
        &self,
        authorization: String,
    ) -> Result<Option<MobileAuthenticatedSession>, EngineError>;

    async fn revalidate(&self, _credential: MobileCredential) -> Result<bool, EngineError> {
        Err(unexpected_result())
    }

    async fn check_content_available(&self, _snapshot_hash: String) -> Result<bool, EngineError> {
        Err(unexpected_result())
    }

    async fn latest_document(&self) -> Result<Option<MobileSyncDocument>, EngineError> {
        Err(unexpected_result())
    }

    async fn apply_document(
        &self,
        _input: ApplyMobileSyncDocumentInput,
    ) -> Result<MobileSyncDocumentApplyOutcome, EngineError> {
        Err(unexpected_result())
    }

    async fn read_file(
        &self,
        _data_name: String,
    ) -> Result<MobileSyncFileReadOutcome, EngineError> {
        Err(unexpected_result())
    }

    async fn begin_upload(
        &self,
        _data_name: String,
        _media_type: String,
        _source_device_id: String,
        _transfer_id: String,
        _total_bytes: Option<u64>,
    ) -> Result<u64, EngineError> {
        Err(unexpected_result())
    }

    async fn append_upload(&self, _handle: u64, _bytes: Vec<u8>) -> Result<(), EngineError> {
        Err(unexpected_result())
    }

    async fn finish_upload(
        &self,
        _handle: u64,
        _media_type: String,
    ) -> Result<MobileSyncDocumentApplyOutcome, EngineError> {
        Err(unexpected_result())
    }

    async fn abort_upload(&self, _handle: u64) -> Result<bool, EngineError> {
        Err(unexpected_result())
    }
}

#[derive(Clone)]
pub(crate) struct MobileLanCore {
    backend: MobileLanBackend,
}

#[derive(Clone)]
enum MobileLanBackend {
    Engine(Arc<Engine>),
    #[cfg(test)]
    Test(Arc<dyn MobileLanTestBackend>),
}

#[derive(Clone)]
pub(crate) enum MobileLanUploadHandle {
    Engine(MobileFileUploadHandle),
    #[cfg(test)]
    Test(u64),
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

    #[cfg(test)]
    pub(crate) fn with_test_backend(backend: Arc<dyn MobileLanTestBackend>) -> Self {
        Self {
            backend: MobileLanBackend::Test(backend),
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
            MobileLanBackend::Test(backend) => backend.authenticate(authorization).await,
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
            MobileLanBackend::Test(backend) => backend.revalidate(credential).await,
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
            MobileLanBackend::Test(backend) => backend.check_content_available(snapshot_hash).await,
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
            MobileLanBackend::Test(backend) => backend.latest_document().await,
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
            MobileLanBackend::Test(backend) => backend.apply_document(input).await,
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
            MobileLanBackend::Test(backend) => backend.read_file(data_name).await,
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
            MobileLanBackend::Test(backend) => backend
                .begin_upload(
                    data_name,
                    media_type,
                    source_device_id,
                    transfer_id,
                    total_bytes,
                )
                .await
                .map(MobileLanUploadHandle::Test),
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
            (MobileLanBackend::Test(backend), MobileLanUploadHandle::Test(handle)) => {
                backend.append_upload(handle, bytes).await
            }
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
            (MobileLanBackend::Test(backend), MobileLanUploadHandle::Test(handle)) => {
                backend.finish_upload(handle, media_type).await
            }
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
            (MobileLanBackend::Test(backend), MobileLanUploadHandle::Test(handle)) => {
                backend.abort_upload(handle).await
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

fn unexpected_result() -> EngineError {
    EngineError::new(
        UNEXPECTED_MOBILE_LAN_RESULT_CODE,
        EngineErrorCategory::Internal,
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    struct AuthenticationOnlyBackend;

    #[async_trait::async_trait]
    impl MobileLanTestBackend for AuthenticationOnlyBackend {
        async fn authenticate(
            &self,
            authorization: String,
        ) -> Result<Option<MobileAuthenticatedSession>, EngineError> {
            Ok(
                (authorization == "Basic test").then(|| MobileAuthenticatedSession {
                    device_id: "test-device".into(),
                    client_type: uc_engine::MobileClientTypeSummary::IosShortcut,
                    credential: MobileCredential::new("test-device", "test-credential"),
                }),
            )
        }
    }

    #[tokio::test]
    async fn test_backend_authenticates_without_core_internal_types() {
        let core = MobileLanCore::with_test_backend(Arc::new(AuthenticationOnlyBackend));

        let authenticated = core
            .authenticate("Basic test".into())
            .await
            .expect("test backend should return a result");

        assert_eq!(
            authenticated.map(|session| session.device_id),
            Some("test-device".into())
        );
    }
}
