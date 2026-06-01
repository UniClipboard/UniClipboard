//! Sparkle 风格的"新版本可用"独立窗口。
//!
//! 取代/补充原本的系统 toast（`notification.rs`）作为更显眼的提醒：
//! - 检测到新版本时由 `scheduler::notify_if_new_version` 调用
//! - 用户从 About 区点击"Check for updates"找到更新时也可复用
//! - dev 入口 `dev_open_updater_window` 用 `dev=true` 注入 mock 数据
//!
//! 窗口生命周期：
//! - label 固定为 [`UPDATER_WINDOW_LABEL`]，重复调用复用已有窗口
//! - 用户关窗即销毁；下次再开走 builder 路径

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use tracing::{debug, warn};

/// 单例 label——和 capabilities/default.json 中登记的 window 名对应。
pub const UPDATER_WINDOW_LABEL: &str = "updater";

const WINDOW_TITLE: &str = "Software Update";
const WINDOW_WIDTH: f64 = 520.0;
const WINDOW_HEIGHT: f64 = 460.0;

/// 打开 updater 窗口；已存在则把它 unminimize + set_focus 提到前台。
///
/// `dev = true` 时在 URL 上附加 `?dev=1`，前端会跳过订阅、改用 hardcoded
/// mock metadata 渲染。production 路径请传 false。
pub fn open_or_focus_updater_window(app: &AppHandle, dev: bool) -> Result<(), tauri::Error> {
    if let Some(existing) = app.get_webview_window(UPDATER_WINDOW_LABEL) {
        debug!(target: "update_scheduler::window", "updater window already exists; focusing");
        existing.unminimize()?;
        existing.show()?;
        existing.set_focus()?;
        #[cfg(target_os = "macos")]
        surface_above_apps_macos(app, &existing);
        return Ok(());
    }

    let path = if dev {
        "updater.html?dev=1"
    } else {
        "updater.html"
    };
    let url = WebviewUrl::App(path.into());

    // Standard decorated window so it behaves like a first-class OS window:
    // native title bar + traffic lights, system shadow, independent lifecycle.
    // A borderless/transparent window with CSS-drawn chrome looked non-native
    // and got tied to the main window's miniaturize; native decorations fix
    // both (the frontend drops its custom border/shadow/drag-region to match).
    let builder = WebviewWindowBuilder::new(app, UPDATER_WINDOW_LABEL, url)
        .title(WINDOW_TITLE)
        .inner_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .resizable(false)
        .center()
        .focused(true)
        .visible(true);

    match builder.build() {
        Ok(window) => {
            debug!(target: "update_scheduler::window", dev, "updater window created");
            // macOS: actively surface + activate so the popup lands on top even
            // when the app is a no-Dock Accessory background process with no
            // visible windows. Without this, a freshly built window stays behind
            // other apps — which manifested as "the update popup only shows after
            // you open the main window". See `surface_above_apps_macos`.
            #[cfg(target_os = "macos")]
            surface_above_apps_macos(app, &window);
            #[cfg(not(target_os = "macos"))]
            let _ = window;
            Ok(())
        }
        Err(err) => {
            warn!(
                target: "update_scheduler::window",
                error = %err,
                "failed to create updater window"
            );
            Err(err)
        }
    }
}

/// macOS: force the updater window above every other app and activate this app,
/// even when it is a no-Dock `Accessory` background process with no visible
/// windows.
///
/// Why `set_focus` / `.focused(true)` isn't enough: for an LSUIElement /
/// `Accessory` background app the system ignores Tauri's activation request, so
/// the window gets built but stays behind whatever app is frontmost — the user
/// thinks "nothing popped up". That is the root cause of the old behavior where
/// the popup only appeared after opening the main window (the main-window path
/// runs `set_dock_visibility(true)`, flipping the app to `Regular`).
///
/// This takes the native AppKit route (same as Sparkle), never touching the
/// activation policy and never showing a Dock icon:
/// - `NSRunningApplication::activate(ActivateAllWindows)` brings this app to the
///   front so the window can become key and take keyboard focus;
/// - `makeKeyAndOrderFront` makes it the key window;
/// - `orderFrontRegardless` is the fallback — on macOS 14+ the system may
///   downgrade a background app's activation request (the old `ignoringOtherApps`
///   override was deprecated to a no-op), so this orders the window above other
///   apps regardless, keeping it visible even when activation didn't fully take.
///
/// AppKit calls must run on the main thread. `open_or_focus_updater_window` is
/// invoked from the scheduler / manual tray check on a tokio worker thread, so
/// this dispatches fire-and-forget to the main thread. The `NSWindow` pointer is
/// re-fetched inside the main-thread closure (raw pointers aren't `Send`), the
/// same pattern as `commands::window_chrome`.
#[cfg(target_os = "macos")]
fn surface_above_apps_macos(app: &AppHandle, window: &tauri::WebviewWindow) {
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWindow};

    let window = window.clone();
    if let Err(err) = app.run_on_main_thread(move || {
        let ns_window_ptr = match window.ns_window() {
            Ok(ptr) if !ptr.is_null() => ptr,
            Ok(_) => {
                warn!(
                    target: "update_scheduler::window",
                    "ns_window pointer is null; updater window may stay behind other apps"
                );
                return;
            }
            Err(err) => {
                warn!(
                    target: "update_scheduler::window",
                    error = %err,
                    "failed to read ns_window; updater window may stay behind other apps"
                );
                return;
            }
        };

        // Bring this app to the front first (no Dock icon, no policy change), so
        // the updater window can become key and take keyboard focus. macOS 14+
        // may still downgrade a background activation request — the
        // `orderFrontRegardless` below is the fallback that keeps the window
        // visible regardless.
        NSRunningApplication::currentApplication()
            .activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows);

        // SAFETY: `ns_window_ptr` points at the NSWindow owned by Tauri; this
        // closure runs on the main thread and its lifetime is kept alive by the
        // captured `WebviewWindow`. We only send order/key messages — no
        // ownership transfer.
        unsafe {
            let ns_window: &NSWindow = &*(ns_window_ptr as *const NSWindow);
            ns_window.makeKeyAndOrderFront(None);
            // Fallback for when activation was downgraded by the system: order
            // above other apps regardless of this app's active state.
            ns_window.orderFrontRegardless();
        }
    }) {
        warn!(
            target: "update_scheduler::window",
            error = %err,
            "failed to dispatch updater window surfacing to the main thread"
        );
    }
}
