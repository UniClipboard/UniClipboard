//! The native windows of the panel and of its preview.

use quick_panel_core::ports::PlatformError;

use super::app::activate_app;

pub fn set_visible(window: &gpui::Window, visible: bool) -> Result<(), PlatformError> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window)
        .map_err(|_| PlatformError::PanelWindowInaccessible)?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return Err(PlatformError::UnsupportedWindowKind);
    };
    // GPUI owns the view; access is synchronous on its UI thread.
    let view = unsafe { &*handle.ns_view.as_ptr().cast::<objc2_app_kit::NSView>() };
    let native = view.window().ok_or(PlatformError::PanelWindowClosed)?;
    native.setHasShadow(true);
    if visible {
        native.orderFrontRegardless();
        // An input method only attaches to the active application, so the panel has to be it while
        // it is open. The paste target is put back in front before anything is pasted, and when the
        // panel closes (see `PasteTarget::return_focus`).
        activate_app();
        native.makeKeyWindow();
    } else {
        native.orderOut(None);
    }
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
    use objc2_foundation::NSPoint;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window)
        .map_err(|_| PlatformError::PanelWindowInaccessible)?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return Err(PlatformError::UnsupportedWindowKind);
    };
    let view = unsafe { &*handle.ns_view.as_ptr().cast::<objc2_app_kit::NSView>() };
    let native = view.window().ok_or(PlatformError::PanelWindowClosed)?;
    let screen = objc2_app_kit::NSScreen::screens(
        objc2::MainThreadMarker::new().ok_or(PlatformError::MainThreadRequired)?,
    )
    .firstObject()
    .ok_or(PlatformError::NoDisplay)?;
    let top = screen.frame().size.height - y;
    // GPUI defers content resizing so its resize callback can update the viewport
    // after the current entity/window update has released its borrow.
    window.resize(gpui::size(gpui::px(width as f32), gpui::px(height as f32)));
    cx.foreground_executor()
        .spawn(async move {
            native.setFrameTopLeftPoint(NSPoint::new(x, top));
        })
        .detach();
    Ok(())
}

pub fn configure_shaped_preview(
    window: &gpui::Window,
    cx: &gpui::App,
) -> Result<(), PlatformError> {
    use objc2_app_kit::{NSColor, NSWindowStyleMask};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window)
        .map_err(|_| PlatformError::PreviewWindowInaccessible)?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return Err(PlatformError::UnsupportedWindowKind);
    };
    let view = unsafe { &*handle.ns_view.as_ptr().cast::<objc2_app_kit::NSView>() };
    let native = view.window().ok_or(PlatformError::PreviewWindowClosed)?;
    // GPUI's titlebar=None still creates a titled, full-size-content NSPanel.
    // A genuinely borderless host lets the painted card-and-pointer alpha
    // define its outline and native shadow, including the transparent gutter.
    // Defer AppKit changes until GPUI releases the current window borrow.
    cx.foreground_executor()
        .spawn(async move {
            native.setStyleMask(NSWindowStyleMask::NonactivatingPanel);
            native.setOpaque(false);
            native.setBackgroundColor(Some(&NSColor::clearColor()));
            native.setHasShadow(true);
            native.invalidateShadow();
        })
        .detach();
    Ok(())
}

pub fn clip_preview_shape(
    window: &gpui::Window,
    placement: quick_panel_core::geometry::window_pair::PreviewPlacement,
    scale: f64,
    cx: &gpui::App,
) -> Result<(), PlatformError> {
    use objc2_app_kit::NSBezierPath;
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    use objc2_quartz_core::{CAShapeLayer, CATransaction};
    use quick_panel_core::geometry::window_pair::{
        PreviewSide, POINTER_DEPTH, POINTER_HALF_HEIGHT, PREVIEW_CORNER_RADIUS,
    };
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window)
        .map_err(|_| PlatformError::PreviewWindowInaccessible)?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return Err(PlatformError::UnsupportedWindowKind);
    };
    let view = unsafe { &*handle.ns_view.as_ptr().cast::<objc2_app_kit::NSView>() };
    let layer = view.layer().ok_or(PlatformError::PreviewLayerNotReady)?;
    let native = view.window().ok_or(PlatformError::PreviewWindowClosed)?;
    let display_scale = f64::from(window.scale_factor());
    cx.foreground_executor()
        .spawn(async move {
            let width = placement.frame.width;
            let height = placement.frame.height;
            let depth = POINTER_DEPTH * scale;
            let half = POINTER_HALF_HEIGHT * scale;
            let x = if placement.side == PreviewSide::Right {
                depth
            } else {
                0.
            };
            let y = if layer.isGeometryFlipped() {
                placement.pointer_y
            } else {
                height - placement.pointer_y
            };
            let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                NSRect::new(NSPoint::new(x, 0.), NSSize::new(width - depth, height)),
                PREVIEW_CORNER_RADIUS * scale,
                PREVIEW_CORNER_RADIUS * scale,
            );
            let (base, tip) = if placement.side == PreviewSide::Right {
                (depth, 0.)
            } else {
                (width - depth, width)
            };
            path.moveToPoint(NSPoint::new(base, y - half));
            path.lineToPoint(NSPoint::new(tip, y));
            path.lineToPoint(NSPoint::new(base, y + half));
            path.closePath();
            let mask = CAShapeLayer::layer();
            mask.setFrame(NSRect::new(
                NSPoint::new(0., 0.),
                NSSize::new(width, height),
            ));
            mask.setContentsScale(display_scale);
            mask.setPath(Some(&path.CGPath()));
            CATransaction::begin();
            CATransaction::setDisableActions(true);
            // The mask is retained by the layer and contains no parent references.
            unsafe {
                layer.setMask(Some(&mask));
            }
            CATransaction::commit();
            native.invalidateShadow();
        })
        .detach();
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn clip_preview_shape(
    _: &gpui::Window,
    _: quick_panel_core::geometry::window_pair::PreviewPlacement,
    _: f64,
    _: &gpui::App,
) -> Result<(), PlatformError> {
    Ok(())
}

pub fn show_without_focus(window: &gpui::Window) -> Result<(), PlatformError> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window)
        .map_err(|_| PlatformError::PreviewWindowInaccessible)?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return Err(PlatformError::UnsupportedWindowKind);
    };
    let view = unsafe { &*handle.ns_view.as_ptr().cast::<objc2_app_kit::NSView>() };
    let native = view.window().ok_or(PlatformError::PreviewWindowClosed)?;
    native.setHasShadow(true);
    native.orderFrontRegardless();
    Ok(())
}
