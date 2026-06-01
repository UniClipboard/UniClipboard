//! macOS 唤醒源：`NSBackgroundActivityScheduler`。
//!
//! 后台周期检查原本只靠 `scheduler` 的 `tokio::sleep` 驱动。但当 app 是无 Dock
//! 的 `Accessory` 后台进程、主窗口隐藏/关闭时，macOS **App Nap** 会挂起 tokio
//! 定时器，那条周期检查实际上几乎从不触发——历史症状正是「只有打开主窗口才检测、
//! 才弹更新窗口」（打开主窗口会把 app 翻成 `Regular`、解除 App Nap，被挂起的检查
//! 才得以发车）。前几次只修了「窗口怎么浮到最前」这一层，没修「检查根本没发车」
//! 这一层，所以反复无效。
//!
//! `NSBackgroundActivityScheduler` 是 Apple 给「节能的周期性后台活动」提供的官方
//! API（Sparkle 同款）：它在 App Nap 下也会触发，能与系统唤醒合并以省电，并在从
//! 睡眠恢复后补跑。每次 fire 往 `wake_tx` 推一下，把 `scheduler` 主循环从被挂起
//! 的 sleep 里叫醒去跑一次真正的检查。
//!
//! Foundation 对象在主线程构造（与本 crate 其它 AppKit 代码一致）；构造好的
//! `Retained` 句柄 `mem::forget` 泄漏到进程生命周期——`drop` 会 `invalidate`
//! 这个活动，而我们要它一直 fire 到进程退出。

use std::time::Duration;

use block2::RcBlock;
use objc2::AllocAnyThread;
use objc2_foundation::{
    NSBackgroundActivityCompletionHandler, NSBackgroundActivityResult,
    NSBackgroundActivityScheduler, NSString,
};
use tauri::AppHandle;
use tokio::sync::mpsc::Sender;
use tracing::warn;

/// 活动标识符。Apple 建议用反向域名风格、进程内稳定不变。
const ACTIVITY_IDENTIFIER: &str = "app.uniclipboard.update-check";

/// 注册后台活动调度器。`interval` 为期望的检查周期（取 scheduler 的成功 cadence
/// 基准，6h）；`tolerance` 给系统 ~10% 的合并窗口去和别的唤醒拼车省电。
///
/// fire-and-forget：dispatch 到主线程构造，失败仅 warn——失败时 scheduler 退回
/// 纯 tokio cadence（后台可能被 App Nap 拖慢，但前台仍可用）。
pub fn start(app: &AppHandle, wake_tx: Sender<()>, interval: Duration) {
    let interval_secs = interval.as_secs_f64();
    if let Err(err) = app.run_on_main_thread(move || {
        let identifier = NSString::from_str(ACTIVITY_IDENTIFIER);
        let scheduler = NSBackgroundActivityScheduler::initWithIdentifier(
            NSBackgroundActivityScheduler::alloc(),
            &identifier,
        );
        scheduler.setRepeats(true);
        scheduler.setInterval(interval_secs);
        scheduler.setTolerance(interval_secs * 0.1);

        let block = RcBlock::new(move |completion: NSBackgroundActivityCompletionHandler| {
            // 运行在 NSBackgroundActivityScheduler 的私有后台队列线程上。
            // try_send 失败：Full（已有 pending tick）或 Closed（scheduler task
            // 已退出）都无害，直接忽略。
            let _ = wake_tx.try_send(());
            // 告诉系统这次活动已完成，让它按 interval 排下一次。
            if !completion.is_null() {
                // SAFETY: `completion` 由系统传入，指向有效的
                // NSBackgroundActivity 完成回调 block；仅在本回调内调用一次。
                unsafe {
                    (*completion).call((NSBackgroundActivityResult::Finished,));
                }
            }
        });
        // SAFETY: block 只捕获 Send 的 `wake_tx`，满足 `scheduleWithBlock` 的
        // “block must be sendable” 要求。
        unsafe {
            scheduler.scheduleWithBlock(&block);
        }

        // 持有到进程结束：drop 会 invalidate 活动。泄漏即「进程生命周期」——
        // 正是想要的触发寿命。`block` 已被 scheduler 内部 copy/retain，本地副本
        // 可随作用域 drop。
        std::mem::forget(scheduler);
    }) {
        warn!(
            target: "update_scheduler",
            error = %err,
            "failed to dispatch NSBackgroundActivityScheduler setup to the main thread; \
             background update checks fall back to cadence-only"
        );
    }
}
