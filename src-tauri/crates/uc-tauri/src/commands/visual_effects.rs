use crate::commands::{record_trace_fields, TraceMetadata};
use crate::visual_effects::{
    EffectsMode, EffectsSample, EffectsSnapshot, SamplePermit, SystemMotion, VisualEffects,
    VisualEffectsService, EFFECTS_EVENT,
};
use tauri::Emitter;
use tracing::Instrument;

fn broadcast(app: &tauri::AppHandle, state: &VisualEffects) {
    if let Err(error) = app.emit(EFFECTS_EVENT, state.snapshot()) {
        tracing::warn!(error_kind = "visual_effects_emit", source = %error, "visual preferences notification failed");
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_visual_effects(
    app: tauri::AppHandle,
    service: tauri::State<'_, VisualEffectsService>,

    _trace: Option<TraceMetadata>,
) -> Result<EffectsSnapshot, String> {
    let span = tracing::info_span!(
        "command.get_visual_effects",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty
    );
    record_trace_fields(&span, &_trace);
    async {
        let state = service.get().await.lock().await;
        let _ = &app;
        tracing::debug!("visual preferences command accepted");
        Ok(state.snapshot())
    }
    .instrument(span)
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn set_visual_effects_mode(
    app: tauri::AppHandle,
    service: tauri::State<'_, VisualEffectsService>,

    mode: EffectsMode,
    _trace: Option<TraceMetadata>,
) -> Result<EffectsSnapshot, String> {
    let span = tracing::info_span!(
        "command.set_visual_effects_mode",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty
    );
    record_trace_fields(&span, &_trace);
    async {
        let mut state = service.get().await.lock().await;
        let _ = &app;
        tracing::debug!("visual preferences command accepted");
        state.set_mode(mode);
        broadcast(&app, &state);
        Ok(state.snapshot())
    }
    .instrument(span)
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn report_visual_effects_environment(
    app: tauri::AppHandle,
    service: tauri::State<'_, VisualEffectsService>,
    window: tauri::WebviewWindow,
    session_id: String,
    system_motion: SystemMotion,
    _trace: Option<TraceMetadata>,
) -> Result<EffectsSnapshot, String> {
    let span = tracing::info_span!(
        "command.report_visual_effects_environment",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty
    );
    record_trace_fields(&span, &_trace);
    async {
        let mut state = service.get().await.lock().await;
        let _ = &app;
        tracing::debug!("visual preferences command accepted");
        state.environment(window.label(), &session_id, system_motion);
        broadcast(&app, &state);
        Ok(state.snapshot())
    }
    .instrument(span)
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn begin_visual_effects_sample(
    app: tauri::AppHandle,
    service: tauri::State<'_, VisualEffectsService>,
    window: tauri::WebviewWindow,
    session_id: String,
    _trace: Option<TraceMetadata>,
) -> Result<Option<SamplePermit>, String> {
    let span = tracing::info_span!(
        "command.begin_visual_effects_sample",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty
    );
    record_trace_fields(&span, &_trace);
    async {
        let mut state = service.get().await.lock().await;
        let _ = &app;
        tracing::debug!("visual preferences command accepted");
        if !window.is_visible().unwrap_or(false) || window.is_minimized().unwrap_or(true) {
            return Ok(None);
        }
        Ok(state.begin_sample(window.label(), &session_id, std::time::Instant::now()))
    }
    .instrument(span)
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn report_visual_effects_sample(
    app: tauri::AppHandle,
    service: tauri::State<'_, VisualEffectsService>,
    window: tauri::WebviewWindow,
    sample: EffectsSample,
    _trace: Option<TraceMetadata>,
) -> Result<EffectsSnapshot, String> {
    let span = tracing::info_span!(
        "command.report_visual_effects_sample",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty
    );
    record_trace_fields(&span, &_trace);
    async {
        let mut state = service.get().await.lock().await;
        let _ = &app;
        tracing::debug!("visual preferences command accepted");
        if window.is_visible().unwrap_or(false) && !window.is_minimized().unwrap_or(true) {
            state.report_sample(window.label(), sample, std::time::Instant::now());
            broadcast(&app, &state);
        }
        Ok(state.snapshot())
    }
    .instrument(span)
    .await
}
