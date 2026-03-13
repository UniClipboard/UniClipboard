//! macOS NSPanel implementation for the quick clipboard panel.
//!
//! Uses NSPanel with `NonactivatingPanel` style mask — the standard macOS
//! mechanism (used by Spotlight, Alfred, Raycast, Maccy) that lets a panel
//! receive keyboard input without activating the owning application.
//!
//! macOS 快捷面板的 NSPanel 实现。使用 `NonactivatingPanel` 样式，
//! 这是 macOS 标准机制（Spotlight / Alfred / Raycast 均采用此方案），
//! 面板可接收键盘输入但不会激活宿主应用。

use core_graphics::event::{CGEvent, CGEventFlags, CGKeyCode};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use objc2::ffi::object_setClass;
use objc2::runtime::AnyObject;
use objc2::{define_class, ClassType, MainThreadMarker};
use objc2_app_kit::{NSEvent, NSPanel, NSScreen, NSWindowStyleMask};
use tauri::WebviewWindow;
use tracing::{error, info};

// Custom NSPanel subclass that overrides `canBecomeKeyWindow` to return YES.
// NSPanel without a title bar (`decorations: false`) returns NO by default,
// preventing all keyboard input. This subclass fixes that.
define_class!(
    #[unsafe(super(NSPanel))]
    #[name = "UCKeyablePanel"]
    struct UCKeyablePanel;

    impl UCKeyablePanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key_window(&self) -> bool {
            true
        }
    }
);

/// Convert a Tauri WebviewWindow's underlying NSWindow into a custom
/// NSPanel subclass (`UCKeyablePanel`) with `NonactivatingPanel` behavior.
///
/// # Safety contract
/// - NSPanel is a direct subclass of NSWindow with **no extra ivars**,
///   and UCKeyablePanel adds none either, so `object_setClass` is safe.
/// - Must be called from the **main thread** (ObjC UI requirement).
///
/// 将 Tauri WebviewWindow 的底层 NSWindow 转换为自定义 NSPanel 子类。
pub fn convert_to_panel(window: &WebviewWindow) {
    let ns_window = match window.ns_window() {
        Ok(ptr) => ptr,
        Err(e) => {
            error!(error = %e, "Failed to get ns_window pointer");
            return;
        }
    };

    unsafe {
        // 1. Swap the ObjC class from NSWindow → UCKeyablePanel.
        //    Safe because neither NSPanel nor our subclass adds instance variables.
        let panel_class = UCKeyablePanel::class();
        object_setClass(ns_window as *mut AnyObject, panel_class as *const _);

        // 2. Treat the same pointer as an NSPanel reference
        let panel: &NSPanel = &*(ns_window as *const NSPanel);

        // 3. Add NonactivatingPanel to the existing style mask
        let mut style = panel.styleMask();
        style |= NSWindowStyleMask::NonactivatingPanel;
        panel.setStyleMask(style);

        // 4. Configure panel behavior
        panel.setFloatingPanel(true); // Float above other windows
        panel.setBecomesKeyOnlyIfNeeded(false); // Accept keyboard input immediately
        panel.setHidesOnDeactivate(false); // Don't auto-hide on app deactivation
    }

    info!("Converted NSWindow → UCKeyablePanel with NonactivatingPanel");
}

/// Make the panel the key window so it receives keyboard input.
///
/// Uses `makeKeyWindow()` instead of Tauri's `set_focus()` to avoid
/// activating the app (which would steal focus from the previous app).
///
/// 将 panel 设为 key window 以接收键盘输入，不会激活宿主应用。
pub fn make_panel_key(window: &WebviewWindow) {
    let ns_window = match window.ns_window() {
        Ok(ptr) => ptr,
        Err(e) => {
            error!(error = %e, "Failed to get ns_window for make_panel_key");
            return;
        }
    };

    unsafe {
        let panel: &NSPanel = &*(ns_window as *const NSPanel);
        panel.makeKeyWindow();
    }
}

/// Get cursor position in screen coordinates (top-left origin).
///
/// macOS uses bottom-left origin; this converts to top-left for Tauri.
///
/// 获取光标位置（屏幕坐标，左上角原点）。
pub fn get_cursor_position() -> (f64, f64) {
    let point = NSEvent::mouseLocation();

    // Convert from macOS bottom-left origin to top-left origin.
    // MainThreadMarker is required by NSScreen::mainScreen; this function
    // is called from the main-thread shortcut handler, so the marker is valid.
    let screen_height = MainThreadMarker::new()
        .and_then(|mtm| {
            let screen = NSScreen::mainScreen(mtm)?;
            Some(screen.frame().size.height)
        })
        .unwrap_or(900.0);

    (point.x, screen_height - point.y)
}

/// Simulate Cmd+V paste keystroke via CoreGraphics CGEvent.
///
/// 通过 CoreGraphics CGEvent 模拟 Cmd+V 粘贴。
pub fn simulate_paste() {
    // macOS virtual key code for 'V'
    const KEY_V: CGKeyCode = 9;

    let source = match CGEventSource::new(CGEventSourceStateID::HIDSystemState) {
        Ok(s) => s,
        Err(_) => {
            error!("Failed to create CGEventSource");
            return;
        }
    };

    let key_down = match CGEvent::new_keyboard_event(source.clone(), KEY_V, true) {
        Ok(e) => e,
        Err(_) => {
            error!("Failed to create key-down CGEvent");
            return;
        }
    };
    key_down.set_flags(CGEventFlags::CGEventFlagCommand);

    let key_up = match CGEvent::new_keyboard_event(source, KEY_V, false) {
        Ok(e) => e,
        Err(_) => {
            error!("Failed to create key-up CGEvent");
            return;
        }
    };
    key_up.set_flags(CGEventFlags::CGEventFlagCommand);

    key_down.post(core_graphics::event::CGEventTapLocation::HID);
    key_up.post(core_graphics::event::CGEventTapLocation::HID);

    info!("Simulated Cmd+V paste keystroke");
}
