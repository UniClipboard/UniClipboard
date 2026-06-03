//! `HostEventEmitterPort` 实现:把 host event bus 上的事件喂给状态机,
//! 然后把变化通知到 listener。
//!
//! ## 接入位置
//!
//! ADR-008 P3-3 (B2'-3): GUI 已是外部 daemon 的纯客户端,无 in-process
//! host_event_bus。`run.rs` 用 `DaemonWsBridge` 订阅 daemon WS 的
//! file-transfer + clipboard topic,把 `RealtimeEvent` 翻成 `HostEvent`
//! 后调本 emitter 的 [`emit`](HostEventEmitterPort::emit)。本 emitter 只
//! 消费 Transfer + IncomingPending,其它事件类别静默跳过。
//!
//! ## 节流
//!
//! `emit` 每次都通知 listener。状态机本身不做节流 —— 节流由 listener
//! (例如 AppKit 端的渲染器)按 UI 帧率自己合并。这样:
//! 1. 状态机保持纯逻辑无时间副作用,单测好写;
//! 2. 不同后端 listener (tracing log / AppKit) 节流策略不同,放在各自
//!    实现里更合适。

use std::sync::{Arc, Mutex};

use tracing::warn;
use uc_application::facade::{
    ClipboardHostEvent, EmitError, HostEvent, HostEventEmitterPort, TransferHostEvent,
};

use super::clock::Clock;
use super::state::{ActivityHudRow, ActivityHudState};

/// HUD 渲染端订阅 trait。emitter 在状态机有变化时调用,参数是当前
/// 完整快照(已稳定排序)。实现必须 `Send + Sync` —— emit 是同步路径
/// 但来自任意 publisher 线程,listener 不得阻塞。
///
/// 实现位置:平台特定的 listener 在 `super::ui::*` 子模块下,装配时由
/// [`super::ui::create_listener`] 按 cfg 选出对应实现。
pub trait ActivityHudListener: Send + Sync {
    fn on_changed(&self, snapshot: Vec<ActivityHudRow>);
}

pub struct ActivityHudEmitter {
    state: Arc<Mutex<ActivityHudState>>,
    /// listener 用 `Mutex<Arc<dyn>>` 持有,因为 macOS 装配路径需要先构造
    /// emitter 才能构造 listener(listener 反向持有 emitter 用于 cancel
    /// 按钮路径)。简单的"构造时一次定下"做不到,所以暴露
    /// [`set_listener`](Self::set_listener) 让装配代码后置替换。读多写
    /// 极少(只在 setup 时写一次),`Mutex` 性能开销可忽略。
    listener: Mutex<Arc<dyn ActivityHudListener>>,
}

impl ActivityHudEmitter {
    pub fn new(clock: Arc<dyn Clock>, listener: Arc<dyn ActivityHudListener>) -> Self {
        Self {
            state: Arc::new(Mutex::new(ActivityHudState::new(clock))),
            listener: Mutex::new(listener),
        }
    }

    /// 替换内部 listener。装配阶段用 —— 见 emitter 文档里两阶段装配的
    /// 说明。运行期间不要在 publisher 路径上调用。
    pub fn set_listener(&self, listener: Arc<dyn ActivityHudListener>) {
        let mut guard = match self.listener.lock() {
            Ok(g) => g,
            Err(poisoned) => {
                warn!("activity_hud listener mutex poisoned; recovering inner");
                poisoned.into_inner()
            }
        };
        *guard = listener;
    }

    fn current_listener(&self) -> Arc<dyn ActivityHudListener> {
        let guard = match self.listener.lock() {
            Ok(g) => g,
            Err(poisoned) => {
                warn!("activity_hud listener mutex poisoned; recovering inner");
                poisoned.into_inner()
            }
        };
        Arc::clone(&*guard)
    }

