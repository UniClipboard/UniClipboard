//! Windows 唤醒源：系统恢复（resume）通知。
//!
//! Modern Standby / 睡眠期间进程被冻结、`QueryPerformanceCounter` 暂停，`scheduler`
//! 的 6h 单调 `tokio::sleep` 跨过整段休眠也走不完，导致后台周期检查迟迟不发车
//! （与 macOS App Nap 同类的「后台不检测」问题）。
//!
//! 这里用 `RegisterSuspendResumeNotification` + `DEVICE_NOTIFY_CALLBACK` 注册一个
//! 系统恢复回调——**无需窗口，也无需消息泵**，系统在自有线程直接回调（message-only
//! 窗口收不到 `WM_POWERBROADCAST` 广播，所以走回调而非窗口句柄）。从睡眠/待机
//! 恢复时往 `wake_tx` 推一下，把 `scheduler` 叫醒去补一次检查；配合 scheduler 侧
//! 的墙钟 guard（`WAKE_MIN_RECHECK_SECS`），短暂休眠后恢复不会重复打 CDN。

use tokio::sync::mpsc::Sender;
use tracing::{info, warn};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Power::{
    RegisterSuspendResumeNotification, DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DEVICE_NOTIFY_CALLBACK, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND,
};

/// 系统电源事件回调。系统可能在任意线程调用它。
///
/// `context` 是 [`start`] 里 `Box::into_raw` 出来的 `Sender<()>`——随注册一起
/// 泄漏，注册存活期间一直有效。
unsafe extern "system" fn on_power_event(
    context: *const core::ffi::c_void,
    event_type: u32,
    _setting: *const core::ffi::c_void,
) -> u32 {
    // 只关心「恢复」类事件；挂起 / 其它一律忽略。
    if (event_type == PBT_APMRESUMEAUTOMATIC || event_type == PBT_APMRESUMESUSPEND)
        && !context.is_null()
    {
        // SAFETY: `context` 指向 start() 泄漏的 Sender<()>，注册期间有效。
        let tx = unsafe { &*(context as *const Sender<()>) };
        // Full（已有 pending tick）/ Closed（scheduler task 已退出）都无害。
        let _ = tx.try_send(());
    }
    0 // ERROR_SUCCESS
}

/// 注册系统恢复通知。fire-and-forget：失败仅 warn，scheduler 退回纯 cadence。
pub fn start(wake_tx: Sender<()>) {
    // 泄漏 sender 到进程生命周期：回调可能在进程存活期内任意时刻被系统调用。
    let ctx = Box::into_raw(Box::new(wake_tx)) as *mut core::ffi::c_void;

    // DEVICE_NOTIFY_CALLBACK 模式下，hRecipient 指向本结构体；系统在注册时读取
    // Callback + Context 并内部保存，结构体本身无需活过本次调用，栈上即可。
    let mut params = DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
        Callback: Some(on_power_event),
        Context: ctx,
    };

    // SAFETY: params 在调用期间有效；DEVICE_NOTIFY_CALLBACK 要求 hRecipient 指向
    // DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS。
    let result = unsafe {
        RegisterSuspendResumeNotification(
            HANDLE(
                &mut params as *mut DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS as *mut core::ffi::c_void,
            ),
            DEVICE_NOTIFY_CALLBACK,
        )
    };

    match result {
        Ok(_handle) => {
            // _handle（HPOWERNOTIFY）泄漏，不 Unregister：随进程退出由系统回收。
            info!(
                target: "update_scheduler",
                "registered Windows suspend/resume notification for update wake"
            );
        }
        Err(err) => {
            // 注册失败：回收 sender，scheduler 退回纯 cadence。
            // SAFETY: ctx 来自上面的 Box::into_raw，注册失败时未被任何回调接管。
            drop(unsafe { Box::from_raw(ctx as *mut Sender<()>) });
            warn!(
                target: "update_scheduler",
                error = %err,
                "failed to register Windows suspend/resume notification; \
                 background update checks fall back to cadence-only"
            );
        }
    }
}
