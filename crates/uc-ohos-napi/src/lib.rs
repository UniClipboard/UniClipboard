//! HarmonyOS N-API bindings for the public `uc-engine` interface.

mod host;
mod runtime;

use napi::bindgen_prelude::{Buffer, External};
use napi::threadsafe_function::{ErrorStrategy, ThreadsafeFunction};
use napi::Env;
use napi_derive::napi;

pub use runtime::OhEngine;

#[napi(object)]
pub struct OhEngineConfig {
    pub app_version: String,
    pub profile_id: String,
}

#[napi(object, object_to_js = false)]
pub struct OhHost {
    pub private_data_directory: String,
    pub cache_directory: String,
    pub temporary_directory: String,
    pub secure_storage_get: ThreadsafeFunction<String, ErrorStrategy::Fatal>,
    pub secure_storage_set: ThreadsafeFunction<(String, Buffer), ErrorStrategy::Fatal>,
    pub secure_storage_delete: ThreadsafeFunction<String, ErrorStrategy::Fatal>,
}

#[napi(object)]
pub struct OhSpaceCreated {
    pub space_id: String,
    pub self_device_id: String,
    pub identity_fingerprint: String,
}

pub struct PreparedHost {
    host: Option<OhHost>,
}

#[napi]
pub fn core_version() -> String {
    format!("core-v{}", env!("CARGO_PKG_VERSION"))
}

#[napi]
pub fn prepare_host(env: Env, mut host: OhHost) -> napi::Result<External<PreparedHost>> {
    host::unref_callbacks(&mut host, &env)?;
    Ok(External::new(PreparedHost { host: Some(host) }))
}

#[napi]
pub async fn start_engine(
    config: OhEngineConfig,
    mut prepared_host: External<PreparedHost>,
) -> napi::Result<OhEngine> {
    let host = prepared_host.host.take().ok_or_else(|| {
        napi::Error::new(
            napi::Status::InvalidArg,
            "OHOS_HOST_ALREADY_CONSUMED".to_owned(),
        )
    })?;
    OhEngine::start(config, host).await
}
