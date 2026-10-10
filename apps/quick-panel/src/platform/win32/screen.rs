//! Where the panel goes on the screens.

use std::sync::atomic::Ordering;

use quick_panel_core::ports::PlatformError;
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

use super::window::ANCHOR_SCALE;

/// The top-left corner of the panel in GPUI's logical pixels, on the display that has the cursor.
///
/// Placement uses the work area, so the panel never covers the taskbar.
pub fn panel_anchor(
    cursor_anchored: bool,
    width: f64,
    height: f64,
) -> Result<(f64, f64), PlatformError> {
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
    let origin = (f64::from(work.left) / scale, f64::from(work.top) / scale);
    let extent = (
        f64::from(work.right - work.left) / scale,
        f64::from(work.bottom - work.top) / scale,
    );
    if cursor_anchored {
        Ok((
            anchored_axis(f64::from(cursor.x) / scale, origin.0, extent.0, width),
            anchored_axis(f64::from(cursor.y) / scale, origin.1, extent.1, height),
        ))
    } else {
        Ok((
            origin.0 + (extent.0 - width) / 2.,
            origin.1 + (extent.1 - height) / 2.,
        ))
    }
}

/// Puts the panel just after the cursor, before it when there is no room, and inside the display
/// either way.
fn anchored_axis(position: f64, origin: f64, extent: f64, panel: f64) -> f64 {
    const GAP: f64 = 6.;
    if position + GAP + panel <= origin + extent {
        position + GAP
    } else if position - GAP - panel >= origin {
        position - GAP - panel
    } else {
        (origin + extent - panel).max(origin)
    }
}
