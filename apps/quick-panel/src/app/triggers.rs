//! What opens and closes the panel: the global shortcut and the modifier double tap.
//!
//! Both feed one channel, and one task on GPUI's executor turns what arrives into a toggle of the
//! panel.

use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
use gpui::{App, AsyncApp};

use super::{double_tap, hotkey, test_control};
use crate::ui::Panel;

/// What asks the panel to open or close.
pub enum Trigger {
    Hotkey(GlobalHotKeyEvent),
    DoubleTap,
    /// A `toggle` line from the end-to-end tests, see `test_control`.
    TestControl,
}

/// Registers both triggers. The channel must exist before the panel opens, because the panel
/// applies the persisted double-tap modifier as soon as it loads settings.
pub fn install(cx: &mut App) -> anyhow::Result<async_channel::Receiver<Trigger>> {
    let manager = hotkey::Shortcuts::new()?;
    cx.set_global(manager);
    let (send, receive) = async_channel::unbounded();
    if test_control::enabled() {
        test_control::read_stdin(send.clone());
    }
    let hotkey_send = send.clone();
    GlobalHotKeyEvent::set_event_handler(Some(move |event| {
        let _ = hotkey_send.try_send(Trigger::Hotkey(event));
    }));
    cx.set_global(double_tap::DoubleTap(double_tap::new_monitor(move || {
        let _ = send.try_send(Trigger::DoubleTap);
    })));
    Ok(receive)
}

/// Toggles `panel` whenever a trigger fires, for as long as the application runs.
pub fn forward(
    cx: &mut App,
    receive: async_channel::Receiver<Trigger>,
    window: gpui::AnyWindowHandle,
    panel: gpui::Entity<Panel>,
) {
    cx.spawn(async move |cx: &mut AsyncApp| {
        while let Ok(trigger) = receive.recv().await {
            if let Trigger::Hotkey(event) = &trigger {
                if event.state != HotKeyState::Pressed {
                    continue;
                }
            }
            if cx
                .update(|cx| {
                    if let Trigger::Hotkey(event) = &trigger {
                        if !cx.global_mut::<hotkey::Shortcuts>().pressed(event.id) {
                            return;
                        }
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
}
