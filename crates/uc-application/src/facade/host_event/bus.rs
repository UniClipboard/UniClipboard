//! 注册式 host-event fan-out 总线。
//!
//! 为什么需要这个模块:
//! GUI shell 进程内通常需要把同一个 [`HostEvent`] 同时送到多个下游 ——
//! 本地日志、daemon WS、Tauri webview。早期实现把 emitter 放进
//! `RwLock<Arc<dyn HostEventEmitterPort>>` cell,后接入方用
//! "读出旧值 + 包一层 Composite 再写回" 的方式串接,结果形成嵌套洋葱
//! (`Composite[Composite[Logging, Tauri], DaemonWs]`),装配顺序敏感、
//! 不能注销、emit 时多层 vtable。
//!
//! `HostEventBus` 把多输出建模回它真正的形态 —— 一个有名字的注册表:
//!
//! - 装配:`bus.register("daemon_ws", ...)` —— 显式声明"我在哪个名字下挂了下游"。
//! - 注销:`bus.unregister("daemon_ws")` —— daemon reload 时 GUI emitter
//!   不受影响。
//! - emit:fan-out 到 `Vec`,某个下游失败仅 `warn!` 不阻塞其它下游。
//!
//! Bus 自身实现 [`HostEventEmitterPort`],所以 cell 历史上持有
//! `Arc<dyn HostEventEmitterPort>` 的地方可以无缝替换为 `Arc<HostEventBus>`,
//! 同时仍能从 [`emit_or_warn`](Self::emit_or_warn) 拿到统一的
//! "emit + 只 warn 不抛错"helper —— 调用方不再各自拷一份。

use std::sync::{Arc, RwLock};

use tracing::warn;

use super::{EmitError, HostEvent, HostEventEmitterPort};

/// 一个挂在 bus 上的 emitter,带名字便于排障与精准注销。
struct Registered {
    name: &'static str,
    emitter: Arc<dyn HostEventEmitterPort>,
}

/// fan-out 注册表。线程安全 —— `register` / `unregister` 走 write 锁,
/// `emit` 走 read 锁,运行期热路径是 read-only。
#[derive(Default)]
pub struct HostEventBus {
    inner: RwLock<Vec<Registered>>,
}

impl HostEventBus {
    pub fn new() -> Self {
        Self::default()
    }

    /// 把一个 emitter 挂到 bus 上。`name` 用于日志(哪条下游 emit 失败)与
    /// 后续 `unregister`。同名重复注册会保留 **最后一次** —— 与"daemon
    /// reload 时新 emitter 替换旧 emitter"的语义一致。
    pub fn register(&self, name: &'static str, emitter: Arc<dyn HostEventEmitterPort>) {
        let mut inner = self.inner.write().unwrap_or_else(|p| p.into_inner());
        inner.retain(|r| r.name != name);
        inner.push(Registered { name, emitter });
    }

    /// 移除 `name` 对应的 emitter。未注册则 no-op。
    pub fn unregister(&self, name: &'static str) {
        let mut inner = self.inner.write().unwrap_or_else(|p| p.into_inner());
        inner.retain(|r| r.name != name);
    }

    /// `emit`,但失败只 `warn!` 不返回 `Err`。给可观测性副作用路径
    /// (dispatch / apply_inbound / publisher)统一收口,避免每条 use
    /// case 各自拷一份 "read cell + emit + warn" 样板。
    pub fn emit_or_warn(&self, event: HostEvent) {
        if let Err(err) = self.emit(event) {
            // emit 自身永远返回 Ok(每个 downstream 的失败已在内部 warn 过),
            // 这一支理论上不会进入;留 warn 是防御性 —— 真有变体进入说明
            // 上游契约被破坏。
            warn!(error = %err, "host event bus emit returned Err unexpectedly");
        }
    }
}

