//! Hold-modifier-key trigger for the quick panel.
//!
//! Monitors global key events and opens the quick panel when the platform
//! modifier key (Cmd on macOS, Ctrl on others) is held for 2 seconds
//! continuously without any other key or mouse button being pressed.
//!
//! State machine: `Idle → Holding → Triggered`
//! - Idle → Holding: modifier-only flags detected
//! - Holding → Triggered: 2 seconds elapsed with modifier still held
//! - Holding → Idle: other key pressed, mouse button pressed, or modifier released
//! - Triggered → Idle: modifier released (dismisses panel)
//!
//! On macOS uses `core-graphics` CGEventTap directly (avoids `rdev`'s
//! TSMGetInputSourceProperty crash on non-main threads).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::{Emitter, Listener};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

/// How long the modifier must be held before triggering.
const HOLD_DURATION: Duration = Duration::from_secs(2);

/// The label of the quick panel window.
const PANEL_LABEL: &str = "quick-panel";

/// State machine for the hold trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    Holding,
    Triggered,
}

/// Simplified event type sent from the platform listener to the state machine.
#[derive(Debug, Clone, Copy)]
enum HoldEvent {
    /// Only modifier key(s) are currently held (no other keys).
    ModifierOnlyDown,
    /// All modifier keys have been released.
    ModifierReleased,
    /// A non-modifier key was pressed.
    OtherKeyDown,
    /// A mouse button was pressed.
    MouseButtonDown,
}

/// Start the hold-trigger listener.
///
/// Spawns a native thread for global event monitoring and a tokio task for the
/// state machine. The `initial_enabled` flag controls whether the feature is
/// active at startup; subsequent changes are picked up by listening to the
/// `setting-changed` Tauri event.
pub fn start(app_handle: tauri::AppHandle, initial_enabled: bool) {
    let enabled = Arc::new(AtomicBool::new(initial_enabled));

    // Listen for setting changes to toggle the feature at runtime.
    let enabled_for_event = enabled.clone();
    app_handle.listen_any("setting-changed", move |event: tauri::Event| {
        if let Ok(payload) = serde_json::from_str::<serde_json::Value>(event.payload()) {
            if let Some(setting_json) = payload.get("settingJson").and_then(|v| v.as_str()) {
                if let Ok(settings) = serde_json::from_str::<serde_json::Value>(setting_json) {
                    if let Some(val) = settings
                        .get("general")
                        .and_then(|g| g.get("hold_modifier_to_open_quick_panel"))
                        .and_then(|v| v.as_bool())
                    {
                        let prev = enabled_for_event.swap(val, Ordering::Relaxed);
                        if prev != val {
                            info!(enabled = val, "Hold-modifier trigger toggled");
                        }
                    }
                }
            }
        }
    });

    // Channel from platform listener thread → tokio state machine task.
    let (tx, rx) = mpsc::unbounded_channel::<HoldEvent>();

    // Spawn platform-specific listener thread.
    start_platform_listener(tx);

    // Tokio task: state machine.
    let app = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        run_state_machine(rx, app, enabled).await;
    });

    info!(initial_enabled, "Hold-modifier trigger started");
}

// ── macOS: CGEventTap ─────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn start_platform_listener(tx: mpsc::UnboundedSender<HoldEvent>) {
    use core_graphics::event::{
        CGEvent, CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions,
        CGEventTapPlacement, CGEventType,
    };

    if let Err(e) = std::thread::Builder::new()
        .name("hold-trigger-cg".into())
        .spawn(move || {
            let events_of_interest = vec![
                CGEventType::FlagsChanged,
                CGEventType::KeyDown,
                CGEventType::LeftMouseDown,
                CGEventType::RightMouseDown,
                CGEventType::OtherMouseDown,
            ];

            let tap = match CGEventTap::new(
                CGEventTapLocation::Session,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::ListenOnly,
                events_of_interest,
                |_proxy, event_type, event: &CGEvent| -> Option<CGEvent> {
                    match event_type {
                        CGEventType::FlagsChanged => {
                            let flags = event.get_flags();
                            let cmd_held = flags.contains(CGEventFlags::CGEventFlagCommand);
                            let ctrl_held = flags.contains(CGEventFlags::CGEventFlagControl);
                            let shift_held = flags.contains(CGEventFlags::CGEventFlagShift);
                            let alt_held = flags.contains(CGEventFlags::CGEventFlagAlternate);

                            // We want ONLY Cmd (macOS modifier) held, nothing else.
                            if cmd_held && !ctrl_held && !shift_held && !alt_held {
                                let _ = tx.send(HoldEvent::ModifierOnlyDown);
                            } else if !cmd_held {
                                let _ = tx.send(HoldEvent::ModifierReleased);
                            } else {
                                // Cmd + other modifiers = treat as other key
                                let _ = tx.send(HoldEvent::OtherKeyDown);
                            }
                        }
                        CGEventType::KeyDown => {
                            let _ = tx.send(HoldEvent::OtherKeyDown);
                        }
                        CGEventType::LeftMouseDown
                        | CGEventType::RightMouseDown
                        | CGEventType::OtherMouseDown => {
                            let _ = tx.send(HoldEvent::MouseButtonDown);
                        }
                        _ => {}
                    }
                    None // ListenOnly — we never modify events
                },
            ) {
                Ok(tap) => tap,
                Err(_) => {
                    warn!("Failed to create CGEventTap for hold-trigger (accessibility permission may be needed)");
                    return;
                }
            };

            let loop_source = tap.mach_port.create_runloop_source(0);
            match loop_source {
                Ok(source) => unsafe {
                    let current = core_foundation::runloop::CFRunLoop::get_current();
                    current.add_source(
                        &source,
                        core_foundation::runloop::kCFRunLoopCommonModes,
                    );
                    tap.enable();
                    core_foundation::runloop::CFRunLoop::run_current();
                },
                Err(_) => {
                    warn!("Failed to create CFRunLoopSource for CGEventTap");
                }
            }
        })
    {
        warn!(error = %e, "Failed to spawn CGEventTap listener thread");
    }
}