    /// 暴露内部状态句柄,给 `cancel` 按钮路径用 (UI 点击取消 ->
    /// `mark_cancel_pending` 乐观切状态 -> 走 daemon `cancel-transfer`
    /// 端点真正发出取消)。出 emitter 之外尽量别拿这个,大多数场景应该走事件管道。
    pub fn state_handle(&self) -> Arc<Mutex<ActivityHudState>> {
        Arc::clone(&self.state)
    }

    /// 用户在 HUD 上点了某行的取消按钮 —— 乐观把行切到 `CancelPending`,
    /// UI 立刻显示"取消中…",真正的 `Cancelled` 由后端
    /// `status_changed: cancelled` 落地。调用方还应另外调
    /// `facade.cancel_inbound_transfer(...)` 真正发出取消请求。
    pub fn mark_cancel_pending(&self, transfer_id: &str) {
        self.apply(|state| state.mark_cancel_pending(transfer_id));
    }

    /// 给后台 sweep 任务调用:扫掉过保留期的终态行,如有变化通知 listener。
    pub fn tick(&self) {
        let snapshot_opt = {
            let mut state = match self.state.lock() {
                Ok(guard) => guard,
                Err(poisoned) => {
                    warn!("activity_hud state mutex poisoned; recovering inner");
                    poisoned.into_inner()
                }
            };
            if state.sweep() {
                Some(state.snapshot())
            } else {
                None
            }
        };
        if let Some(snapshot) = snapshot_opt {
            self.current_listener().on_changed(snapshot);
        }
    }

    /// 内部:拿锁、apply 闭包、判断是否需要通知;期间 listener 不持锁。
    fn apply<F>(&self, f: F)
    where
        F: FnOnce(&mut ActivityHudState) -> bool,
    {
        let snapshot_opt = {
            let mut state = match self.state.lock() {
                Ok(guard) => guard,
                Err(poisoned) => {
                    warn!("activity_hud state mutex poisoned; recovering inner");
                    poisoned.into_inner()
                }
            };
            let changed = f(&mut state);
            if changed {
                Some(state.snapshot())
            } else {
                None
            }
        };
        if let Some(snapshot) = snapshot_opt {
            self.current_listener().on_changed(snapshot);
        }
    }
}

