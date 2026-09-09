//! Effective compositor geometry, independent of the selected color palette.

use super::DesktopThemeState;
use tauri::Emitter;
use tracing::Instrument;
use uc_desktop::hyprland::Hyprland;

pub(super) fn install(
    app: tauri::AppHandle,
    state: DesktopThemeState,
    cancel: tokio_util::sync::CancellationToken,
) {
    let Some(compositor) = Hyprland::current() else {
        return;
    };
    let span = tracing::info_span!("desktop_theme.rounding", source = "hyprland");
    tauri::async_runtime::spawn(async move {
        tracing::debug!("Desktop rounding watcher started");
        // Runtime keyword overrides do not necessarily emit configreloaded.
        // A bounded read also recovers after temporary IPC failures.
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut failed = false;
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = interval.tick() => {},
            }
            let client = compositor.clone();
            let result = tokio::task::spawn_blocking(move || client.window_corner_radius()).await;
            let radius = match result {
                Ok(Ok(radius)) => radius,
                error => {
                    if !failed {
                        tracing::warn!(error_kind = "rounding_read_failed", retryable = true,
                            error = ?error, "Desktop rounding unavailable; retaining last valid value");
                    }
                    failed = true;
                    continue;
                }
            };
            if failed {
                tracing::debug!("Desktop rounding reader recovered");
                failed = false;
            }
            let snapshot = match state.0.lock() {
                Ok(mut value) => {
                    if value.window_corner_radius == Some(radius) {
                        continue;
                    }
                    value.window_corner_radius = Some(radius);
                    value.revision = value.revision.saturating_add(1);
                    value.clone()
                }
                Err(_) => {
                    tracing::warn!("Desktop theme state unavailable");
                    break;
                }
            };
            tracing::debug!(radius, "Desktop rounding changed");
            if let Err(error) = app.emit("desktop-theme://changed", snapshot) {
                tracing::warn!(%error, "Desktop rounding broadcast failed");
            }
        }
        tracing::debug!("Desktop rounding watcher stopped");
    }.instrument(span));
}
