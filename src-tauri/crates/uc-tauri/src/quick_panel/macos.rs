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
use tracing::{debug, error, info};

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

// ── Accessibility helpers (shared types & FFI) ────────────────────────

#[repr(C)]
struct AXCFRange {
    location: i64,
    length: i64,
}

#[repr(C)]
struct AXCGRect {
    origin_x: f64,
    origin_y: f64,
    size_width: f64,
    size_height: f64,
}

#[repr(C)]
struct AXCGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
struct AXCGSize {
    width: f64,
    height: f64,
}

const AX_VALUE_CG_POINT: u32 = 1;
const AX_VALUE_CG_SIZE: u32 = 2;
const AX_VALUE_CG_RECT: u32 = 3;
const AX_VALUE_CF_RANGE: u32 = 4;
const AX_ERROR_SUCCESS: i32 = 0;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateSystemWide() -> core_foundation::base::CFTypeRef;
    fn AXUIElementCopyAttributeValue(
        element: core_foundation::base::CFTypeRef,
        attribute: core_foundation::string::CFStringRef,
        value: *mut core_foundation::base::CFTypeRef,
    ) -> i32;
    fn AXUIElementCopyParameterizedAttributeValue(
        element: core_foundation::base::CFTypeRef,
        attribute: core_foundation::string::CFStringRef,
        parameter: core_foundation::base::CFTypeRef,
        value: *mut core_foundation::base::CFTypeRef,
    ) -> i32;
    fn AXValueGetValue(
        value: core_foundation::base::CFTypeRef,
        type_: u32,
        value_ptr: *mut std::ffi::c_void,
    ) -> bool;
    fn AXValueCreate(
        type_: u32,
        value_ptr: *const std::ffi::c_void,
    ) -> core_foundation::base::CFTypeRef;
    fn CFStringGetCString(
        the_string: core_foundation::base::CFTypeRef,
        buffer: *mut u8,
        buffer_size: i64,
        encoding: u32,
    ) -> bool;
}

/// Helper: get a CFRange attribute from an AX element.
unsafe fn ax_get_range(
    element: core_foundation::base::CFTypeRef,
    attr_name: &str,
) -> Option<AXCFRange> {
    use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
    use core_foundation::string::CFString;
    use std::ffi::c_void;
    use std::ptr;

    let attr = CFString::new(attr_name);
    let mut val: CFTypeRef = ptr::null();
    let err = AXUIElementCopyAttributeValue(element, attr.as_concrete_TypeRef(), &mut val);
    if err != AX_ERROR_SUCCESS || val.is_null() {
        return None;
    }
    let mut range = AXCFRange {
        location: 0,
        length: 0,
    };
    let ok = AXValueGetValue(
        val,
        AX_VALUE_CF_RANGE,
        &mut range as *mut AXCFRange as *mut c_void,
    );
    CFRelease(val);
    if ok {
        Some(range)
    } else {
        None
    }
}

/// Helper: get a CGPoint attribute from an AX element.
unsafe fn ax_get_point(
    element: core_foundation::base::CFTypeRef,
    attr_name: &str,
) -> Option<AXCGPoint> {
    use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
    use core_foundation::string::CFString;
    use std::ffi::c_void;
    use std::ptr;

    let attr = CFString::new(attr_name);
    let mut val: CFTypeRef = ptr::null();
    let err = AXUIElementCopyAttributeValue(element, attr.as_concrete_TypeRef(), &mut val);
    if err != AX_ERROR_SUCCESS || val.is_null() {
        return None;
    }
    let mut point = AXCGPoint { x: 0.0, y: 0.0 };
    let ok = AXValueGetValue(
        val,
        AX_VALUE_CG_POINT,
        &mut point as *mut AXCGPoint as *mut c_void,
    );
    CFRelease(val);
    if ok {
        Some(point)
    } else {
        None
    }
}