impl HostEventEmitterPort for ActivityHudEmitter {
    fn emit(&self, event: HostEvent) -> Result<(), EmitError> {
        match event {
            HostEvent::Transfer(TransferHostEvent::Progress {
                transfer_id,
                entry_id: _,
                peer_id,
                direction,
                bytes_transferred,
                total_bytes,
            }) => {
                self.apply(|state| {
                    state.apply_progress(
                        &transfer_id,
                        &peer_id,
                        direction,
                        bytes_transferred,
                        total_bytes,
                    )
                });
            }
            HostEvent::Transfer(TransferHostEvent::StatusChanged {
                transfer_id,
                entry_id: _,
                status,
                reason,
            }) => {
                self.apply(|state| state.apply_status_changed(&transfer_id, &status, reason));
            }
            HostEvent::Clipboard(ClipboardHostEvent::IncomingPending {
                entry_id,
                from_device: _,
                total_bytes,
                filenames,
            }) => {
                // 协议约定 transfer_id == entry_id,所以直接用 entry_id 作行键。
                self.apply(|state| state.apply_incoming_pending(&entry_id, filenames, total_bytes));
            }
            // 其它事件类别 (Delivery / Clipboard::NewContent) HUD 不消费。
            HostEvent::Clipboard(_) | HostEvent::Delivery(_) => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex as StdMutex;

    use uc_application::facade::{
        ClipboardHostEvent, HostEvent, HostEventEmitterPort, TransferHostEvent,
    };
    use uc_core::file_transfer::FileTransferDirection;

    use super::super::clock::ManualClock;
    use super::*;

    #[derive(Default)]
    struct Recorder {
        calls: StdMutex<Vec<Vec<ActivityHudRow>>>,
    }

    impl Recorder {
        fn snapshots(&self) -> Vec<Vec<ActivityHudRow>> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl ActivityHudListener for Recorder {
        fn on_changed(&self, snapshot: Vec<ActivityHudRow>) {
            self.calls.lock().unwrap().push(snapshot);
        }
    }

    fn make_emitter() -> (ActivityHudEmitter, Arc<Recorder>, Arc<ManualClock>) {
        let clock = Arc::new(ManualClock::new());
        let listener = Arc::new(Recorder::default());
        let emitter = ActivityHudEmitter::new(
            clock.clone() as Arc<dyn Clock>,
            listener.clone() as Arc<dyn ActivityHudListener>,
        );
        (emitter, listener, clock)
    }

    fn progress_event(
        transfer_id: &str,
        peer_id: &str,
        direction: FileTransferDirection,
        bytes: u64,
        total: Option<u64>,
    ) -> HostEvent {
        HostEvent::Transfer(TransferHostEvent::Progress {
            transfer_id: transfer_id.into(),
            entry_id: Some(transfer_id.into()),
            peer_id: peer_id.into(),
            direction,
            bytes_transferred: bytes,
            total_bytes: total,
        })
    }

    #[test]
    fn receiving_progress_triggers_listener() {
        let (emitter, recorder, _clock) = make_emitter();
        emitter
            .emit(progress_event(
                "t1",
                "peer-a",
                FileTransferDirection::Receiving,
                100,
                Some(1000),
            ))
            .unwrap();
        let snaps = recorder.snapshots();
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].len(), 1);
        assert_eq!(snaps[0][0].transfer_id, "t1");
    }

    #[test]
    fn sending_progress_does_not_trigger_listener() {
        let (emitter, recorder, _clock) = make_emitter();
        emitter
            .emit(progress_event(
                "t1",
                "peer-a",
                FileTransferDirection::Sending,
                100,
                Some(1000),
            ))
            .unwrap();
        assert!(recorder.snapshots().is_empty());
    }

    #[test]
    fn incoming_pending_then_progress_joins_filenames() {
        let (emitter, recorder, _clock) = make_emitter();
        emitter
            .emit(HostEvent::Clipboard(ClipboardHostEvent::IncomingPending {
                entry_id: "t1".into(),
                from_device: "win-laptop".into(),
                total_bytes: Some(2048),
                filenames: vec!["a.txt".into(), "b.txt".into()],
            }))
            .unwrap();
        emitter
            .emit(progress_event(
                "t1",
                "peer-a",
                FileTransferDirection::Receiving,
                100,
                Some(2048),
            ))
            .unwrap();
        let last = recorder.snapshots().pop().unwrap();
        assert_eq!(
            last[0].filenames,
            Some(vec!["a.txt".into(), "b.txt".into()])
        );
    }

    #[test]
    fn status_changed_completed_then_sweep_clears() {
        let (emitter, recorder, clock) = make_emitter();
        emitter
            .emit(progress_event(
                "t1",
                "peer-a",
                FileTransferDirection::Receiving,
                100,
                Some(100),
            ))
            .unwrap();
        emitter
            .emit(HostEvent::Transfer(TransferHostEvent::StatusChanged {
                transfer_id: "t1".into(),
                entry_id: "t1".into(),
                status: "completed".into(),
                reason: None,
            }))
            .unwrap();
        clock.advance(super::super::state::COMPLETED_RETAIN_MS + 100);
        emitter.tick();
        let last = recorder.snapshots().pop().unwrap();
        assert!(last.is_empty(), "sweep 后行应被清空");
    }

    #[test]
    fn outbound_status_changed_is_silently_dropped() {
        let (emitter, recorder, _clock) = make_emitter();
        // 没有先发 Progress,所以行不存在 —— StatusChanged 应被丢弃,
        // 不触发 listener。
        emitter
            .emit(HostEvent::Transfer(TransferHostEvent::StatusChanged {
                transfer_id: "t-outbound".into(),
                entry_id: "t-outbound".into(),
                status: "cancelled".into(),
                reason: Some("local_user".into()),
            }))
            .unwrap();
        assert!(recorder.snapshots().is_empty());
    }
}
