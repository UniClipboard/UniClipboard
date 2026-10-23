//! What opens and closes the panel: the global shortcut and the modifier double tap.
//!
//! Both feed one channel, and one task on GPUI's executor turns what arrives into a toggle of the
//! panel.

use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
use gpui::{App, AsyncApp};

use super::lifecycle::HostCommand;
use super::{double_tap, hotkey, test_control};
use crate::ui::Panel;

/// What asks the panel to open or close.
pub enum Trigger {
    Hotkey(GlobalHotKeyEvent),
    DoubleTap,
    /// A `toggle` line from the end-to-end tests, see `test_control`.
    TestControl,
    /// A command line from the supervising GUI, see `lifecycle::watch_host`.
    Host(HostCommand),
    #[cfg(target_os = "windows")]
    ForegroundChanged,
}

#[cfg(target_os = "windows")]
struct ForegroundGuard {
    _observer: crate::platform::ForegroundObserver,
}
#[cfg(target_os = "windows")]
impl gpui::Global for ForegroundGuard {}

/// Registers both triggers on the channel `send` feeds. The channel must exist before the panel
/// opens, because the panel applies the persisted double-tap modifier as soon as it loads settings.
pub fn install(
    cx: &mut App,
    send: async_channel::Sender<Trigger>,
    supervised: bool,
) -> anyhow::Result<()> {
    let manager = hotkey::Shortcuts::new(supervised)?;
    cx.set_global(manager);
    if test_control::enabled() {
        test_control::read_stdin(send.clone());
    }
    let hotkey_send = send.clone();
    GlobalHotKeyEvent::set_event_handler(Some(move |event| {
        let _ = hotkey_send.try_send(Trigger::Hotkey(event));
    }));
    #[cfg(target_os = "windows")]
    {
        let foreground_send = send.clone();
        cx.set_global(ForegroundGuard {
            _observer: crate::platform::ForegroundObserver::new(move || {
                let _ = foreground_send.try_send(Trigger::ForegroundChanged);
            })?,
        });
    }
    cx.set_global(double_tap::DoubleTap(double_tap::new_monitor(move || {
        let _ = send.try_send(Trigger::DoubleTap);
    })));
    Ok(())
}

/// Carries out a command of the supervising GUI on the UI thread, which owns the hotkey manager.
fn host_command(
    cx: &mut App,
    command: HostCommand,
    window: gpui::AnyWindowHandle,
    panel: &gpui::Entity<Panel>,
) {
    match command {
        HostCommand::SetShortcuts { seq, shortcuts } => {
            let shortcuts_now = cx.global_mut::<hotkey::Shortcuts>();
            let result = shortcuts_now.set_from_host(shortcuts);
            if let Err(error) = &result {
                tracing::warn!("The quick panel shortcut was refused: {error}");
            }
            let line = crate::adapters::host::shortcut_reply_line(
                seq,
                &shortcuts_now.held(),
                result.as_ref().err().map(String::as_str),
            );
            if crate::adapters::host::write_line(&line).is_err() {
                tracing::warn!("Could not answer the GUI");
            }
        }
        HostCommand::ReloadSettings => {
            if window
                .update(cx, |_, window, cx| {
                    panel.update(cx, |panel, cx| panel.reload_settings(window, cx))
                })
                .is_err()
            {
                tracing::warn!("Quick panel window already closed");
            }
        }
    }
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
            #[cfg(target_os = "windows")]
            if matches!(trigger, Trigger::ForegroundChanged) {
                let _ = window.update(cx, |_, window, cx| {
                    panel.update(cx, |panel, cx| panel.foreground_changed(window, cx));
                });
                continue;
            }
            if let Trigger::Host(command) = trigger {
                if cx
                    .update(|cx| host_command(cx, command, window, &panel))
                    .is_err()
                {
                    return;
                }
                continue;
            }
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
