//! GUI-local desktop palette integration. Unsupported desktops return no override.

use crate::commands::{record_trace_fields, TraceMetadata};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tauri::State;
use tauri::{Emitter, Manager};
use tracing::{info_span, Instrument};

#[cfg(target_os = "linux")]
mod omarchy;
mod preferences;
#[cfg(target_os = "linux")]
mod rounding;

#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DesktopTheme {
    pub dark: bool,
    pub variables: BTreeMap<String, String>,
}

#[derive(Clone, Default, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DesktopThemeSnapshot {
    pub revision: u32,
    pub follow_omarchy_theme: bool,
    pub omarchy_available: bool,
    pub theme: Option<DesktopTheme>,
    pub window_corner_radius: Option<u32>,
}

#[derive(Clone, Default)]
pub struct DesktopThemeState(Arc<Mutex<DesktopThemeSnapshot>>);

impl DesktopThemeState {
    fn snapshot(&self) -> Result<DesktopThemeSnapshot, String> {
        self.0
            .lock()
            .map(|value| value.clone())
            .map_err(|_| "Desktop theme state unavailable".into())
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_desktop_theme(
    state: State<'_, DesktopThemeState>,
    _trace: Option<TraceMetadata>,
) -> Result<DesktopThemeSnapshot, String> {
    let span = info_span!(
        "command.get_desktop_theme",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty
    );
    record_trace_fields(&span, &_trace);
    async {
        let snapshot = state.snapshot();
        match &snapshot {
            Ok(value) => {
                tracing::debug!(available = value.theme.is_some(), "Desktop theme queried")
            }
            Err(error) => tracing::warn!(%error, "Desktop theme query failed"),
        }
        snapshot
    }
    .instrument(span)
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn set_follow_omarchy_theme(
    app: tauri::AppHandle,
    state: State<'_, DesktopThemeState>,
    enabled: bool,
    _trace: Option<TraceMetadata>,
) -> Result<DesktopThemeSnapshot, String> {
    let span = info_span!(
        "command.set_follow_omarchy_theme",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty
    );
    record_trace_fields(&span, &_trace);
    let state = state.inner().clone();
    async move {
        let result = tauri::async_runtime::spawn_blocking(move || {
            let mut value = state.0.lock().map_err(|_| "Desktop theme state unavailable".to_string())?;
            preferences::save(enabled).map_err(|_| "Unable to save desktop preferences".to_string())?;
            value.follow_omarchy_theme = enabled;
            value.revision = value.revision.saturating_add(1);
            Ok::<_, String>(value.clone())
        }).await.map_err(|_| "Desktop preference task failed".to_string()).and_then(|result| result);
        match result {
            Ok(snapshot) => {
                tracing::debug!(enabled, "Desktop theme preference saved");
                if let Err(error) = app.emit("desktop-theme://changed", &snapshot) {
                    tracing::warn!(%error, "Desktop theme broadcast failed");
                }
                Ok(snapshot)
            }
            Err(error) => {
                tracing::warn!(%error, error_kind = "preference_save_failed", "Desktop preference save failed");
                Err(error)
            }
        }
    }.instrument(span).await
}

pub fn install(app: &tauri::AppHandle, cancel: tokio_util::sync::CancellationToken) {
    let enabled = match preferences::load() {
        Ok(enabled) => enabled,
        Err(error) => {
            tracing::warn!(error_kind = "preference_read_failed", io_kind = ?error.kind(), "Unable to load desktop preferences");
            false
        }
    };
    match app.state::<DesktopThemeState>().0.lock() {
        Ok(mut value) => value.follow_omarchy_theme = enabled,
        Err(_) => tracing::warn!("Desktop theme state unavailable at startup"),
    }
    #[cfg(target_os = "linux")]
    {
        rounding::install(
            app.clone(),
            app.state::<DesktopThemeState>().inner().clone(),
            cancel.clone(),
        );
        omarchy::install(
            app.clone(),
            app.state::<DesktopThemeState>().inner().clone(),
            cancel,
        );
    }
    #[cfg(not(target_os = "linux"))]
    let _ = (app, cancel);
}