/// Helper: get an integer attribute from an AX element.
unsafe fn ax_get_int(element: core_foundation::base::CFTypeRef, attr_name: &str) -> Option<i64> {
    use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
    use core_foundation::string::CFString;
    use std::ptr;

    let attr = CFString::new(attr_name);
    let mut val: CFTypeRef = ptr::null();
    let err = AXUIElementCopyAttributeValue(element, attr.as_concrete_TypeRef(), &mut val);
    if err != AX_ERROR_SUCCESS || val.is_null() {
        return None;
    }

    // CFNumber → i64
    let mut result: i64 = 0;
    let ok = core_foundation::number::CFNumberGetValue(
        val as core_foundation::number::CFNumberRef,
        core_foundation::number::kCFNumberSInt64Type,
        &mut result as *mut i64 as *mut std::ffi::c_void,
    );
    CFRelease(val);
    if ok {
        Some(result)
    } else {
        None
    }
}

/// Helper: get AXBoundsForRange for a given CFRange on an element.
/// Returns the CGRect in AX screen coordinates (top-left origin).
unsafe fn ax_bounds_for_range(
    element: core_foundation::base::CFTypeRef,
    range: &AXCFRange,
) -> Option<AXCGRect> {
    use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
    use core_foundation::string::CFString;
    use std::ffi::c_void;
    use std::ptr;

    let range_val = AXValueCreate(
        AX_VALUE_CF_RANGE,
        range as *const AXCFRange as *const c_void,
    );
    if range_val.is_null() {
        return None;
    }

    let attr_bounds = CFString::new("AXBoundsForRange");
    let mut bounds_val: CFTypeRef = ptr::null();
    let err = AXUIElementCopyParameterizedAttributeValue(
        element,
        attr_bounds.as_concrete_TypeRef(),
        range_val,
        &mut bounds_val,
    );
    CFRelease(range_val);
    if err != AX_ERROR_SUCCESS || bounds_val.is_null() {
        return None;
    }

    let mut rect = AXCGRect {
        origin_x: 0.0,
        origin_y: 0.0,
        size_width: 0.0,
        size_height: 0.0,
    };
    let ok = AXValueGetValue(
        bounds_val,
        AX_VALUE_CG_RECT,
        &mut rect as *mut AXCGRect as *mut c_void,
    );
    CFRelease(bounds_val);
    if ok && rect.size_height > 0.0 {
        Some(rect)
    } else {
        None
    }
}

/// Helper: get the AXRole string of an element.
unsafe fn ax_get_role(element: core_foundation::base::CFTypeRef) -> Option<String> {
    use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
    use core_foundation::string::CFString;
    use std::ptr;

    let attr = CFString::new("AXRole");
    let mut val: CFTypeRef = ptr::null();
    let err = AXUIElementCopyAttributeValue(element, attr.as_concrete_TypeRef(), &mut val);
    if err != AX_ERROR_SUCCESS || val.is_null() {
        return None;
    }
    let mut buf = [0u8; 128];
    let ok = CFStringGetCString(val, buf.as_mut_ptr(), 128, 0x0600_0100); // kCFStringEncodingUTF8
    CFRelease(val);
    if ok {
        let s = std::ffi::CStr::from_ptr(buf.as_ptr() as *const _);
        s.to_str().ok().map(|s| s.to_string())
    } else {
        None
    }
}

// ── Caret position detection ──────────────────────────────────────────

/// Try to get caret bounds for a specific character location.
/// Uses `length: 1` (key difference from broken `length: 0` approach).
///
/// 获取指定位置字符的边界矩形（使用 length: 1）。
unsafe fn get_bounds_at(
    element: core_foundation::base::CFTypeRef,
    location: i64,
) -> Option<AXCGRect> {
    let range = AXCFRange {
        location: location.max(0),
        length: 1,
    };
    ax_bounds_for_range(element, &range)
}

/// Try to get precise caret position via AXBoundsForRange.
/// Handles cursor-at-end-of-text and visible range edge cases
/// (learned from InputSourcePro).
///
/// `cursor_pos` is the character offset of the caret (selected.location + selected.length).
///
/// 通过 AXBoundsForRange 获取精确光标位置，处理文本末尾等边界情况。
unsafe fn find_native_cursor_bounds(
    focused: core_foundation::base::CFTypeRef,
    cursor_pos: i64,
) -> Option<AXCGRect> {
    let visible = ax_get_range(focused, "AXVisibleCharacterRange");

    // Check if cursor is at or past the end of visible text
    let is_at_end = visible
        .as_ref()
        .map(|v| cursor_pos >= v.location + v.length)
        .unwrap_or(false);

    let location = if is_at_end && cursor_pos > 0 {
        cursor_pos - 1 // Back up one character to get valid bounds
    } else {
        cursor_pos
    };

    // Primary: get bounds of the character at cursor (length: 1)
    if let Some(rect) = get_bounds_at(focused, location) {
        debug!(
            origin_x = rect.origin_x,
            origin_y = rect.origin_y,
            width = rect.size_width,
            height = rect.size_height,
            at_end = is_at_end,
            "Got cursor bounds via AXBoundsForRange"
        );
        return Some(rect);
    }

    None
}

