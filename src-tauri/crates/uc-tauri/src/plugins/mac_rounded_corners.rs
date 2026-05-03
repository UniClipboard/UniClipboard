use tauri::{AppHandle, Runtime, WebviewWindow};

#[cfg(target_os = "macos")]
use objc2::runtime::Bool;

#[cfg(target_os = "macos")]
use objc2_app_kit::{NSWindowStyleMask, NSWindowTitleVisibility};

#[cfg(target_os = "macos")]
use objc2_foundation::{NSPoint, NSRect};

#[cfg(target_os = "macos")]
use objc::{msg_send, sel, sel_impl};

#[cfg(target_os = "macos")]
type ObjcId = *mut objc::runtime::Object;

/// Configuration for Traffic Lights positioning
pub struct TrafficLightsConfig {
    /// Offset in pixels from default position (positive = right, negative = left)
    pub offset_x: f64,
    /// Offset in pixels from default position (positive = down, negative = up)
    pub offset_y: f64,
}

impl Default for TrafficLightsConfig {
    fn default() -> Self {
        Self {
            offset_x: 0.0,
            offset_y: 0.0,
        }
    }
}

/// Enables rounded corners for the window (macOS only)
/// Uses only public APIs - App Store compatible
#[tauri::command]
pub fn enable_rounded_corners<R: Runtime>(
    _app: AppHandle<R>,
    window: WebviewWindow<R>,
    offset_x: Option<f64>,
    offset_y: Option<f64>,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let config = TrafficLightsConfig {
            offset_x: offset_x.unwrap_or(0.0),
            offset_y: offset_y.unwrap_or(0.0),
        };

        window
            .with_webview(move |webview| {
                #[cfg(target_os = "macos")]
                unsafe {
                    let ns_window = webview.ns_window() as ObjcId;

                    let mut style_mask: NSWindowStyleMask = msg_send![ns_window, styleMask];

                    // Add necessary styles for rounded corners
                    style_mask |= NSWindowStyleMask::FullSizeContentView;
                    style_mask |= NSWindowStyleMask::Titled;
                    style_mask |= NSWindowStyleMask::Closable;
                    style_mask |= NSWindowStyleMask::Miniaturizable;
                    style_mask |= NSWindowStyleMask::Resizable;

                    let _: () = msg_send![ns_window, setStyleMask: style_mask];
                    let _: () = msg_send![ns_window, setTitlebarAppearsTransparent: Bool::YES];

                    let content_view: ObjcId = msg_send![ns_window, contentView];
                    let _: () = msg_send![content_view, setWantsLayer: Bool::YES];

                    position_traffic_lights(ns_window, config.offset_x, config.offset_y);
                }
            })
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    {
        Ok(())
    }
}

/// Enables modern window style with rounded corners and shadow
#[tauri::command]
pub fn enable_modern_window_style<R: Runtime>(
    _app: AppHandle<R>,
    window: WebviewWindow<R>,
    corner_radius: Option<f64>,
    offset_x: Option<f64>,
    offset_y: Option<f64>,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let config = TrafficLightsConfig {
            offset_x: offset_x.unwrap_or(0.0),
            offset_y: offset_y.unwrap_or(0.0),
        };
        let radius = corner_radius.unwrap_or(12.0);

        window
            .with_webview(move |webview| {
                #[cfg(target_os = "macos")]
                unsafe {
                    let ns_window = webview.ns_window() as ObjcId;

                    let mut style_mask: NSWindowStyleMask = msg_send![ns_window, styleMask];

                    style_mask |= NSWindowStyleMask::FullSizeContentView;
                    style_mask |= NSWindowStyleMask::Titled;
                    style_mask |= NSWindowStyleMask::Closable;
                    style_mask |= NSWindowStyleMask::Miniaturizable;
                    style_mask |= NSWindowStyleMask::Resizable;

                    let _: () = msg_send![ns_window, setStyleMask: style_mask];
                    let _: () = msg_send![ns_window, setTitlebarAppearsTransparent: Bool::YES];
                    let _: () = msg_send![
                        ns_window,
                        setTitleVisibility: NSWindowTitleVisibility::Hidden
                    ];
                    let _: () = msg_send![ns_window, setHasShadow: Bool::YES];
                    let _: () = msg_send![ns_window, setOpaque: Bool::NO];

                    let content_view: ObjcId = msg_send![ns_window, contentView];
                    let _: () = msg_send![content_view, setWantsLayer: Bool::YES];

                    let layer: ObjcId = msg_send![content_view, layer];
                    if !layer.is_null() {
                        let _: () = msg_send![layer, setCornerRadius: radius];
                        let _: () = msg_send![layer, setMasksToBounds: Bool::YES];
                    }

                    position_traffic_lights(ns_window, config.offset_x, config.offset_y);
                }
            })
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    {
        Ok(())
    }
}

/// Repositions Traffic Lights only (useful after fullscreen toggle)
#[tauri::command]
pub fn reposition_traffic_lights<R: Runtime>(
    _app: AppHandle<R>,
    window: WebviewWindow<R>,
    offset_x: Option<f64>,
    offset_y: Option<f64>,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let config = TrafficLightsConfig {
            offset_x: offset_x.unwrap_or(0.0),
            offset_y: offset_y.unwrap_or(0.0),
        };

        window
            .with_webview(move |webview| {
                #[cfg(target_os = "macos")]
                unsafe {
                    let ns_window = webview.ns_window() as ObjcId;
                    position_traffic_lights(ns_window, config.offset_x, config.offset_y);
                }
            })
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    {
        Ok(())
    }
}

#[cfg(target_os = "macos")]
unsafe fn position_traffic_lights(ns_window: ObjcId, offset_x: f64, offset_y: f64) {
    let default_x = 20.0;
    let default_y = 0.0;

    let close_button: ObjcId = msg_send![ns_window, standardWindowButton: 0];
    let miniaturize_button: ObjcId = msg_send![ns_window, standardWindowButton: 1];
    let zoom_button: ObjcId = msg_send![ns_window, standardWindowButton: 2];

    let new_x = default_x + offset_x;
    let new_y = default_y - offset_y;

    if !close_button.is_null() {
        let frame: NSRect = msg_send![close_button, frame];
        let new_frame = NSRect::new(NSPoint::new(new_x, new_y), frame.size);
        let _: () = msg_send![close_button, setFrame: new_frame];
    }

    if !miniaturize_button.is_null() {
        let frame: NSRect = msg_send![miniaturize_button, frame];
        let new_frame = NSRect::new(NSPoint::new(new_x + 20.0, new_y), frame.size);
        let _: () = msg_send![miniaturize_button, setFrame: new_frame];
    }

    if !zoom_button.is_null() {
        let frame: NSRect = msg_send![zoom_button, frame];
        let new_frame = NSRect::new(NSPoint::new(new_x + 40.0, new_y), frame.size);
        let _: () = msg_send![zoom_button, setFrame: new_frame];
    }
}
