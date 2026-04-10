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
use objc2::{define_class, msg_send, ClassType};
use objc2_app_kit::{NSColor, NSPanel, NSWindowStyleMask};
use tauri::WebviewWindow;
use tracing::{error, info, warn};

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

        // 5. Window-level transparency (CSS handles rounded corners)
        make_panel_transparent(panel);
    }

    // 6. Disable WKWebView background drawing so CSS transparency works
    if let Err(e) = window.with_webview(|webview| unsafe {
        let wk: *mut AnyObject = webview.inner().cast();
        if !wk.is_null() {
            let _: () = msg_send![&*wk, _setDrawsBackground: false];
        }
    }) {
        warn!(error = %e, "Failed to set webview background transparent");
    }

    info!("Converted NSWindow → UCKeyablePanel with NonactivatingPanel");
}

/// Make a borderless NSPanel fully transparent so CSS handles all visuals.
///
/// Only sets window-level transparency. Rounded corners are handled by
/// CSS `rounded-xl overflow-hidden` on the root container — no native
/// `cornerRadius`/`masksToBounds` needed. Avoiding `setWantsLayer` on the
/// contentView prevents WKWebView repaint issues where mouse interaction
/// would cause the layer-backed compositing to draw an opaque background.
unsafe fn make_panel_transparent(panel: &NSPanel) {
    panel.setOpaque(false);
    panel.setBackgroundColor(Some(&NSColor::clearColor()));
}

/// Show the panel without activating the app.
///
/// Uses `orderFrontRegardless` instead of Tauri's `show()` to avoid
/// `NSApp.activate` which would bring the main window to the front.
///
/// 显示面板但不激活应用，避免主窗口被拉到前台。
pub fn show_panel(window: &WebviewWindow) {
    let ns_window = match window.ns_window() {
        Ok(ptr) => ptr,
        Err(e) => {
            error!(error = %e, "Failed to get ns_window for show_panel");
            return;
        }
    };

    unsafe {
        let panel: &NSPanel = &*(ns_window as *const NSPanel);
        panel.orderFrontRegardless();
        panel.makeKeyWindow();
    }
}

/// Simulate Cmd+V paste keystroke via CoreGraphics CGEvent.
///
/// 通过 CoreGraphics CGEvent 模拟 Cmd+V 粘贴。
pub fn simulate_paste() -> Result<(), String> {
    // macOS virtual key code for 'V'
    const KEY_V: CGKeyCode = 9;

    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|e| format!("Failed to create CGEventSource: {:?}", e))?;

    let key_down = CGEvent::new_keyboard_event(source.clone(), KEY_V, true)
        .map_err(|e| format!("Failed to create key-down CGEvent: {:?}", e))?;
    key_down.set_flags(CGEventFlags::CGEventFlagCommand);

    let key_up = CGEvent::new_keyboard_event(source, KEY_V, false)
        .map_err(|e| format!("Failed to create key-up CGEvent: {:?}", e))?;
    key_up.set_flags(CGEventFlags::CGEventFlagCommand);

    key_down.post(core_graphics::event::CGEventTapLocation::HID);
    key_up.post(core_graphics::event::CGEventTapLocation::HID);

    Ok(())
}