/// Fallback: get line-level bounds via AXInsertionPointLineNumber →
/// AXRangeForLine → AXBoundsForRange, then refine x-position using
/// the sub-range from line start to cursor position.
///
/// 降级方案：通过行号获取行级别的边界，再用子范围精确定位 x 坐标。
unsafe fn find_line_bounds(
    focused: core_foundation::base::CFTypeRef,
    cursor_pos: i64,
) -> Option<AXCGRect> {
    use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
    use core_foundation::string::CFString;
    use std::ffi::c_void;
    use std::ptr;

    let line_num = ax_get_int(focused, "AXInsertionPointLineNumber")?;

    // AXRangeForLine is a parameterized attribute: line_number → CFRange
    let line_cf = core_foundation::number::CFNumber::from(line_num as i32);
    let attr_rfl = CFString::new("AXRangeForLine");
    let mut range_val: CFTypeRef = ptr::null();
    let err = AXUIElementCopyParameterizedAttributeValue(
        focused,
        attr_rfl.as_concrete_TypeRef(),
        line_cf.as_concrete_TypeRef() as CFTypeRef,
        &mut range_val,
    );
    if err != AX_ERROR_SUCCESS || range_val.is_null() {
        debug!(ax_error = err, "Cannot get AXRangeForLine");
        return None;
    }

    let mut line_range = AXCFRange {
        location: 0,
        length: 0,
    };
    let ok = AXValueGetValue(
        range_val,
        AX_VALUE_CF_RANGE,
        &mut line_range as *mut AXCFRange as *mut c_void,
    );
    CFRelease(range_val);
    if !ok || line_range.length == 0 {
        return None;
    }

    // Get the full line bounds (for y-position and height)
    let line_rect = ax_bounds_for_range(focused, &line_range)?;

    // Refine x-position: get bounds from line start to cursor position.
    // The right edge of this sub-range is where the caret actually is.
    let chars_before_cursor = cursor_pos - line_range.location;
    if chars_before_cursor > 0 {
        let sub_range = AXCFRange {
            location: line_range.location,
            length: chars_before_cursor,
        };
        if let Some(sub_rect) = ax_bounds_for_range(focused, &sub_range) {
            debug!(
                line = line_num,
                origin_x = sub_rect.origin_x + sub_rect.size_width,
                origin_y = line_rect.origin_y,
                "Got line bounds with refined x-position"
            );
            return Some(AXCGRect {
                origin_x: sub_rect.origin_x + sub_rect.size_width,
                origin_y: line_rect.origin_y,
                size_width: 0.0,
                size_height: line_rect.size_height,
            });
        }
    }

    debug!(
        line = line_num,
        origin_x = line_rect.origin_x,
        origin_y = line_rect.origin_y,
        "Got line bounds fallback (line start)"
    );
    Some(line_rect)
}

/// Try web area cursor detection (browsers/Electron apps).
/// Uses AXSelectedTextMarkerRange + AXBoundsForTextMarkerRange.
///
/// 浏览器/Electron 应用的光标检测。
unsafe fn find_web_area_cursor(focused: core_foundation::base::CFTypeRef) -> Option<AXCGRect> {
    use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
    use core_foundation::string::CFString;
    use std::ffi::c_void;
    use std::ptr;

    // Get AXSelectedTextMarkerRange (opaque marker, not a CFRange)
    let attr_marker = CFString::new("AXSelectedTextMarkerRange");
    let mut marker_val: CFTypeRef = ptr::null();
    let err =
        AXUIElementCopyAttributeValue(focused, attr_marker.as_concrete_TypeRef(), &mut marker_val);
    if err != AX_ERROR_SUCCESS || marker_val.is_null() {
        return None;
    }

    // AXBoundsForTextMarkerRange: marker_range → CGRect
    let attr_bounds = CFString::new("AXBoundsForTextMarkerRange");
    let mut bounds_val: CFTypeRef = ptr::null();
    let err = AXUIElementCopyParameterizedAttributeValue(
        focused,
        attr_bounds.as_concrete_TypeRef(),
        marker_val,
        &mut bounds_val,
    );
    CFRelease(marker_val);
    if err != AX_ERROR_SUCCESS || bounds_val.is_null() {
        return None;
    }

    let mut rect = AXCGRect {
        origin_x: 0.0,
        origin_y: 0.0,
        size_width: 0.0,
        size_height: 0.0,
    };
    let ok = AXValueGetValue(
        bounds_val,
        AX_VALUE_CG_RECT,
        &mut rect as *mut AXCGRect as *mut c_void,
    );
    CFRelease(bounds_val);
    if ok && rect.size_height > 0.0 {
        debug!(
            origin_x = rect.origin_x,
            origin_y = rect.origin_y,
            "Got web area cursor bounds"
        );
        Some(rect)
    } else {
        None
    }
}

