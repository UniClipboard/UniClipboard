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
        return Ok(());
    }

    let path = if dev {
        "updater.html?dev=1"
    } else {
        "updater.html"
    };
    let url = WebviewUrl::App(path.into());

    let builder = WebviewWindowBuilder::new(app, UPDATER_WINDOW_LABEL, url)
        .title(WINDOW_TITLE)
        .inner_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        // OS shadow draws a rectangle outside the webview; combined with
        // transparent + CSS border-radius it bleeds past the rounded corners
        // (tauri-apps/tauri#9287 macOS, #11321 Win10). CSS shadow-2xl on the
        // root div already paints a corner-aware shadow.
        .shadow(false)
        .center()
        .focused(true)
        .visible(true);

    match builder.build() {
        Ok(_) => {
            debug!(target: "update_scheduler::window", dev, "updater window created");
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
