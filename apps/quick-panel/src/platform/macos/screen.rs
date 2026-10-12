//! The display a show session lays its windows out on.

use quick_panel_core::geometry::placement::SessionScreen;
use quick_panel_core::geometry::window_pair::Rect;
use quick_panel_core::ports::PlatformError;

/// The pointer and the usable area (visible frame, without menu bar and Dock) of the display that
/// holds it, in GPUI's global logical pixels: origin at the top-left of the primary display.
///
/// AppKit's global space has its origin at the bottom-left of the primary display, so both are
/// flipped with the primary display's height. A pointer outside every display (for example while
/// displays are being rearranged) uses the nearest one.
pub fn session_screen() -> Result<SessionScreen, PlatformError> {
    use objc2_app_kit::{NSEvent, NSScreen};
    let screens =
        NSScreen::screens(objc2::MainThreadMarker::new().ok_or(PlatformError::MainThreadRequired)?);
    let primary = screens.firstObject().ok_or(PlatformError::NoDisplay)?;
    let primary_height = primary.frame().size.height;
    let pointer = NSEvent::mouseLocation();
    let distance = |screen: &objc2_app_kit::NSScreen| {
        let frame = screen.frame();
        let dx = (frame.origin.x - pointer.x)
            .max(pointer.x - (frame.origin.x + frame.size.width))
            .max(0.);
        let dy = (frame.origin.y - pointer.y)
            .max(pointer.y - (frame.origin.y + frame.size.height))
            .max(0.);
        dx.hypot(dy)
    };
    let screen = screens
        .iter()
        .min_by(|a, b| distance(a).total_cmp(&distance(b)))
        .unwrap_or(primary);
    let visible = screen.visibleFrame();
    Ok(SessionScreen {
        pointer: (pointer.x, primary_height - pointer.y),
        work: Rect {
            x: visible.origin.x,
            y: primary_height - visible.origin.y - visible.size.height,
            width: visible.size.width,
            height: visible.size.height,
        },
    })
}
