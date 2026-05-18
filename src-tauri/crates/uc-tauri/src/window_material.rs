//! Windows 主窗口的 DWM 材质装配 + 状态查询。
//!
//! ## 为什么需要这个模块
//!
//! `tauri.conf.json` 的 `windowEffects: ["hudWindow"]` 是 macOS 专属字段（Tauri
//! 在 Windows 下不读它）—— 结果就是 macOS 走 NSVisualEffectView 真透明，但
//! Windows 端长期是不透明白底，与 macOS 主窗口并排放时有显眼的 native-vs-not
//! 视觉差距。issue #699 / ship-readiness §D31 要求两端材质对齐。
//!
//! 我们用 `window-vibrancy` 0.6 仅装 Mica（Win 11 22H2+，build 22621+）。**不**
//! 走 Acrylic 兜底：该 crate 自己警告 `apply_acrylic` 在 Win 10 v1903+ /
//! Win 11 build 22000 上 resize/drag 时窗口卡顿（DWM 私有 API 的已知问题），
//! 作为默认会把"白底"这点小事换成"拖动卡顿"这种新 bug。Win 10 / 早期 Win 11
//! 保持原 opaque 行为，等后续做 settings 开关再让用户显式 opt-in Acrylic。
//!
//! ## 与前端的协议
//!
//! 装配结果通过 [`MainWindowMaterial`] state 暴露给 webview——前端 `main.tsx`
//! 启动期调 [`crate::commands::window_material::get_main_window_material`]
//! 命令拿状态，根据结果在 `<html>` 上设 `data-uc-window-material="mica" | "none"`，
//! `globals.css` 据此切换 `--background` token 为透明 / 不透明。装失败 → 默认
//! 不设 attr，CSS 走 opaque 路径，与现状完全一致（macOS / Linux 零回归）。
//!
//! ## 装配时机
//!
//! 必须在 `setup` callback 内、main window 已经存在但尚未被 `show()` 之前调
//! [`apply_to_main_window`]：先装材质再 show，避免用户看到先白后透的闪烁。
//! show/hide（hide-to-tray 路径）不会丢材质——DWM attribute 跟 HWND 走，所以
//! 我们不需要在 `on_window_event` 里挂任何重新装配的逻辑。同理，active /
//! inactive 的灰过渡也由 DWM 系统自动处理。

use serde::Serialize;
use specta::Type;

/// 主窗口当前实际生效的 DWM 材质。
///
/// 这个 enum 是 Rust ↔ TypeScript 共享类型（specta 生成 binding），变体名直接
/// 序列化成小写字符串，前端拿到的就是 `"mica"` / `"none"`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum MainWindowMaterial {
    /// Mica 装配成功（仅 Win 11 22H2+）。前端切到透明 background token。
    Mica,
    /// 未装材质：非 Windows，或 Mica 不可用（Win 10 / 早期 Win 11）。前端
    /// 保持 opaque background。
    None,
}

impl MainWindowMaterial {
    /// 对应前端 CSS 的 `data-uc-window-material` attribute 值。
    pub fn as_attr(self) -> &'static str {
        match self {
            Self::Mica => "mica",
            Self::None => "none",
        }
    }
}

/// 进程级 main window 材质状态。setup 装完之后由 [`apply_to_main_window`]
/// 写入；前端通过 specta command 读。Tauri 用 `tauri::State` 模式 share，
/// 因为状态只在装配时写一次、之后只读，所以包一层 `std::sync::OnceLock` 不需要。
/// 直接 `app.manage(MainWindowMaterial)` 由 Tauri 内部 Mutex 保护。
pub type MainWindowMaterialState = MainWindowMaterial;

/// 尝试为主窗口装 Mica，返回最终生效的材质。
///
/// 非 Windows / Win 10 / 早期 Win 11 → 返回 [`MainWindowMaterial::None`]，
/// 调用方不需要做额外处理。失败时 `tracing::warn!` 记录原因，方便后续
/// 核对实际命中率；这个失败是预期的（系统版本不够），不应该升级到 error。
#[allow(unused_variables)] // window 在非 Windows 路径未使用
pub fn apply_to_main_window(window: &tauri::WebviewWindow) -> MainWindowMaterial {
    #[cfg(target_os = "windows")]
    {
        // dark=None 让 DWM 跟随系统外观偏好。我们前端的 light/dark 切换是
        // CSS-driven 的，这里跟系统对齐就够了——如果用户系统是浅色，Mica
        // 给浅色基色，反之亦然。如果将来前端要强制覆盖 dark 模式（而不
        // 跟随系统），再考虑在 settings 改变时调 clear_mica → apply_mica。
        match window_vibrancy::apply_mica(window, None) {
            Ok(()) => {
                tracing::info!("Mica window material applied to main window");
                MainWindowMaterial::Mica
            }
            Err(error) => {
                // 预期失败：Win 10、Win 11 22H2 之前、或运行在不支持的虚机里。
                // 用 info 而不是 warn —— 这是常态，不是异常。
                tracing::info!(
                    error = %error,
                    "Mica unavailable on this Windows version; main window stays opaque"
                );
                MainWindowMaterial::None
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        // macOS 走 tauri.conf 的 `windowEffects: ["hudWindow"]`（NSVisualEffectView）；
        // Linux 没有等价系统材质。两边都不进这个函数。
        MainWindowMaterial::None
    }
}
