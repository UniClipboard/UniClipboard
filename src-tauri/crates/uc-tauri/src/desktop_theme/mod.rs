//! GUI-local desktop palette integration. Unsupported desktops return no override.

use crate::commands::{record_trace_fields, TraceMetadata};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
#[cfg(target_os = "linux")]
use tauri::Manager;
use tauri::State;
use tracing::{info_span, Instrument};

#[cfg(target_os = "linux")]
mod omarchy;

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
    pub theme: Option<DesktopTheme>,
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

pub fn install(app: &tauri::AppHandle, cancel: tokio_util::sync::CancellationToken) {
    #[cfg(target_os = "linux")]
    omarchy::install(
        app.clone(),
        app.state::<DesktopThemeState>().inner().clone(),
        cancel,
    );
    #[cfg(not(target_os = "linux"))]
    let _ = (app, cancel);
}
