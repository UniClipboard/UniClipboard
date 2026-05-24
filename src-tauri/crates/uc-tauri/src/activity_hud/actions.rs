//! HUD 用户动作抽象。
//!
//! ## 目的
//!
//! 让 UI 平台模块 (`ui/macos.rs` / `ui/windows.rs` / …) 不直接依赖
//! `TauriAppRuntime`、`uc_application::facade`、`uc_core::*` 之类的领
//! 域类型。平台代码只调 [`ActivityHudActions::cancel`] 这种"动作动词",
//! 装配代码负责把动作翻译成"乐观状态更新 + 真正向 facade 发请求"。
//!
//! 这层抽象的边界:
//! - **平台模块拿不到的**:emitter / facade / Tauri State
//! - **平台模块拿得到的**:`Arc<dyn ActivityHudActions>` + 平台自有的
//!   UI 上下文(AppHandle、窗口句柄等)
//!
//! ## 为什么不直接让平台调 facade
//!
//! 1. 跨平台 (Windows / Linux) 复用同一份"取消"语义,避免每个平台
//!    各自处理乐观 UI 反馈 + 错误日志。
//! 2. facade 签名演化时(未来加参数 / 重命名)只改装配代码一处,平台
//!    模块不动。
//! 3. 测试时可以用 mock `ActivityHudActions` 验证 UI 行为,不需要起
//!    真实的 facade。

use std::sync::Arc;

use tauri::{AppHandle, Manager};
use tracing::warn;
use uc_core::FileTransferCancellationReason;

use crate::bootstrap::TauriAppRuntime;

use super::emitter::ActivityHudEmitter;

/// HUD 用户动作。所有方法都是 fire-and-forget:动作内部应自行处理
/// 乐观 UI 状态、异步执行、错误日志。调用方不等待结果。
pub trait ActivityHudActions: Send + Sync {
    /// 用户在 HUD 上请求取消某条传输。语义包括:
    /// 1. 状态机上立即把行切到 `CancelPending` (乐观 UI 反馈)
    /// 2. 向后端发出实际取消请求 (`facade.cancel_inbound_transfer`)
    /// 3. 后端落地的 `StatusChanged: cancelled` 会通过 host event bus
    ///    回流到状态机,把行最终切到 `Cancelled`
    fn cancel(&self, transfer_id: &str);
}

/// 默认实现:把"乐观状态更新"和"调 facade"两件事捏在一起。
///
/// 装配代码用 `Arc::new` 包一份给 UI 平台模块。如果未来要支持 daemon
/// 重启时切换 facade 实现,这里可以换成 Weak<emitter> + Mutex<facade>,
/// 但当前进程级单例不需要。
pub struct DefaultActivityHudActions {
    emitter: Arc<ActivityHudEmitter>,
    app_handle: AppHandle,
}

impl DefaultActivityHudActions {
    pub fn new(emitter: Arc<ActivityHudEmitter>, app_handle: AppHandle) -> Self {
        Self {
            emitter,
            app_handle,
        }
    }
}

impl ActivityHudActions for DefaultActivityHudActions {
    fn cancel(&self, transfer_id: &str) {
        // 1) 乐观切到 CancelPending —— UI 立即反馈,不等 facade 回应。
        self.emitter.mark_cancel_pending(transfer_id);

        // 2) 真正发出取消请求。spawn 到 Tauri 自维护的 runtime;facade
        //    是 async,主线程不能 block 等待。
        let transfer_id = transfer_id.to_string();
        let app_handle = self.app_handle.clone();
        tauri::async_runtime::spawn(async move {
            let runtime: Arc<TauriAppRuntime> =
                app_handle.state::<Arc<TauriAppRuntime>>().inner().clone();
            if let Err(err) = runtime
                .app_facade()
                .cancel_inbound_transfer(&transfer_id, FileTransferCancellationReason::LocalUser)
                .await
            {
                warn!(
                    error = %err,
                    transfer_id = %transfer_id,
                    "activity_hud: cancel_inbound_transfer failed"
                );
            }
        });
    }
}
