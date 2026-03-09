//! Setup-related Tauri commands
//! 设置流程相关的 Tauri 命令

use crate::bootstrap::AppRuntime;
use crate::commands::record_trace_fields;
use std::sync::Arc;
use tauri::State;
use tracing::{info_span, Instrument};
use uc_core::ports::observability::TraceMetadata;
use uc_core::setup::SetupState;

fn encode_setup_state(state: SetupState) -> Result<String, String> {
    serde_json::to_string(&state).map_err(|e| e.to_string())
}

/// Get current setup state.
/// 获取当前设置流程状态。
#[tauri::command]
pub async fn get_setup_state(
    runtime: State<'_, Arc<AppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<String, String> {
    let span = info_span!(
        "command.setup.get_state",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);
    async {
        let state = runtime.usecases().setup_orchestrator().get_state().await;
        encode_setup_state(state)
    }
    .instrument(span)
    .await
}

#[tauri::command]
pub async fn start_new_space(
    runtime: State<'_, Arc<AppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<String, String> {
    let span = info_span!(
        "command.setup.start_new_space",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);
    async {
        let orchestrator = runtime.usecases().setup_orchestrator();
        let state = orchestrator.new_space().await.map_err(|e| e.to_string())?;
        encode_setup_state(state)
    }
    .instrument(span)
    .await
}

#[tauri::command]
pub async fn start_join_space(
    runtime: State<'_, Arc<AppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<String, String> {
    let span = info_span!(
        "command.setup.start_join_space",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);
    async {
        let orchestrator = runtime.usecases().setup_orchestrator();
        let state = orchestrator.join_space().await.map_err(|e| e.to_string())?;
        encode_setup_state(state)
    }
    .instrument(span)
    .await
}

#[tauri::command]
pub async fn select_device(
    runtime: State<'_, Arc<AppRuntime>>,
    peer_id: String,
    _trace: Option<TraceMetadata>,
) -> Result<String, String> {
    let span = info_span!(
        "command.setup.select_device",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);
    async {
        let orchestrator = runtime.usecases().setup_orchestrator();
        let state = orchestrator
            .select_device(peer_id)
            .await
            .map_err(|e| e.to_string())?;
        encode_setup_state(state)
    }
    .instrument(span)
    .await
}

#[tauri::command]
pub async fn submit_passphrase(
    runtime: State<'_, Arc<AppRuntime>>,
    passphrase1: String,
    passphrase2: String,
    _trace: Option<TraceMetadata>,
) -> Result<String, String> {
    let span = info_span!(
        "command.setup.submit_passphrase",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);
    async {
        let orchestrator = runtime.usecases().setup_orchestrator();
        let state = orchestrator
            .submit_passphrase(passphrase1, passphrase2)
            .await
            .map_err(|e| e.to_string())?;
        encode_setup_state(state)
    }
    .instrument(span)
    .await
}

#[tauri::command]
pub async fn verify_passphrase(
    runtime: State<'_, Arc<AppRuntime>>,
    passphrase: String,
    _trace: Option<TraceMetadata>,
) -> Result<String, String> {
    let span = info_span!(
        "command.setup.verify_passphrase",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);
    async {
        let orchestrator = runtime.usecases().setup_orchestrator();
        let state = orchestrator
            .verify_passphrase(passphrase)
            .await
            .map_err(|e| e.to_string())?;
        encode_setup_state(state)
    }
    .instrument(span)
    .await
}

#[tauri::command]
pub async fn confirm_peer_trust(
    runtime: State<'_, Arc<AppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<String, String> {
    let span = info_span!(
        "command.setup.confirm_peer_trust",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);
    async {
        let orchestrator = runtime.usecases().setup_orchestrator();
        let state = orchestrator
            .confirm_peer_trust()
            .await
            .map_err(|e| e.to_string())?;
        encode_setup_state(state)
    }
    .instrument(span)
    .await
}

#[tauri::command]
pub async fn cancel_setup(
    runtime: State<'_, Arc<AppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<String, String> {
    let span = info_span!(
        "command.setup.cancel_setup",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);
    async {
        let orchestrator = runtime.usecases().setup_orchestrator();
        let state = orchestrator
            .cancel_setup()
            .await
            .map_err(|e| e.to_string())?;
        encode_setup_state(state)
    }
    .instrument(span)
    .await
}

#[cfg(test)]
mod tests {
    use super::encode_setup_state;
    use uc_core::setup::SetupState;

    #[test]
    fn encode_setup_state_welcome() {
        let encoded = encode_setup_state(SetupState::Welcome).expect("encode failed");
        assert_eq!(encoded, "\"Welcome\"");
    }

    #[test]
    fn encode_setup_state_create_space_passphrase() {
        let encoded = encode_setup_state(SetupState::CreateSpaceInputPassphrase { error: None })
            .expect("encode failed");
        assert_eq!(encoded, "{\"CreateSpaceInputPassphrase\":{\"error\":null}}");
    }
}
