#[cfg(target_os = "macos")]
mod macos {
    use core_graphics::event::{CGEvent, CGEventFlags};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
    use objc2::rc::Retained;
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};

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

        /// Display name of the application the paste goes to, if there is one.
        pub fn name(&self) -> Option<String> {
            self.0
                .as_ref()
                .and_then(|app| app.localizedName())
                .map(|name| name.to_string())
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
            // The panel itself is the active application while it is open; anything else in front
            // that is not the target means the user moved on.
            let ours = std::process::id() as i32;
            if NSWorkspace::sharedWorkspace()
                .frontmostApplication()
                .is_none_or(|front| {
                    front.processIdentifier() != app.processIdentifier()
                        && front.processIdentifier() != ours
                })
            {
                return Err("焦点已切换，请回到目标应用重新唤起面板。".into());
            }
            Ok(())
        }

        fn target_is_front(app: &NSRunningApplication) -> bool {
            NSWorkspace::sharedWorkspace()
                .frontmostApplication()
                .is_some_and(|front| front.processIdentifier() == app.processIdentifier())
        }

        /// Brings the target application back to the front and waits until it is there.
        pub fn bring_front(&self) -> Result<(), String> {
            let app = self.0.as_ref().ok_or("没有可粘贴的目标应用。")?;
            if Self::target_is_front(app) {
                return Ok(());
            }
            #[allow(deprecated)]
            app.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows);
            for _ in 0..60 {
                if Self::target_is_front(app) {
                    return Ok(());
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err("无法切回目标应用，请重新唤起面板。".into())
        }

        /// When the panel is the active application, hands focus back to the target. Nothing
        /// happens if the user has already gone to another application.
        pub fn return_focus(&self) {
            let ours = std::process::id() as i32;
            if NSWorkspace::sharedWorkspace()
                .frontmostApplication()
                .is_some_and(|front| front.processIdentifier() == ours)
            {
                let _ = self.bring_front();
            }
        }

        pub fn type_text(&self, text: &str) -> Result<(), String> {
            self.check()?;
            self.bring_front()?;
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
            self.bring_front()?;
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

/// Runs the process as a background app: no Dock icon or menu bar, like a menu bar utility.
///
/// GPUI selects the regular activation policy just before it calls the launch callback, so this
/// has to run inside that callback rather than earlier.
#[cfg(target_os = "macos")]
pub fn run_as_background_app() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

    let Some(main_thread) = MainThreadMarker::new() else {
        tracing::warn!("Not on the main thread; keeping the regular activation policy");
        return;
    };
    let switched = NSApplication::sharedApplication(main_thread)
        .setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    if !switched {
        tracing::warn!("Could not switch to the accessory activation policy");
    }
}

/// Windows popups are already tool windows without a taskbar button.
#[cfg(not(target_os = "macos"))]
pub fn run_as_background_app() {}

#[cfg(target_os = "macos")]
pub fn reveal_path(path: &str) -> Result<(), String> {
    use objc2_foundation::{NSArray, NSString, NSURL};
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));
    objc2_app_kit::NSWorkspace::sharedWorkspace()
        .activateFileViewerSelectingURLs(&NSArray::from_retained_slice(&[url]));
    Ok(())
}

/// Opens a web link in the browser or a file in its default application.
#[cfg(target_os = "macos")]
pub fn open_target(target: &str) -> Result<(), String> {
    use objc2_foundation::{NSString, NSURL};
    let url = if target.starts_with("http://") || target.starts_with("https://") {
        NSURL::URLWithString(&NSString::from_str(target)).ok_or("链接无效。")?
    } else {
        NSURL::fileURLWithPath(&NSString::from_str(target))
    };
    if objc2_app_kit::NSWorkspace::sharedWorkspace().openURL(&url) {
        Ok(())
    } else {
        Err("无法打开这项内容。".into())
    }
}

#[cfg(not(target_os = "macos"))]
pub fn open_target(_: &str) -> Result<(), String> {
    Err("此平台尚未实现打开。".into())
}

#[cfg(not(target_os = "macos"))]
pub fn reveal_path(_: &str) -> Result<(), String> {
    Err("此平台尚未实现文件定位。".into())
}

/// Makes this process the active application.
#[cfg(target_os = "macos")]
pub fn activate_app() {
    if let Some(main_thread) = objc2::MainThreadMarker::new() {
        #[allow(deprecated)]
        objc2_app_kit::NSApplication::sharedApplication(main_thread)
            .activateIgnoringOtherApps(true);
    }
}

#[cfg(not(target_os = "macos"))]
pub fn activate_app() {}

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
pub fn clip_preview_shape(
    window: &gpui::Window,
    placement: crate::window_pair::PreviewPlacement,
    scale: f64,
    cx: &gpui::App,
) -> Result<(), String> {
    use crate::window_pair::{
        PreviewSide, POINTER_DEPTH, POINTER_HALF_HEIGHT, PREVIEW_CORNER_RADIUS,
    };
    use objc2_app_kit::NSBezierPath;
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    use objc2_quartz_core::{CAShapeLayer, CATransaction};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window).map_err(|_| "无法访问预览窗口。")?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return Err("窗口类型不支持。".into());
    };
    let view = unsafe { &*handle.ns_view.as_ptr().cast::<objc2_app_kit::NSView>() };
    let layer = view.layer().ok_or("预览绘制层尚未就绪。")?;
    let native = view.window().ok_or("预览窗口已关闭。")?;
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
    _: crate::window_pair::PreviewPlacement,
    _: f64,
    _: &gpui::App,
) -> Result<(), String> {
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
    pub fn name(&self) -> Option<String> {
        None
    }
    pub fn check(&self) -> Result<(), String> {
        Err("此原型的自动粘贴仅支持 macOS；请使用复制按钮。".into())
    }
    pub fn return_focus(&self) {}
    pub fn paste(&self) -> Result<(), String> {
        self.check()
    }
}
