mod appearance;
mod backend;
mod filters;
mod image_geometry;
mod panel;
mod platform;
mod selection;
mod shortcuts;
mod window_pair;

use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
use gpui::{
    px, size, App, AppContext, Application, Bounds, WindowBounds, WindowKind, WindowOptions,
};
use gpui_component::Root;
use panel::Panel;
use std::time::Duration;

fn open_panel(
    cx: &mut App,
    runtime: tokio::runtime::Handle,
    message: Option<String>,
) -> anyhow::Result<(gpui::AnyWindowHandle, gpui::Entity<Panel>)> {
    let target = std::rc::Rc::new(platform::PasteTarget::capture());
    let bounds = Bounds::centered(
        None,
        size(
            px(window_pair::PANEL_WIDTH as f32),
            px(window_pair::PANEL_HEIGHT as f32),
        ),
        cx,
    );
    let mut panel_entity = None;
    let handle = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: None,
            kind: WindowKind::PopUp,
            is_resizable: false,
            window_min_size: Some(size(
                px(window_pair::PANEL_WIDTH as f32),
                px(window_pair::PANEL_HEIGHT as f32),
            )),
            window_background: gpui::WindowBackgroundAppearance::Transparent,
            ..Default::default()
        },
        |window, cx| {
            window.set_window_title("UniClipboard History");
            if appearance::apply(None, window, cx).is_err() {
                tracing::warn!("Could not apply quick panel theme");
            }
            let panel = cx.new(|cx| Panel::new(runtime, target, message, window, cx));
            panel_entity = Some(panel.clone());
            cx.new(|cx| Root::new(panel, window, cx))
        },
    )?;
    Ok((
        handle.into(),
        panel_entity.ok_or_else(|| anyhow::anyhow!("Panel was not created"))?,
    ))
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .with_writer(std::io::stderr)
        .init();
    let runtime = tokio::runtime::Runtime::new()?;
    let handle = runtime.handle().clone();
    Application::new()
        .with_assets(gpui_component_assets::Assets)
        .run(move |cx| {
            gpui_component::init(cx);
            cx.bind_keys([
                gpui::KeyBinding::new("cmd-c", gpui_component::input::Copy, Some("QuickPanel")),
                gpui::KeyBinding::new("ctrl-c", gpui_component::input::Copy, Some("QuickPanel")),
                gpui::KeyBinding::new("tab", panel::NextSuggestion, Some("QuickPanel")),
                gpui::KeyBinding::new("shift-tab", panel::PreviousSuggestion, Some("QuickPanel")),
            ]);
            if cx
                .text_system()
                .add_fonts(vec![
                    std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Inter.ttf")),
                    std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/JetBrainsMono.ttf")),
                ])
                .is_err()
            {
                tracing::warn!("Could not load quick panel fonts");
            }
            let manager = match shortcuts::Shortcuts::new() {
                Ok(manager) => manager,
                Err(_) => {
                    tracing::error!("Failed to initialize quick panel hotkey manager");
                    cx.quit();
                    return;
                }
            };
            cx.set_global(manager);
            let (window, panel) = match open_panel(cx, handle.clone(), None) {
                Ok(panel) => panel,
                Err(_) => {
                    tracing::error!("Failed to open GPUI quick panel");
                    cx.quit();
                    return;
                }
            };
            let (send, receive) = async_channel::unbounded();
            GlobalHotKeyEvent::set_event_handler(Some(move |event| {
                let _ = send.try_send(event);
            }));
            cx.spawn(async move |cx| {
                while let Ok(event) = receive.recv().await {
                    if event.state != HotKeyState::Pressed {
                        continue;
                    }
                    if cx
                        .update(|cx| {
                            if !cx.global_mut::<shortcuts::Shortcuts>().pressed(event.id) {
                                return;
                            }
                            if window
                                .update(cx, |_, window, cx| {
                                    panel.update(cx, |panel, cx| panel.toggle(window, cx))
                                })
                                .is_err()
                            {
                                tracing::warn!("Quick panel window already closed");
                            }
                        })
                        .is_err()
                    {
                        return;
                    }
                }
            })
            .detach();
        });
    runtime.shutdown_timeout(Duration::from_secs(1));
    Ok(())
}
