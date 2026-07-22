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
    pub file_metadata: ThreadsafeFunction<String, ErrorStrategy::Fatal>,
    pub file_read_chunk: ThreadsafeFunction<(String, String, u32), ErrorStrategy::Fatal>,
    pub file_write_chunk: ThreadsafeFunction<(String, String, Buffer), ErrorStrategy::Fatal>,
    pub file_finish_write: ThreadsafeFunction<String, ErrorStrategy::Fatal>,
    pub clipboard_read: ThreadsafeFunction<(), ErrorStrategy::Fatal>,
    pub clipboard_write: ThreadsafeFunction<OhClipboardSnapshot, ErrorStrategy::Fatal>,
}

#[napi(object)]
pub struct OhClipboardRepresentation {
    pub kind: String,
    pub format: String,
    pub mime_type: Option<String>,
    pub bytes: Option<Buffer>,
    pub handle: Option<String>,
    pub display_name: Option<String>,
    pub size_bytes: Option<String>,
}

#[napi(object)]
pub struct OhClipboardSnapshot {
    pub observed_at_ms: f64,
    pub representations: Vec<OhClipboardRepresentation>,
}

#[napi(object)]
pub struct OhSpaceCreated {
    pub space_id: String,
    pub self_device_id: String,
    pub identity_fingerprint: String,
}

#[napi(object)]
pub struct OhInvitationIssued {
    pub invitation_code: String,
    pub expires_at_ms: f64,
    pub availability: String,
}

#[napi(object)]
pub struct OhSpaceJoined {
    pub sponsor_device_id: String,
    pub sponsor_identity_fingerprint: String,
    pub space_id: String,
    pub self_device_id: String,
    pub self_identity_fingerprint: String,
    pub migrated_records: Option<String>,
}

#[napi(object)]
pub struct OhSendReport {
    pub entry_id: String,
    pub at_ms: f64,
    pub total_accepted: u32,
    pub total_duplicate: u32,
    pub total_offline: u32,
    pub total_errored: u32,
    pub total_pending: u32,
}

#[napi(object)]
pub struct OhEngineEvent {
    pub kind: String,
    pub state: Option<String>,
    pub refresh_reason: Option<String>,
    pub operation_id: Option<String>,
    pub terminal: Option<String>,
    pub error_code: Option<u32>,
    pub error_category: Option<String>,
    pub retryable: Option<bool>,
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