impl HostEventEmitterPort for HostEventBus {
    fn emit(&self, event: HostEvent) -> Result<(), EmitError> {
        // read 锁不阻止并发 emit;snapshot vec 用一次拷贝(几个 Arc)替代
        // "持锁 fan-out",避免下游 emitter 在 callback 里再调 register
        // 时死锁(同进程内 Tauri / daemon 不会这么做,但 deadlock 一旦
        // 发生极难诊断,显式 clone 一份更稳)。
        let snapshot: Vec<Registered> = {
            let inner = self.inner.read().unwrap_or_else(|p| p.into_inner());
            inner
                .iter()
                .map(|r| Registered {
                    name: r.name,
                    emitter: Arc::clone(&r.emitter),
                })
                .collect()
        };
        for entry in snapshot.iter() {
            if let Err(err) = entry.emitter.emit(event.clone()) {
                warn!(
                    emitter = entry.name,
                    error = %err,
                    "host event bus: downstream emit failed"
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    struct Recorder {
        events: Mutex<Vec<HostEvent>>,
    }
    impl HostEventEmitterPort for Recorder {
        fn emit(&self, event: HostEvent) -> Result<(), EmitError> {
            self.events.lock().unwrap().push(event);
            Ok(())
        }
    }

    struct Failing;
    impl HostEventEmitterPort for Failing {
        fn emit(&self, _event: HostEvent) -> Result<(), EmitError> {
            Err(EmitError::Failed("boom".to_string()))
        }
    }

    fn sample_event() -> HostEvent {
        use crate::facade::host_event::{DeliveryHostEvent, HostEvent};
        HostEvent::Delivery(DeliveryHostEvent::StatusChanged {
            entry_id: "entry-1".to_string(),
            target_device_id: "peer-a".to_string(),
        })
    }

    #[test]
    fn registered_emitters_receive_events() {
        let bus = HostEventBus::new();
        let a = Arc::new(Recorder::default());
        let b = Arc::new(Recorder::default());
        bus.register("a", Arc::clone(&a) as Arc<dyn HostEventEmitterPort>);
        bus.register("b", Arc::clone(&b) as Arc<dyn HostEventEmitterPort>);

        bus.emit(sample_event()).expect("emit ok");

        assert_eq!(a.events.lock().unwrap().len(), 1);
        assert_eq!(b.events.lock().unwrap().len(), 1);
    }

    #[test]
    fn downstream_failure_does_not_stop_others() {
        let bus = HostEventBus::new();
        let recorder = Arc::new(Recorder::default());
        bus.register("fail", Arc::new(Failing) as Arc<dyn HostEventEmitterPort>);
        bus.register("ok", Arc::clone(&recorder) as Arc<dyn HostEventEmitterPort>);

        bus.emit(sample_event()).expect("bus emit ok");
        assert_eq!(recorder.events.lock().unwrap().len(), 1);
    }

    #[test]
    fn unregister_removes_emitter() {
        let bus = HostEventBus::new();
        let a = Arc::new(Recorder::default());
        bus.register("a", Arc::clone(&a) as Arc<dyn HostEventEmitterPort>);
        bus.unregister("a");

        bus.emit(sample_event()).expect("emit ok");
        assert!(a.events.lock().unwrap().is_empty());
    }

    #[test]
    fn same_name_replaces_previous() {
        let bus = HostEventBus::new();
        let first = Arc::new(Recorder::default());
        let second = Arc::new(Recorder::default());
        bus.register("tauri", Arc::clone(&first) as Arc<dyn HostEventEmitterPort>);
        bus.register(
            "tauri",
            Arc::clone(&second) as Arc<dyn HostEventEmitterPort>,
        );

        bus.emit(sample_event()).expect("emit ok");
        assert!(first.events.lock().unwrap().is_empty());
        assert_eq!(second.events.lock().unwrap().len(), 1);
    }

    #[test]
    fn empty_bus_is_noop() {
        let bus = HostEventBus::new();
        bus.emit(sample_event()).expect("emit ok");
    }
}
