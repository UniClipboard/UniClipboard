//! Tauri 端 [`HostEventEmitterPort`] 实现 —— 把 application 层语义事件
//! 翻译成 tauri-specta 事件推到前端。
//!
//! 为什么需要这个模块:
//! Phase 5(Issue #747)之前,GUI 进程内注入的是 `LoggingHostEventEmitter`,
//! 所有 `HostEvent` 只打 log,不会到达前端。结果是后台 dispatch 已经把
//! delivery 状态写入,但 detail 视图的 EntryDeliveryBadge 还停留在"等待
//! 同步",必须切 entry / reload 才能刷新。本模块是修复这一断链的基础设施
//! 入口:`TauriHostEventEmitter` 拿 `AppHandle` 后,把 `HostEvent::Delivery`
//! 翻成 typed tauri 事件 emit 给前端,前端订阅后据 entry_id 重新拉 view。
//!
//! ## 范围
//!
//! 当前实现 **只** 翻译 [`HostEvent::Delivery`]。Clipboard / Transfer 两类
//! 事件在 daemon 链路已经通过 `DaemonApiEventEmitter` 推 WS,前端历史代码
//! 也走 WS 订阅 —— 本 emitter 留空跳过,避免重复推送 / 前端去重。后续要不
//! 要把它们也走 Tauri 通道是独立决策(见 Issue #747 "非目标")。
//!
//! ## 事件 payload 形态
//!
//! 事件只携带 `(entry_id, target_device_id)`,**不带 status**。前端订阅
//! 后按 entry_id 匹配,匹配则 refetch view —— view 永远是 status 的真相源,
//! 事件本身只是"该不该 refetch"的指针。让事件 payload 和 view DTO 同步
//! 携带 status 副本会形成两份并行的 wire enum,新增 variant 必须双改且
//! drift 时编译器无感,所以这里刻意不带 status,见 `DeliveryHostEvent` 的
//! 注释。

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_specta::Event;
use tracing::warn;
use uc_application::facade::{DeliveryHostEvent, EmitError, HostEvent, HostEventEmitterPort};

/// `clipboard_delivery_status_changed` 事件 payload。
///
/// 前端 detail 视图按 `entry_id` 过滤后 refetch `GET /clipboard/entries/{id}/delivery`
/// (ADR-008 P3-1 起走 daemon loopback API),拿 view 内的 status 渲染。
/// `target_device_id` 当前未被消费,留作未来 per-peer 局部刷新的钩子。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardDeliveryStatusChanged {
    /// 触发本次变更的 entry id。前端 detail 视图按当前打开的 entry_id 过滤,
    /// 只对匹配事件 refetch。
    pub entry_id: String,
    /// 投递目标对端。view 渲染时按对端聚合状态,事件粒度也是按对端。
    pub target_device_id: String,
}

/// Tauri 端 emitter:`AppHandle` 在 setup callback 之后才可用,所以构造期
/// 直接持 `AppHandle`(`Clone`,内部已是 `Arc`)。`HostEventEmitterPort::emit`
/// 是同步 trait,Tauri `emit` 接口同样同步,直接转发即可。
pub struct TauriHostEventEmitter {
    handle: AppHandle,
}

impl TauriHostEventEmitter {
    pub fn new(handle: AppHandle) -> Self {
        Self { handle }
    }
}

impl HostEventEmitterPort for TauriHostEventEmitter {
    fn emit(&self, event: HostEvent) -> Result<(), EmitError> {
        match event {
            HostEvent::Delivery(DeliveryHostEvent::StatusChanged {
                entry_id,
                target_device_id,
            }) => {
                let payload = ClipboardDeliveryStatusChanged {
                    entry_id,
                    target_device_id,
                };
                if let Err(err) = payload.emit(&self.handle) {
                    // emit 失败按 emitter port 契约返回 EmitError;上层
                    // (`HostEventBus`)会 warn 并继续 fan-out 给其它下游,
                    // dispatch 主路径不感知。
                    warn!(error = %err, "tauri host event emitter: emit failed");
                    return Err(EmitError::Failed(err.to_string()));
                }
            }
            // 其它事件类别本 emitter 不接管;bus 上注册的其它下游
            // (logging 已经覆盖、daemon WS 也会推过)负责。保留 silent
            // Ok 避免重复推送给前端造成抖动。
            HostEvent::Clipboard(_) | HostEvent::Transfer(_) => {}
        }
        Ok(())
    }
}
