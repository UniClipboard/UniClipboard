#[cfg(target_os = "macos")]
mod macos {
    use core_graphics::event::{CGEvent, CGEventFlags};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
    use objc2::rc::Retained;
    use objc2_app_kit::{NSRunningApplication, NSWorkspace};

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }

    pub struct PasteTarget(Option<Retained<NSRunningApplication>>);

    impl PasteTarget {
        pub fn capture() -> Self {
            Self(
                NSWorkspace::sharedWorkspace()
                    .frontmostApplication()
                    .filter(|app| app.processIdentifier() != std::process::id() as i32),
            )
        }

        pub fn check(&self) -> Result<(), String> {
            // Accessibility permission is checked without opening a system prompt.
            if !unsafe { AXIsProcessTrusted() } {
                return Err("需要辅助功能权限才能自动粘贴；也可使用复制按钮。".into());
            }
            let app = self
                .0
                .as_ref()
                .ok_or("没有可粘贴的目标应用，请从其他应用唤起面板。")?;
            if app.isTerminated() {
                return Err("原应用已退出，请重新唤起面板。".into());
            }
            if NSWorkspace::sharedWorkspace()
                .frontmostApplication()
                .is_none_or(|front| front.processIdentifier() != app.processIdentifier())
            {
                return Err("焦点已切换，请回到目标应用重新唤起面板。".into());
            }
            Ok(())
        }

        pub fn type_text(&self, text: &str) -> Result<(), String> {
            self.check()?;
            let app = self.0.as_ref().ok_or("没有可粘贴的目标应用。")?;
            let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
                .map_err(|_| "无法创建输入事件。")?;
            let down = CGEvent::new_keyboard_event(source.clone(), 0, true)
                .map_err(|_| "无法创建输入事件。")?;
            let up =
                CGEvent::new_keyboard_event(source, 0, false).map_err(|_| "无法创建输入事件。")?;
            down.set_string(text);
            up.set_string(text);
            down.post_to_pid(app.processIdentifier());
            up.post_to_pid(app.processIdentifier());
            Ok(())
        }

        pub fn paste(&self) -> Result<(), String> {
            self.check()?;
            let app = self.0.as_ref().ok_or("没有可粘贴的目标应用。")?;
            let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
                .map_err(|_| "无法创建粘贴事件。")?;
            let down = CGEvent::new_keyboard_event(source.clone(), 9, true)
                .map_err(|_| "无法创建粘贴事件。")?;
            let up =
                CGEvent::new_keyboard_event(source, 9, false).map_err(|_| "无法创建粘贴事件。")?;
            down.set_flags(CGEventFlags::CGEventFlagCommand);
            up.set_flags(CGEventFlags::CGEventFlagCommand);
            // Target the retained application's PID rather than the global event stream.
            down.post_to_pid(app.processIdentifier());
            up.post_to_pid(app.processIdentifier());
            Ok(())
        }
    }
}

#[cfg(target_os = "macos")]
pub use macos::PasteTarget;

#[cfg(target_os = "macos")]
pub fn panel_anchor(cursor_anchored: bool, width: f64, height: f64) -> Result<(f64, f64), String> {
    use objc2_app_kit::{NSEvent, NSScreen};
    let screens = NSScreen::screens(objc2::MainThreadMarker::new().ok_or("需要主线程。")?);
    let primary = screens.firstObject().ok_or("找不到显示器。")?;
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

#[cfg(not(target_os = "macos"))]
pub fn panel_anchor(_: bool, _: f64, _: f64) -> Result<(f64, f64), String> {
    Err("此平台窗口定位尚未实现。".into())
}

#[cfg(target_os = "macos")]
pub fn reveal_path(path: &str) -> Result<(), String> {
    use objc2_foundation::{NSArray, NSString, NSURL};
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));
    objc2_app_kit::NSWorkspace::sharedWorkspace()
        .activateFileViewerSelectingURLs(&NSArray::from_retained_slice(&[url]));
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn reveal_path(_: &str) -> Result<(), String> {
    Err("此平台尚未实现文件定位。".into())
}

#[cfg(target_os = "macos")]
pub fn set_visible(window: &gpui::Window, visible: bool) -> Result<(), String> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window).map_err(|_| "无法访问面板窗口。")?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return Err("窗口类型不支持。".into());
    };
    // GPUI owns the view; access is synchronous on its UI thread.
    let view = unsafe { &*handle.ns_view.as_ptr().cast::<objc2_app_kit::NSView>() };
    let native = view.window().ok_or("面板窗口已关闭。")?;
    native.setHasShadow(true);
    if visible {
        native.orderFrontRegardless();
        native.makeKeyWindow();
    } else {
        native.orderOut(None);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn set_frame(
    window: &mut gpui::Window,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    cx: &gpui::App,
) -> Result<(), String> {
    use objc2_foundation::NSPoint;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window).map_err(|_| "无法访问面板窗口。")?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return Err("窗口类型不支持。".into());
    };
    let view = unsafe { &*handle.ns_view.as_ptr().cast::<objc2_app_kit::NSView>() };
    let native = view.window().ok_or("面板窗口已关闭。")?;
    let screen =
        objc2_app_kit::NSScreen::screens(objc2::MainThreadMarker::new().ok_or("需要主线程。")?)
            .firstObject()
            .ok_or("找不到显示器。")?;
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

#[cfg(target_os = "macos")]
pub fn configure_shaped_preview(window: &gpui::Window, cx: &gpui::App) -> Result<(), String> {
    use objc2_app_kit::{NSColor, NSWindowStyleMask};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window).map_err(|_| "无法访问预览窗口。")?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return Err("窗口类型不支持。".into());
    };
    let view = unsafe { &*handle.ns_view.as_ptr().cast::<objc2_app_kit::NSView>() };
    let native = view.window().ok_or("预览窗口已关闭。")?;
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

#[cfg(not(target_os = "macos"))]
pub fn configure_shaped_preview(_: &gpui::Window, _: &gpui::App) -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn show_without_focus(window: &gpui::Window) -> Result<(), String> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window).map_err(|_| "无法访问预览窗口。")?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return Err("窗口类型不支持。".into());
    };
    let view = unsafe { &*handle.ns_view.as_ptr().cast::<objc2_app_kit::NSView>() };
    let native = view.window().ok_or("预览窗口已关闭。")?;
    native.setHasShadow(true);
    native.orderFrontRegardless();
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn show_without_focus(window: &gpui::Window) -> Result<(), String> {
    set_visible(window, true)
}

#[cfg(not(target_os = "macos"))]
pub fn set_visible(window: &gpui::Window, visible: bool) -> Result<(), String> {
    if visible {
        window.activate_window();
        Ok(())
    } else {
        Err("此平台隐藏窗口尚未实现。".into())
    }
}

#[cfg(not(target_os = "macos"))]
pub fn set_frame(
    _: &mut gpui::Window,
    _: f64,
    _: f64,
    _: f64,
    _: f64,
    _: &gpui::App,
) -> Result<(), String> {
    Err("此平台窗口定位尚未实现。".into())
}

#[cfg(not(target_os = "macos"))]
pub struct PasteTarget;

#[cfg(not(target_os = "macos"))]
impl PasteTarget {
    pub fn type_text(&self, _: &str) -> Result<(), String> {
        self.check()
    }
    pub fn capture() -> Self {
        Self
    }
    pub fn check(&self) -> Result<(), String> {
        Err("此原型的自动粘贴仅支持 macOS；请使用复制按钮。".into())
    }
    pub fn paste(&self) -> Result<(), String> {
        self.check()
    }
}
