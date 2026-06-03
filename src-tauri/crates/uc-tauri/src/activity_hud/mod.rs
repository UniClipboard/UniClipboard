//! macOS / 未来跨平台原生文件接收 HUD。
//!
//! ## 目的
//!
//! 当从其他设备 (Windows / Linux 对端) 接收文件到本机时,弹出一个独立
//! 的原生窗口显示进度,类似 AirDrop 的接收浮窗。这一层独立于主 webview
//! 中已有的 `TransferProgressBar`,两者并存:主窗口里仍有详细列表与历
//! 史,HUD 提供随手可见的进度 + 取消入口。
//!
//! ## 模块分层
//!
//! ```text
//!   daemon WS → DaemonWsBridge → HostEvent (run.rs)
//!         │
//!         ▼
//!   emitter (跨平台)
//!         │
//!         ▼
//!   state (跨平台)
//!         │
//!         ▼ snapshot
//!   ui::ActivityHudListener (平台特定)
//!         │
//!         │ 用户点取消
//!         ▼
//!   actions::ActivityHudActions (跨平台)
//!         │
//!         ▼
//!   facade.cancel_inbound_transfer
//! ```
//!
//! - [`clock`]:单调时钟抽象,生产代码用 `Instant::now()`,单测用手动
//!   推进时钟。
//! - [`state`]:纯逻辑状态机,接收 host event,输出行快照。无 AppKit、
//!   无 host event 类型依赖以外的副作用,可完整单元测试。
//! - [`emitter`]:`HostEventEmitterPort` 适配器,把 host event bus 上
//!   的事件喂给状态机,并通过 [`emitter::ActivityHudListener`] 通知 UI。
//! - [`actions`]:用户动作抽象 ([`actions::ActivityHudActions`]),把
//!   "取消"等动词从 UI 平台代码里抽出来,平台模块不直接依赖 facade。
//! - [`ui`]:平台特定 listener 集合。`ui::macos` 是 AppKit panel,
//!   `ui::tracing` 是非 macOS 平台的日志 fallback。加 Windows 端时
//!   在这里新增 `ui::windows`。
//!
//! ## 装配
//!
//! 上层 (run.rs) 调一次 [`install`] 拿到 emitter,再用 `DaemonWsBridge`
//! 把 daemon WS 上的 transfer / incoming-pending 事件翻成 `HostEvent`
//! 喂给它 (ADR-008 P3-3 B2'-3 —— GUI 已无 in-process host_event_bus):
//!
//! ```ignore
//! let hud = activity_hud::install(activity_hud::InstallDeps {
//!     app_handle: app.handle().clone(),
//! });
//! // run.rs: bridge.subscribe([FileTransfer, Clipboard]) → translate → hud.emit(..)
//! ```
//!
//! 内部完成:
//! 1. 构造 state + emitter (placeholder listener,两阶段装配)
//! 2. 构造 actions (持 emitter)
//! 3. 创建平台 listener (持 actions)
//! 4. `emitter.set_listener(real)` 完成接线
//! 5. spawn 后台 sweep tick 周期清理终态行

pub mod actions;
pub mod clock;
pub mod emitter;
pub mod state;
pub mod ui;

use std::sync::Arc;
use std::time::Duration;

use tauri::AppHandle;

use self::actions::{ActivityHudActions, DefaultActivityHudActions};
use self::clock::{Clock, SystemClock};
use self::emitter::{ActivityHudEmitter, ActivityHudListener};

/// 装配 HUD 需要的外部依赖。所有字段都从 Tauri setup callback 拿得到。
pub struct InstallDeps {
    pub app_handle: AppHandle,
}

/// 后台 sweep tick 周期 —— 清理过保留期的终态行。500ms 对 2-4s 的保留
/// 期是 4-8 倍精度,行的"完成→消失"过渡看不出抖动。
const SWEEP_INTERVAL: Duration = Duration::from_millis(500);

/// 一站式装配:构造状态机 / emitter / actions / 平台 listener,接线到
/// host event bus,启动后台 sweep tick。返回 emitter handle,主要供测
/// 试与排障使用;运行期通常不需要。
///
/// 装配顺序处理了一个隐含的 Arc 环 (emitter → listener → actions →
/// emitter):用 placeholder listener 先把 emitter 立起来,然后构造真
/// 实 listener,最后 `set_listener` 替换。**TODO**:可以用
/// `Arc::new_cyclic` + `Weak` 把环改成无循环;当前 emitter 进程级单例
/// 活到进程退出,内存不漏。
pub fn install(deps: InstallDeps) -> Arc<ActivityHudEmitter> {
    let InstallDeps { app_handle } = deps;

    // 1) 状态机 + emitter,先用 tracing listener 占位。
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let placeholder: Arc<dyn ActivityHudListener> =
        Arc::new(self::ui::tracing::TracingActivityHudListener);
    let emitter = Arc::new(ActivityHudEmitter::new(clock, placeholder));

    // 2) 默认 actions(乐观更新 + 调 facade),依赖 emitter。
    let actions: Arc<dyn ActivityHudActions> = Arc::new(DefaultActivityHudActions::new(
        Arc::clone(&emitter),
        app_handle.clone(),
    ));

    // 3) 平台 listener(macOS 真实 HUD / 其它平台 tracing 兜底)。
    let real_listener = self::ui::create_listener(app_handle, actions);
    emitter.set_listener(real_listener);

    // 4) 后台 sweep tick:周期清理过保留期的终态行。MissedTickBehavior::Skip
    //    避免 runtime 偶发卡顿后追补一堆 tick。
    //
    // ADR-008 P3-3 (B2'-3): emitter 不再注册到进程内 host_event_bus —— GUI
    // 已无 in-process bus。事件改由 run.rs 的 `DaemonWsBridge` 从 daemon WS
    // 拉取后翻成 HostEvent 喂给返回的 emitter。
    let sweep_emitter = Arc::clone(&emitter);
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(SWEEP_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            sweep_emitter.tick();
        }
    });

    emitter
}