// ── Non-macOS: rdev ───────────────────────────────────────────────────

#[cfg(not(target_os = "macos"))]
fn start_platform_listener(tx: mpsc::UnboundedSender<HoldEvent>) {
    if let Err(e) = std::thread::Builder::new()
        .name("hold-trigger-rdev".into())
        .spawn(move || {
            if let Err(e) = rdev::listen(move |event| {
                let hold_event = match event.event_type {
                    rdev::EventType::KeyPress(key) => {
                        if matches!(key, rdev::Key::ControlLeft | rdev::Key::ControlRight) {
                            Some(HoldEvent::ModifierOnlyDown)
                        } else {
                            Some(HoldEvent::OtherKeyDown)
                        }
                    }
                    rdev::EventType::KeyRelease(key) => {
                        if matches!(key, rdev::Key::ControlLeft | rdev::Key::ControlRight) {
                            Some(HoldEvent::ModifierReleased)
                        } else {
                            None
                        }
                    }
                    rdev::EventType::ButtonPress(_) => Some(HoldEvent::MouseButtonDown),
                    _ => None,
                };
                if let Some(ev) = hold_event {
                    let _ = tx.send(ev);
                }
            }) {
                warn!(error = ?e, "rdev::listen exited with error");
            }
        })
    {
        warn!(error = %e, "Failed to spawn rdev listener thread");
    }
}

// ── State machine ─────────────────────────────────────────────────────

/// Run the hold-trigger state machine.
async fn run_state_machine(
    mut rx: mpsc::UnboundedReceiver<HoldEvent>,
    app: tauri::AppHandle,
    enabled: Arc<AtomicBool>,
) {
    let mut state = State::Idle;

    loop {
        match state {
            State::Idle => {
                let Some(event) = rx.recv().await else {
                    debug!("Hold-trigger channel closed, exiting");
                    return;
                };

                if !enabled.load(Ordering::Relaxed) {
                    continue;
                }

                if matches!(event, HoldEvent::ModifierOnlyDown) {
                    state = State::Holding;
                    debug!("Hold trigger: Idle → Holding");
                }
            }

            State::Holding => {
                match tokio::time::timeout(HOLD_DURATION, rx.recv()).await {
                    // Timeout: modifier held for 2 seconds — trigger!
                    Err(_elapsed) => {
                        if !enabled.load(Ordering::Relaxed) {
                            state = State::Idle;
                            continue;
                        }

                        // Don't trigger if panel is already visible.
                        if super::is_visible(&app) {
                            debug!("Hold trigger: panel already visible, going to Triggered");
                            state = State::Triggered;
                            continue;
                        }

                        debug!("Hold trigger: Holding → Triggered (showing panel)");
                        let app_clone = app.clone();
                        if let Err(e) = app.run_on_main_thread(move || {
                            super::show(&app_clone);
                        }) {
                            warn!(error = %e, "Failed to show quick panel from hold trigger");
                            state = State::Idle;
                            continue;
                        }

                        // Emit hold-mode event so the frontend shows hold-mode hints.
                        if let Err(e) = app.emit_to(PANEL_LABEL, "quick-panel://hold-mode", ()) {
                            warn!(error = %e, "Failed to emit hold-mode event");
                        }

                        state = State::Triggered;
                    }

                    // Channel closed.
                    Ok(None) => {
                        debug!("Hold-trigger channel closed, exiting");
                        return;
                    }

                    // Received an event before timeout.
                    Ok(Some(event)) => match event {
                        HoldEvent::ModifierReleased => {
                            debug!("Hold trigger: Holding → Idle (modifier released early)");
                            state = State::Idle;
                        }
                        HoldEvent::OtherKeyDown => {
                            debug!("Hold trigger: Holding → Idle (other key pressed)");
                            state = State::Idle;
                        }
                        HoldEvent::MouseButtonDown => {
                            debug!("Hold trigger: Holding → Idle (mouse button pressed)");
                            state = State::Idle;
                        }
                        HoldEvent::ModifierOnlyDown => {
                            // Repeated modifier event (e.g. key repeat) — stay Holding.
                        }
                    },
                }
            }

            State::Triggered => {
                let Some(event) = rx.recv().await else {
                    debug!("Hold-trigger channel closed, exiting");
                    return;
                };

                if let HoldEvent::ModifierReleased = event {
                    debug!("Hold trigger: Triggered → Idle (modifier released, dismissing)");
                    let app_clone = app.clone();
                    if let Err(e) = app.run_on_main_thread(move || {
                        super::dismiss(&app_clone);
                    }) {
                        warn!(error = %e, "Failed to dismiss quick panel from hold trigger");
                    }
                    state = State::Idle;
                }
            }
        }
    }
}
