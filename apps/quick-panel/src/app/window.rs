//! Opening the panel window.

use std::sync::Arc;

use gpui::{px, size, App, AppContext, Bounds, WindowBounds, WindowKind, WindowOptions};
use gpui_component::Root;
use quick_panel_core::geometry::window_pair::{PANEL_HEIGHT, PANEL_WIDTH};
use quick_panel_core::ports::{HistoryService, HostLink};

use crate::platform;
use crate::ui::{appearance, Panel};

/// Opens the panel window, hidden. It is shown when the shortcut or the double tap asks for it.
pub fn open_panel(
    cx: &mut App,
    runtime: tokio::runtime::Handle,
    history: Arc<dyn HistoryService>,
    host: Arc<dyn HostLink>,
    message: Option<String>,
) -> anyhow::Result<(gpui::AnyWindowHandle, gpui::Entity<Panel>)> {
    let target = platform::capture_paste_target();
    let bounds = Bounds::centered(
        None,
        size(px(PANEL_WIDTH as f32), px(PANEL_HEIGHT as f32)),
        cx,
    );
    let mut panel_entity = None;
    let handle = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: None,
            kind: WindowKind::PopUp,
            is_resizable: false,
            window_min_size: Some(size(px(PANEL_WIDTH as f32), px(PANEL_HEIGHT as f32))),
            window_background: gpui::WindowBackgroundAppearance::Transparent,
            // The panel stays hidden until the shortcut or the double tap asks for it. Showing it
            // at startup would pop it up whenever the GUI starts the helper, and its first frame
            // would use the system theme before the settings arrive.
            show: false,
            focus: false,
            ..Default::default()
        },
        |window, cx| {
            window.set_window_title("UniClipboard History");
            if appearance::apply(None, window, cx).is_err() {
                tracing::warn!("Could not apply quick panel theme");
            }
            let panel =
                cx.new(|cx| Panel::new(runtime, history, host, target, message, window, cx));
            panel_entity = Some(panel.clone());
            cx.new(|cx| Root::new(panel, window, cx))
        },
    )?;
    Ok((
        handle.into(),
        panel_entity.ok_or_else(|| anyhow::anyhow!("Panel was not created"))?,
    ))
}
