//! The composition root: builds the adapters, wires them into the panel and runs the event loop.

pub mod double_tap;
pub mod hotkey;
mod lifecycle;
pub mod test_control;
mod triggers;
mod window;

use std::sync::Arc;
use std::time::Duration;

use gpui::Application;
use quick_panel_core::ports::{HistoryService, HostLink};

use crate::adapters::daemon::DaemonHistory;
use crate::{adapters, platform};

/// Warnings go to standard error, which the supervising GUI forwards to its own log. Standard
/// output carries the host protocol, so it is never used for logs. The same events at the
/// profile's level also go to the per-role log file (`uniclipboard-quick-panel.json.<date>`), so
/// placement and surface diagnostics survive the session. The guard must outlive the process work.
fn init_logging() -> Option<uc_observability::WorkerGuard> {
    use tracing_subscriber::prelude::*;
    let stderr = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_filter(tracing_subscriber::filter::LevelFilter::WARN);
    let file = uc_app_paths::app_log_dir().and_then(|dir| {
        uc_observability::build_json_layer(&dir, &uc_observability::LogProfile::from_env()).ok()
    });
    match file {
        Some((layer, guard)) => {
            let _ = tracing_subscriber::registry()
                .with(stderr)
                .with(layer)
                .try_init();
            Some(guard)
        }
        None => {
            let _ = tracing_subscriber::registry().with(stderr).try_init();
            None
        }
    }
}

pub fn run() -> anyhow::Result<()> {
    let _log_guard = init_logging();
    let host = Arc::new(adapters::host::SupervisorLink::default());
    if std::env::args().any(|argument| argument == lifecycle::EXIT_WHEN_STDIN_CLOSES) {
        host.mark_supervised();
        // Under test control standard input carries commands, and its reader exits on its end.
        if !test_control::enabled() {
            lifecycle::watch_parent(std::io::stdin(), || std::process::exit(0));
        }
    }
    // Until the settings are read the panel follows the system, as the main window does for an
    // unset language; the search box takes its placeholder from this when it is created.
    crate::ui::apply_language(None);
    let runtime = tokio::runtime::Runtime::new()?;
    let handle = runtime.handle().clone();
    let history: Arc<dyn HistoryService> = Arc::new(DaemonHistory::new());
    let host: Arc<dyn HostLink> = host;
    Application::new()
        .with_assets(gpui_component_assets::Assets)
        .run(move |cx| {
            platform::run_as_background_app();
            gpui_component::init(cx);
            cx.bind_keys([
                gpui::KeyBinding::new("cmd-c", gpui_component::input::Copy, Some("QuickPanel")),
                gpui::KeyBinding::new("ctrl-c", gpui_component::input::Copy, Some("QuickPanel")),
                gpui::KeyBinding::new("tab", crate::ui::NextSuggestion, Some("QuickPanel")),
                gpui::KeyBinding::new(
                    "shift-tab",
                    crate::ui::PreviousSuggestion,
                    Some("QuickPanel"),
                ),
            ]);
            let receive = match triggers::install(cx) {
                Ok(receive) => receive,
                Err(_) => {
                    tracing::error!("Failed to initialize quick panel hotkey manager");
                    cx.quit();
                    return;
                }
            };
            let (window, panel) =
                match window::open_panel(cx, handle.clone(), history, host.clone(), None) {
                    Ok(panel) => panel,
                    Err(_) => {
                        tracing::error!("Failed to open GPUI quick panel");
                        cx.quit();
                        return;
                    }
                };
            triggers::forward(cx, receive, window, panel);
        });
    runtime.shutdown_timeout(Duration::from_secs(1));
    Ok(())
}
