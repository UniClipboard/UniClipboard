//! 主窗口系统材质装配 / 上报 + 状态查询。
//!
//! ## 为什么需要这个模块
//!
//! `tauri.conf.json` 配了 `windowEffects: ["hudWindow"]`（macOS 专属字段）+
//! `transparent: true`，本意是让窗口透出 NSVisualEffectView——但前端
//! `globals.css` 把 `html` 的 background 硬涂成 white、`body` 又走不透明的
//! `--background` token，结果是 macOS 上 NSVisualEffectView 一直被完全遮住、
//! 视觉无效；Windows 端则没有任何材质装配（windowEffects 不读）。两边事实上
//! 都是白底，只是机制不同。
//!
//! issue #699 / ship-readiness §D31 要求两端材质真正可见。修复思路是把
//! "什么材质生效"作为运行期事实由后端报上来、前端据此切 background token
//! 为透明，让底层材质透出来。
//!
//! ### Windows 侧
//!
//! 用 `window-vibrancy` 0.6 仅装 Mica（Win 11 22H2+，build 22621+）。**不**走
//! Acrylic 兜底：该 crate 自己警告 `apply_acrylic` 在 Win 10 v1903+ /
//! Win 11 build 22000 上 resize/drag 时窗口卡顿（DWM 私有 API 的已知问题），
//! 作为默认会把"白底"这点小事换成"拖动卡顿"这种新 bug。Win 10 / 早期 Win 11
//! 保持原 opaque 行为，等后续做 settings 开关再让用户显式 opt-in Acrylic。
//!
//! ### macOS 侧
//!
//! NSVisualEffectView 已经由 tauri.conf 的 `windowEffects: ["hudWindow"]` 装好，
//! 不需要在 Rust 侧再调系统 API；本模块只负责"上报 Hud 已就绪"，让前端 CSS
//! 跟 Mica 一样走透明 background。
//!
//! ### Linux 侧
//!
//! 没有等价系统材质，返回 [`MainWindowMaterial::None`]，前端走 opaque 兜底。
//!
//! ## 与前端的协议
//!
//! 装配结果通过 [`MainWindowMaterial`] state 暴露给 webview——前端 `main.tsx`
//! 启动期调 [`crate::commands::window_material::get_main_window_material`]
//! 命令拿状态，根据结果在 `<html>` 上设 `data-uc-window-material="mica" | "hud" | "none"`，
//! `globals.css` 据此切换 `--background` token 为透明 / 不透明。`mica` 与
//! `hud` 在 CSS 层走同一组透明 token（视觉效果都是"系统模糊背景透出来"），
//! `none` 保留 opaque，确保失败 / 不支持平台零回归。
//!
//! ## 装配时机
//!
//! 必须在 `setup` callback 内、main window 已经存在但尚未被 `show()` 之前调
//! [`apply_to_main_window`]：先装材质再 show，避免用户看到先白后透的闪烁。
//! show/hide（hide-to-tray 路径）不会丢材质——DWM attribute / NSVisualEffectView
//! 都跟窗口实例走，所以我们不需要在 `on_window_event` 里挂任何重新装配的逻辑。
//! 同理，active / inactive 的灰过渡都由系统自动处理。

use serde::Serialize;
use specta::Type;

/// 主窗口当前实际生效的系统材质。
///
/// 这个 enum 是 Rust ↔ TypeScript 共享类型（specta 生成 binding），变体名直接
/// 序列化成小写字符串，前端拿到的就是 `"mica"` / `"hud"` / `"none"`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum MainWindowMaterial {
    /// Windows 11 22H2+ 上 DWM Mica 装配成功。前端切到透明 background token。
    Mica,
    /// macOS NSVisualEffectView (`hudWindow`)。由 `tauri.conf.json` 在窗口
    /// 创建时装好，Rust 侧只是上报。前端切到透明 background token。
    Hud,
    /// 未装材质：Linux，或 Windows 上 Mica 不可用（Win 10 / 早期 Win 11）。
    /// 前端保持 opaque background，与历史行为一致。
    None,
}

impl MainWindowMaterial {
    /// 对应前端 CSS 的 `data-uc-window-material` attribute 值。
    pub fn as_attr(self) -> &'static str {
        match self {
            Self::Mica => "mica",
            Self::Hud => "hud",
            Self::None => "none",
        }
    }
}

/// 进程级 main window 材质状态。setup 装完之后由 [`apply_to_main_window`]
/// 写入；前端通过 specta command 读。Tauri 用 `tauri::State` 模式 share，
/// 因为状态只在装配时写一次、之后只读，所以包一层 `std::sync::OnceLock` 不需要。
/// 直接 `app.manage(MainWindowMaterial)` 由 Tauri 内部 Mutex 保护。
pub type MainWindowMaterialState = MainWindowMaterial;

/// 主窗口系统材质的"装配 + 上报"统一入口。
///
/// - **Windows**：尝试装 Mica；成功返回 [`MainWindowMaterial::Mica`]，失败
///   （Win 10 / 早期 Win 11）返回 [`MainWindowMaterial::None`]。
/// - **macOS**：NSVisualEffectView 已经在 `tauri.conf.json` 的 `windowEffects:
///   ["hudWindow"]` 里装好，本函数不调任何系统 API，只返回
///   [`MainWindowMaterial::Hud`] 告诉前端"材质已就绪、可以走透明 token"。
/// - **Linux**：没有等价系统材质，返回 [`MainWindowMaterial::None`]。
///
/// 调用方拿到结果后应该 `app.manage(...)` 让 `get_main_window_material`
/// command 能读到。
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

    #[cfg(target_os = "macos")]
    {
        // tauri.conf 里配的 `windowEffects: ["hudWindow"]` 在窗口创建时已经
        // 装好 NSVisualEffectView，这里不需要再调系统 API。但需要把这一事实
        // 上报给前端，否则前端走默认 opaque token 会把材质完全遮住——这是
        // issue #699 引入这套机制前 macOS 上"hudWindow 一直没有视觉效果"
        // 的根因。
        MainWindowMaterial::Hud
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        // Linux 没有等价的系统模糊背景；保持 opaque 与现状一致。
        MainWindowMaterial::None
    }
}
