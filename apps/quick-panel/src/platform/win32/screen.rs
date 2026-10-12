//! Where the panel goes on the screens.

use std::sync::atomic::Ordering;

use quick_panel_core::geometry::placement::SessionScreen;
use quick_panel_core::geometry::window_pair::Rect;
use quick_panel_core::ports::PlatformError;
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

use super::window::ANCHOR_SCALE;

/// The pointer and the work area of the display that holds it, in GPUI's logical pixels.
///
/// The work area leaves out the taskbar, so neither window covers it. Logical pixels are physical
/// pixels divided by the scale of that display; the same scale is remembered for `set_frame`, so
/// both windows of the pair are placed in one coordinate system.
pub fn session_screen() -> Result<SessionScreen, PlatformError> {
    let mut cursor = POINT::default();
    unsafe { GetCursorPos(&mut cursor) }.map_err(|_| PlatformError::NoDisplay)?;
    let monitor = unsafe { MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return Err(PlatformError::NoDisplay);
    }
    let (mut dpi_x, mut dpi_y) = (96_u32, 96_u32);
    // A failure leaves the 96 dpi default, which is the right scale for a display that has none.
    let _ = unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
    let scale = f64::from(dpi_x.max(96)) / 96.;
    ANCHOR_SCALE.store(scale.to_bits(), Ordering::Relaxed);

    let work = info.rcWork;
    Ok(SessionScreen {
        pointer: (f64::from(cursor.x) / scale, f64::from(cursor.y) / scale),
        work: Rect {
            x: f64::from(work.left) / scale,
            y: f64::from(work.top) / scale,
            width: f64::from(work.right - work.left) / scale,
            height: f64::from(work.bottom - work.top) / scale,
        },
    })
}
