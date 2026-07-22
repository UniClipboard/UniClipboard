use std::sync::Arc;
use std::time::Duration;

use napi::Status;
use napi_derive::napi;
use uc_engine::{
    CreateSpaceInput, Engine, EngineConfig, EventStream, Operation, OperationResult, SecretString,
};
use zeroize::Zeroizing;

use crate::{host, OhEngineConfig, OhHost, OhSpaceCreated};

#[napi]
pub struct OhEngine {
    engine: Arc<Engine>,
    _events: tokio::sync::Mutex<EventStream>,
}

impl OhEngine {
    pub(crate) async fn start(config: OhEngineConfig, host: OhHost) -> napi::Result<Self> {
        let capabilities = host::capabilities(host)?;
        let config = EngineConfig::new(config.app_version).with_profile_id(config.profile_id);
        let (engine, events) = Engine::start(config, capabilities)
            .await
            .map_err(engine_error)?;
        Ok(Self {
            engine: Arc::new(engine),
            _events: tokio::sync::Mutex::new(events),
        })
    }
}

#[napi]
impl OhEngine {
    #[napi]
    pub async fn create_space(
        &self,
        device_name: Option<String>,
        passphrase: String,
    ) -> napi::Result<OhSpaceCreated> {
        let passphrase = Zeroizing::new(passphrase);
        let result = self
            .engine
            .execute(Operation::CreateSpace(CreateSpaceInput {
                device_name,
                passphrase: SecretString::new(passphrase.as_str()),
                passphrase_confirmation: SecretString::new(passphrase.as_str()),
            }))
            .await
            .map_err(engine_error)?;
        match result {
            OperationResult::SpaceCreated {
                space_id,
                self_device_id,
                identity_fingerprint,
            } => Ok(OhSpaceCreated {
                space_id,
                self_device_id,
                identity_fingerprint,
            }),
            _ => Err(unexpected_result()),
        }
    }

    #[napi]
    pub async fn shutdown(&self, deadline_ms: u32) -> napi::Result<()> {
        self.engine
            .shutdown(Duration::from_millis(u64::from(deadline_ms)))
            .await
            .map_err(engine_error)
    }
}

fn engine_error(error: uc_engine::EngineError) -> napi::Error {
    napi::Error::new(
        Status::GenericFailure,
        format!(
            "UC_ENGINE:{}:{}:{}",
            error.code(),
            error.category(),
            error.is_retryable()
        ),
    )
}

fn unexpected_result() -> napi::Error {
    napi::Error::new(Status::GenericFailure, "UC_ENGINE:UNEXPECTED_RESULT")
}
