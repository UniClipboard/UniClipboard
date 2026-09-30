//! Where the panel goes on the screens.

use quick_panel_core::ports::PlatformError;

pub fn panel_anchor(
    cursor_anchored: bool,
    width: f64,
    height: f64,
) -> Result<(f64, f64), PlatformError> {
    use objc2_app_kit::{NSEvent, NSScreen};
    let screens =
        NSScreen::screens(objc2::MainThreadMarker::new().ok_or(PlatformError::MainThreadRequired)?);
    let primary = screens.firstObject().ok_or(PlatformError::NoDisplay)?;
    let pointer = NSEvent::mouseLocation();
    let screen = screens
        .iter()
        .find(|screen| {
            let frame = screen.frame();
            pointer.x >= frame.origin.x
                && pointer.x < frame.origin.x + frame.size.width
                && pointer.y >= frame.origin.y
                && pointer.y < frame.origin.y + frame.size.height
        })
        .unwrap_or(primary.clone());
    let frame = screen.frame();
    let top = primary.frame().size.height - frame.origin.y - frame.size.height;
    if cursor_anchored {
        let axis = |position: f64, origin: f64, extent: f64, panel: f64| {
            if position + 6. + panel <= origin + extent {
                position + 6.
            } else if position - 6. - panel >= origin {
                position - 6. - panel
            } else {
                (origin + extent - panel).max(origin)
            }
        };
        Ok((
            axis(pointer.x, frame.origin.x, frame.size.width, width),
            axis(
                primary.frame().size.height - pointer.y,
                top,
                frame.size.height,
                height,
            ),
        ))
    } else {
        Ok((
            frame.origin.x + (frame.size.width - width) / 2.,
            top + (frame.size.height - height) / 2.,
        ))
    }
}
