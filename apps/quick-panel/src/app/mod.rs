//! The composition root: builds the adapters, wires them into the panel and runs the event loop.

pub mod double_tap;
pub mod hotkey;
mod lifecycle;
mod triggers;
mod window;

use std::sync::Arc;
use std::time::Duration;

use gpui::Application;
use quick_panel_core::ports::{HistoryService, HostLink};

use crate::adapters::daemon::DaemonHistory;
use crate::{adapters, platform};

pub fn run() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .with_writer(std::io::stderr)
        .init();
    let host = Arc::new(adapters::host::SupervisorLink::default());
    if std::env::args()
        .any(|argument| argument == uc_desktop::quick_panel_helper::EXIT_WHEN_STDIN_CLOSES)
    {
        host.mark_supervised();
        lifecycle::watch_parent(std::io::stdin(), || std::process::exit(0));
    }
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
