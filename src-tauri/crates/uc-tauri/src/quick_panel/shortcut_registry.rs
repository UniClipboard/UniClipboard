//! [`uc_desktop::shortcuts::GlobalShortcutRegistry`] 的 Tauri 适配实现。
//!
//! 把"注册物理快捷键到 OS"的契约落到 `tauri-plugin-global-shortcut` 上。
//!
//! - 回调（按下时触发什么）由调用方在构造期注入；本适配器把它包装成 Tauri
//!   callback 签名（`Fn(&AppHandle, &Shortcut, ShortcutEvent)`），仅在
//!   [`ShortcutState::Pressed`] 时调一次。
//! - 同步实现，假设调用方已在 Tauri main thread 上下文（启动期 setup
//!   callback、或 command 内部用 `app.run_on_main_thread` 包过的闭包）。

use std::sync::Arc;

use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tracing::{error, info, warn};

use uc_desktop::shortcuts::{GlobalShortcutRegistry, ShortcutError};

/// Tauri 全局快捷键注册器。
///
/// 通过 `new` 注入按下回调；之后每次 `register` 都用同一个回调注册。
pub struct TauriGlobalShortcutRegistry {
    app: AppHandle,
    on_pressed: Arc<dyn Fn() + Send + Sync>,
}

impl TauriGlobalShortcutRegistry {
    pub fn new(app: AppHandle, on_pressed: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            app,
            on_pressed: Arc::new(on_pressed),
        }
    }
}

impl GlobalShortcutRegistry for TauriGlobalShortcutRegistry {
    fn register(&self, shortcut: &str) -> Result<(), ShortcutError> {
        // 防御性反注册：Windows 上 OS 级 hotkey 可能在前一次进程崩溃 / 强杀
        // 后仍残留，导致 "HotKey already registered"。
        if let Err(e) = self.app.global_shortcut().unregister(shortcut) {
            warn!(
                error = %e,
                shortcut = %shortcut,
                "Defensive unregister before registering global shortcut failed"
            );
        }

        let on_pressed = Arc::clone(&self.on_pressed);
        self.app
            .global_shortcut()
            .on_shortcut(shortcut, move |_app, _shortcut, event| {
                if event.state == ShortcutState::Pressed {
                    info!("Global shortcut triggered");
                    on_pressed();
                }
            })
            .map_err(|e| {
                error!(error = %e, shortcut = %shortcut, "Failed to register global shortcut");
                ShortcutError::backend(format!("Failed to register shortcut '{shortcut}': {e}"))
            })?;
        info!(shortcut = %shortcut, "Global shortcut registered");
        Ok(())
    }

    fn unregister(&self, shortcut: &str) -> Result<(), ShortcutError> {
        // 契约：未注册视为成功。`tauri-plugin-global-shortcut` 不区分
        // "未注册" 与其它后端错误，所以这里统一降级为 warn 并返回 Ok。
        if let Err(e) = self.app.global_shortcut().unregister(shortcut) {
            warn!(
                error = %e,
                shortcut = %shortcut,
                "Unregister global shortcut returned error; treating as no-op per trait contract"
            );
        }
        Ok(())
    }
}
