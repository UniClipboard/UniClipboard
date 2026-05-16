//! 组合多个 [`HostEventEmitterPort`] 的"扇出"实现。
//!
//! 为什么需要这个模块:
//! GUI shell 进程内通常需要把同一个 [`HostEvent`] 同时送到多个下游 ——
//! 比如同时写本地日志 + 推 daemon WS + 推 Tauri webview。如果让每条
//! emit 路径自己组合,各装配点都要再实现一次 "for-each + warn-on-err"
//! 的样板,且 daemon 在启动时把 emitter cell 整个 swap 成自己专属
//! 实现就会丢掉 GUI shell 已经装好的其它 emitter。统一收口到一个
//! `CompositeHostEventEmitter`,新增 emitter 永远是"在已有 emitter 之上
//! 包一层",不会破坏既有装配。
//!
//! 失败语义:某个下游 emit 失败不阻塞其它下游;每条失败按 `warn!` 落日
//! 志(带下游 index 与错误内容)。`emit` 的返回值始终 `Ok(())` —— 真正
//! 关心可观测性的下游会把 emit 失败映射成 metric / log,不应再把错误
//! 反弹回应用层。

use std::sync::Arc;

use tracing::warn;

use super::{EmitError, HostEvent, HostEventEmitterPort};

/// 把多个 [`HostEventEmitterPort`] 串成一个对外契约不变的 emitter。
///
/// `inner` 的顺序就是 emit 顺序;构造期固定,运行时不再变化。如果需要
/// "增量挂入",建议在装配处读出旧 cell 内容,与新 emitter 一起包一层
/// 新的 `CompositeHostEventEmitter` 再写回。
pub struct CompositeHostEventEmitter {
    inner: Vec<Arc<dyn HostEventEmitterPort>>,
}

impl CompositeHostEventEmitter {
    /// 用一组 emitter 构造扇出 emitter。空 `Vec` 合法 —— 等价于一个 noop
    /// emitter,用于测试或"未来再注入"的占位场景。
    pub fn new(inner: Vec<Arc<dyn HostEventEmitterPort>>) -> Self {
        Self { inner }
    }

    /// 在 `base` 之上追加 `extra`,产出新的扇出 emitter。装配处常用形态:
    /// 读出 cell 当前值 → `append(current, new)` → 写回 cell。
    pub fn append(
        base: Arc<dyn HostEventEmitterPort>,
        extra: Arc<dyn HostEventEmitterPort>,
    ) -> Self {
        Self {
            inner: vec![base, extra],
        }
    }
}

impl HostEventEmitterPort for CompositeHostEventEmitter {
    fn emit(&self, event: HostEvent) -> Result<(), EmitError> {
        for (idx, emitter) in self.inner.iter().enumerate() {
            if let Err(err) = emitter.emit(event.clone()) {
                warn!(
                    emitter_index = idx,
                    error = %err,
                    "composite host event emitter: downstream emit failed"
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
        use crate::facade::host_event::{DeliveryHostEvent, DeliveryStatusKind};
        HostEvent::Delivery(DeliveryHostEvent::StatusChanged {
            entry_id: "entry-1".to_string(),
            target_device_id: "peer-a".to_string(),
            new_status: DeliveryStatusKind::Delivered,
        })
    }

    #[test]
    fn emits_to_all_downstreams_in_order() {
        let a = Arc::new(Recorder::default());
        let b = Arc::new(Recorder::default());
        let composite = CompositeHostEventEmitter::new(vec![
            Arc::clone(&a) as Arc<dyn HostEventEmitterPort>,
            Arc::clone(&b) as Arc<dyn HostEventEmitterPort>,
        ]);

        composite.emit(sample_event()).expect("composite emit");

        assert_eq!(a.events.lock().unwrap().len(), 1);
        assert_eq!(b.events.lock().unwrap().len(), 1);
    }

    #[test]
    fn downstream_failure_does_not_stop_others() {
        let recorder = Arc::new(Recorder::default());
        let composite = CompositeHostEventEmitter::new(vec![
            Arc::new(Failing) as Arc<dyn HostEventEmitterPort>,
            Arc::clone(&recorder) as Arc<dyn HostEventEmitterPort>,
        ]);

        // 即使第一个 emitter 报错,第二个仍要被调用,且整体返回 Ok。
        composite.emit(sample_event()).expect("composite emit ok");
        assert_eq!(recorder.events.lock().unwrap().len(), 1);
    }

    #[test]
    fn empty_composite_is_noop() {
        let composite = CompositeHostEventEmitter::new(Vec::new());
        composite.emit(sample_event()).expect("composite emit ok");
    }
}
