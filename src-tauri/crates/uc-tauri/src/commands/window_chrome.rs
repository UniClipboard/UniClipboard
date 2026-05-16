//! 窗口外壳相关 Tauri 命令（目前仅 macOS 交通灯定位）。
//!
//! ## 为什么需要这个模块
//!
//! `tauri.conf.json` 用 `titleBarStyle: "Overlay"` + `hiddenTitle: true` 让前端
//! 自绘 titlebar（见 `src/components/TitleBar.tsx`），但 macOS 系统画的三色
//! 交通灯默认 y-origin 是按"系统标准 titlebar 高度 (28pt)"算的居中位置——
//! 我们自绘的标题栏是 40pt，两个高度对不上，肉眼看就是按钮偏上。Tauri 2 并未
//! 对外暴露 `setTrafficLightPosition` API；第三方
//! `@cloudworxx/tauri-plugin-mac-rounded-corners` 提供过等价能力但已下架。
//! 本模块用 `objc2` 直接调 `NSWindow.standardWindowButton()` 重新摆三个按钮。
//!
//! ## 坐标语义
//!
//! `offset_x` / `offset_y` 走 **屏幕坐标系（y-down）**：正 X 向右、正 Y 向下，
//! 与 UI 直觉一致。内部转成 NSPoint（y-up）时把 Y 取反：
//! `button.origin = (baseline.x + offset_x, baseline.y - offset_y)`。
//!
//! ## 幂等性
//!
//! 命令幂等：传相同 `(offset_x, offset_y)` 多次最终位置一致。首次调用时把
//! 三个按钮的原始 origin 缓存为 baseline，后续调用都基于 baseline + offset，
//! 避免反复加 offset 导致按钮越漂越远。macOS 在 window unmaximize / 全屏
//! 切换后会把按钮重置回标准位置——前端 `TitleBar` 在 `onResized` 里需要再
//! 调一次此命令重新生效。

use crate::commands::record_trace_fields;
#[cfg(target_os = "macos")]
use tauri::Manager;
use tauri::WebviewWindow;
use tracing::{info_span, Instrument};
use uc_platform::ports::observability::TraceMetadata;

#[cfg(target_os = "macos")]
mod imp {
    use objc2_app_kit::{NSWindow, NSWindowButton};
    use objc2_foundation::NSPoint;
    use std::sync::Mutex;
    use tauri::WebviewWindow;

    /// 进程级 baseline：首次调用时三个按钮 (close, miniaturize, zoom) 的
    /// 原始 origin。每次 macOS 把按钮重置回标准位置后，我们仍从 baseline 起算，
    /// 保证幂等。
    static BASELINE: Mutex<Option<[NSPoint; 3]>> = Mutex::new(None);

    const BUTTONS: [NSWindowButton; 3] = [
        NSWindowButton::CloseButton,
        NSWindowButton::MiniaturizeButton,
        NSWindowButton::ZoomButton,
    ];

    pub fn apply(window: &WebviewWindow, offset_x: f64, offset_y: f64) -> Result<(), String> {
        let ns_window_ptr = window
            .ns_window()
            .map_err(|e| format!("Failed to get ns_window: {e}"))?;
        if ns_window_ptr.is_null() {
            return Err("ns_window pointer is null".to_string());
        }

        // SAFETY: ns_window 指向 Tauri 持有的 NSWindow 实例，命令在主线程上
        // 执行（调用方用 `app.run_on_main_thread` 派发），生命周期被 WebviewWindow
        // 间接保证；只读 frame() / 调 setFrameOrigin 不转移所有权。
        unsafe {
            let ns_window: &NSWindow = &*(ns_window_ptr as *const NSWindow);

            let buttons = BUTTONS.map(|b| ns_window.standardWindowButton(b));
            if buttons.iter().any(Option::is_none) {
                return Err("standardWindowButton returned None for one or more buttons".into());
            }

            let mut guard = BASELINE
                .lock()
                .map_err(|e| format!("BASELINE mutex poisoned: {e}"))?;
            let baseline = *guard.get_or_insert_with(|| {
                let mut bs = [NSPoint::new(0.0, 0.0); 3];
                for (i, btn) in buttons.iter().enumerate() {
                    if let Some(b) = btn {
                        bs[i] = b.frame().origin;
                    }
                }
                bs
            });

            // 把屏幕坐标系（y-down）的 offset_y 转成 NSPoint（y-up）：
            // 正 offset_y = 按钮向下 → NSPoint.y 要减。
            for (slot, base) in buttons.iter().zip(baseline.iter()) {
                if let Some(btn) = slot {
                    btn.setFrameOrigin(NSPoint::new(base.x + offset_x, base.y - offset_y));
                }
            }
        }
        Ok(())
    }
}

/// 调整 macOS 主窗口三色交通灯（close/min/zoom）按钮位置。
///
/// `offset_x` / `offset_y` 相对系统给的标准位置偏移，**屏幕坐标系**：
/// 正 X 向右、正 Y 向下。调用幂等。非 macOS 平台 no-op。
#[tauri::command]
#[specta::specta]
pub async fn set_traffic_light_position(
    window: WebviewWindow,
    offset_x: f64,
    offset_y: f64,
    _trace: Option<TraceMetadata>,
) -> Result<(), String> {
    let span = info_span!(
        "command.window_chrome.set_traffic_light_position",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        offset_x,
        offset_y,
    );
    record_trace_fields(&span, &_trace);

    async move {
        #[cfg(target_os = "macos")]
        {
            let app = window.app_handle().clone();
            let window_for_main = window.clone();
            let (tx, rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
            app.run_on_main_thread(move || {
                let _ = tx.send(imp::apply(&window_for_main, offset_x, offset_y));
            })
            .map_err(|e| format!("Failed to dispatch to main thread: {e}"))?;
            rx.await
                .map_err(|e| format!("main thread task cancelled: {e}"))?
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (window, offset_x, offset_y);
            Ok(())
        }
    }
    .instrument(span)
    .await
}