/// Try to get the text caret position from the focused app using the
/// macOS Accessibility API.
///
/// Strategy (learned from InputSourcePro):
/// 1. Try web area cursor (AXSelectedTextMarkerRange → AXBoundsForTextMarkerRange)
/// 2. Try native cursor (AXSelectedTextRange → AXBoundsForRange, length: 1)
/// 3. Fallback: AXInsertionPointLineNumber → AXRangeForLine → refined x via sub-range
///
/// No strict role check — AXSelectedTextRange availability is the natural
/// filter (non-text elements simply won't have it). This avoids rejecting
/// chat apps or custom inputs with non-standard roles.
///
/// Returns `Some((x, y))` in screen coordinates (top-left origin).
///
/// 通过 macOS Accessibility API 获取聚焦应用中文本光标的位置。
/// 不做严格的 role 检查，以 AXSelectedTextRange 是否存在作为天然过滤。
fn get_caret_position() -> Option<(f64, f64)> {
    use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
    use core_foundation::string::CFString;
    use std::ptr;

    unsafe {
        // 1. System-wide → focused element
        let system = AXUIElementCreateSystemWide();
        if system.is_null() {
            return None;
        }
        let attr_focused = CFString::new("AXFocusedUIElement");
        let mut focused: CFTypeRef = ptr::null();
        let err =
            AXUIElementCopyAttributeValue(system, attr_focused.as_concrete_TypeRef(), &mut focused);
        CFRelease(system);
        if err != AX_ERROR_SUCCESS || focused.is_null() {
            debug!(ax_error = err, "Cannot get AXFocusedUIElement");
            return None;
        }

        let role = ax_get_role(focused);
        let role_str = role.as_deref().unwrap_or("");
        debug!(role = role_str, "Focused element role");

        // 2. Try web area cursor first (browsers / Electron)
        if let Some(rect) = find_web_area_cursor(focused) {
            CFRelease(focused);
            let x = rect.origin_x;
            let y = rect.origin_y + rect.size_height;
            info!(x, y, "Got caret position via web area marker");
            return Some((x, y));
        }

        // 3. Try native cursor bounds (AXBoundsForRange with length: 1)
        //    This also extracts cursor_pos for the line fallback below.
        let selected = ax_get_range(focused, "AXSelectedTextRange");
        let cursor_pos = selected
            .as_ref()
            .map(|s| s.location + s.length)
            .unwrap_or(-1);

        if selected.is_some() {
            if let Some(rect) = find_native_cursor_bounds(focused, cursor_pos) {
                CFRelease(focused);
                let x = rect.origin_x;
                let y = rect.origin_y + rect.size_height;
                info!(x, y, "Got caret position via AXBoundsForRange");
                return Some((x, y));
            }
        }

        // 4. Fallback: line bounds with refined x-position
        if cursor_pos >= 0 {
            if let Some(rect) = find_line_bounds(focused, cursor_pos) {
                CFRelease(focused);
                let x = rect.origin_x;
                let y = rect.origin_y + rect.size_height;
                info!(x, y, "Got caret position via line bounds");
                return Some((x, y));
            }
        }

        // 5. Last resort for confirmed text inputs (e.g. empty input field):
        //    AXSelectedTextRange exists but no bounds could be retrieved
        //    (no characters to measure). Use the element's AXPosition directly —
        //    the caret in an empty field sits at the element's top-left.
        if selected.is_some() {
            if let Some(pos) = ax_get_point(focused, "AXPosition") {
                CFRelease(focused);
                info!(
                    x = pos.x,
                    y = pos.y,
                    "Using element AXPosition for empty text input"
                );
                return Some((pos.x, pos.y));
            }
        }

        CFRelease(focused);
        None
    }
}

