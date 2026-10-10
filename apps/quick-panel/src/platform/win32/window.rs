//! The native windows of the panel and of its preview.

use std::sync::atomic::{AtomicU64, Ordering};

use quick_panel_core::ports::PlatformError;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, ShowWindow, GWL_EXSTYLE, HWND_TOPMOST,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SW_HIDE,
    WS_EX_NOACTIVATE, WS_EX_TOPMOST,
};

use super::focus;

/// The display scale `screen::panel_anchor` last used, as `f64` bits. GPUI positions windows in
/// logical pixels, which on Windows are physical pixels divided by the scale of the display they
/// are on; the panel and its preview open on the display of the cursor, so one scale serves both.
pub(super) static ANCHOR_SCALE: AtomicU64 = AtomicU64::new(0);

fn hwnd_of(window: &gpui::Window, closed: PlatformError) -> Result<HWND, PlatformError> {
    let handle = HasWindowHandle::window_handle(window).map_err(|_| closed)?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return Err(PlatformError::UnsupportedWindowKind);
    };
    Ok(HWND(handle.hwnd.get() as *mut _))
}

/// Shows or hides the panel window.
///
/// The native calls are made after the current GPUI update, not inside it. `ShowWindow` and
/// `SetWindowPos` deliver window messages synchronously, and GPUI handles those by borrowing the
/// window state that the running update already holds: the result is "RefCell already borrowed",
/// lost activation and size notifications, and a search box that never gets the keyboard focus.
pub fn set_visible(
    window: &gpui::Window,
    visible: bool,
    cx: &gpui::App,
) -> Result<(), PlatformError> {
    let hwnd = hwnd_of(window, PlatformError::PanelWindowInaccessible)?;
    if !focus::is_window(hwnd) {
        return Err(PlatformError::PanelWindowClosed);
    }
    cx.foreground_executor()
        .spawn(async move {
            unsafe {
                if visible {
                    // Topmost, so the panel stays above the window it was opened over.
                    let _ = SetWindowPos(
                        hwnd,
                        Some(HWND_TOPMOST),
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
                    );
                    // The search box needs keyboard focus, which a hotkey press does not grant.
                    focus::activate(hwnd, HWND::default());
                } else {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                }
            }
        })
        .detach();
    Ok(())
}

/// Shows the preview above the panel without taking the focus from it, after the current update
/// (see [`set_visible`]).
///
/// The preview never becomes the active window: `WS_EX_NOACTIVATE` keeps a click on it from
/// activating it, and every call here and in [`set_frame`] passes `SWP_NOACTIVATE`, so the panel
/// stays active and the focus-left-the-panel rule does not fire.
pub fn show_without_focus(window: &gpui::Window, cx: &gpui::App) -> Result<(), PlatformError> {
    let hwnd = hwnd_of(window, PlatformError::PreviewWindowInaccessible)?;
    if !focus::is_window(hwnd) {
        return Err(PlatformError::PreviewWindowClosed);
    }
    cx.foreground_executor()
        .spawn(async move {
            unsafe {
                let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                let wanted = style | (WS_EX_NOACTIVATE.0 | WS_EX_TOPMOST.0) as isize;
                if wanted != style {
                    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, wanted);
                }
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
            }
        })
        .detach();
    Ok(())
}

pub fn set_frame(
    window: &mut gpui::Window,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    cx: &gpui::App,
) -> Result<(), PlatformError> {
    let hwnd = hwnd_of(window, PlatformError::PanelWindowInaccessible)?;
    let stored = f64::from_bits(ANCHOR_SCALE.load(Ordering::Relaxed));
    let scale = if stored > 0. {
        stored
    } else {
        f64::from(window.scale_factor())
    };
    // Resizing goes through `SetWindowPos` here, not `Window::resize`: GPUI's resize leaves out
    // `SWP_NOACTIVATE`, so resizing the preview after it is shown makes it the active window and
    // takes the keyboard from the search box. The call runs after the current update because
    // `SetWindowPos` delivers WM_SIZE and WM_MOVE synchronously, into the window being updated.
    let (left, top) = ((x * scale).round() as i32, (y * scale).round() as i32);
    let (cx_px, cy_px) = (
        (width * scale).round() as i32,
        (height * scale).round() as i32,
    );
    cx.foreground_executor()
        .spawn(async move {
            let _ = unsafe {
                SetWindowPos(
                    hwnd,
                    None,
                    left,
                    top,
                    cx_px,
                    cy_px,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                )
            };
        })
        .detach();
    Ok(())
}

/// Nothing to configure: the preview is an ordinary rectangular window here.
pub fn configure_shaped_preview(_: &gpui::Window, _: &gpui::App) -> Result<(), PlatformError> {
    Ok(())
}

/// Nothing to clip: see [`configure_shaped_preview`].
pub fn clip_preview_shape(
    _: &gpui::Window,
    _: quick_panel_core::geometry::window_pair::PreviewPlacement,
    _: f64,
    _: &gpui::App,
) -> Result<(), PlatformError> {
    Ok(())
}