/// Try to get the focused UI element's frame position via AXPosition/AXSize.
/// Less precise than caret position but more widely supported.
///
/// 通过 AXPosition/AXSize 获取焦点元素的位置（比 AXBoundsForRange 更通用）。
fn get_focused_element_position() -> Option<(f64, f64)> {
    use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
    use core_foundation::string::CFString;
    use std::ffi::c_void;
    use std::ptr;

    unsafe {
        let system = AXUIElementCreateSystemWide();
        if system.is_null() {
            return None;
        }

        let attr_focused = CFString::new("AXFocusedUIElement");
        let mut focused: CFTypeRef = ptr::null();
        let err =
            AXUIElementCopyAttributeValue(system, attr_focused.as_concrete_TypeRef(), &mut focused);
        CFRelease(system);
        if err != AX_ERROR_SUCCESS || focused.is_null() {
            return None;
        }

        // Get AXPosition
        let attr_pos = CFString::new("AXPosition");
        let mut pos_val: CFTypeRef = ptr::null();
        let err =
            AXUIElementCopyAttributeValue(focused, attr_pos.as_concrete_TypeRef(), &mut pos_val);
        if err != AX_ERROR_SUCCESS || pos_val.is_null() {
            CFRelease(focused);
            return None;
        }

        let mut point = AXCGPoint { x: 0.0, y: 0.0 };
        let ok = AXValueGetValue(
            pos_val,
            AX_VALUE_CG_POINT,
            &mut point as *mut AXCGPoint as *mut c_void,
        );
        CFRelease(pos_val);
        if !ok {
            CFRelease(focused);
            return None;
        }

        // Get AXSize to position panel below the element
        let attr_size = CFString::new("AXSize");
        let mut size_val: CFTypeRef = ptr::null();
        let err =
            AXUIElementCopyAttributeValue(focused, attr_size.as_concrete_TypeRef(), &mut size_val);

        let y_offset = if err == AX_ERROR_SUCCESS && !size_val.is_null() {
            let mut size = AXCGSize {
                width: 0.0,
                height: 0.0,
            };
            let ok = AXValueGetValue(
                size_val,
                AX_VALUE_CG_SIZE,
                &mut size as *mut AXCGSize as *mut c_void,
            );
            CFRelease(size_val);
            if ok {
                size.height
            } else {
                0.0
            }
        } else {
            0.0
        };

        CFRelease(focused);

        if point.x == 0.0 && point.y == 0.0 {
            debug!("AXPosition returned (0, 0), treating as unavailable");
            return None;
        }

        let x = point.x;
        let y = point.y + y_offset;
        info!(x, y, "Got focused element position via AXPosition/AXSize");
        Some((x, y))
    }
}

/// Get cursor position in screen coordinates (top-left origin).
///
/// Tries these sources in order:
/// 1. Text caret position via `AXBoundsForRange` (most precise)
/// 2. Focused element position via `AXPosition`/`AXSize` (wider support)
/// 3. Mouse cursor position (always available)
///
/// 获取光标位置（屏幕坐标，左上角原点）。依次尝试：
/// 文本光标 → 焦点元素位置 → 鼠标位置。
pub fn get_cursor_position() -> (f64, f64) {
    // Try text caret position first (most precise)
    if let Some(pos) = get_caret_position() {
        return pos;
    }

    // Try focused element position (wider support)
    if let Some(pos) = get_focused_element_position() {
        return pos;
    }

    // Fallback: mouse cursor position
    debug!("Caret and element position unavailable, falling back to mouse position");
    let point = NSEvent::mouseLocation();

    // Convert from macOS bottom-left origin to top-left origin.
    let screen_height = MainThreadMarker::new()
        .and_then(|mtm| {
            let screen = NSScreen::mainScreen(mtm)?;
            Some(screen.frame().size.height)
        })
        .unwrap_or(900.0);

    let result = (point.x, screen_height - point.y);
    debug!(x = result.0, y = result.1, "Using mouse position fallback");
    result
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
