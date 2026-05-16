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

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_specta::Event;
use tracing::warn;
use uc_application::facade::{
    DeliveryHostEvent, DeliveryStatusKind, EmitError, HostEvent, HostEventEmitterPort,
};
use uc_core::clipboard::DeliveryFailureReason;

/// `clipboard_delivery_status_changed` 事件 payload。
///
/// 字段命名与 [`crate::commands::clipboard_delivery::EntryDeliveryViewDto`] 的
/// 同名子结构保持一致:`tag` 字段供前端 discriminated union 直接 switch,
/// 与 view DTO 共用同一份 i18n key 与渲染分支,避免出现"事件枚举一套、
/// view 枚举另一套"的两份口径。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardDeliveryStatusChanged {
    /// 触发本次变更的 entry id。前端 detail 视图按当前打开的 entry_id 过滤,
    /// 只对匹配事件 refetch。
    pub entry_id: String,
    /// 投递目标对端。前端按对端聚合渲染状态,所以 payload 粒度也是按对端。
    pub target_device_id: String,
    /// 投递的新状态,wire 形状与 `EntryDeliveryStatusDto` 同源。
    pub status: ClipboardDeliveryStatusPayload,
}

/// 与命令侧 `EntryDeliveryStatusDto` 等价的事件版镜像。两边定义分开是因为
/// 命令 DTO 持有的是 view 层的合成态(含 `Pending`),事件这边只承载写路
/// 径真实产生的三档,把 `Pending` 排除在外让订阅方少处理一个永远不会到达
/// 的分支。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(tag = "tag", rename_all = "camelCase")]
pub enum ClipboardDeliveryStatusPayload {
    Delivered,
    Duplicate,
    Failed {
        #[serde(rename = "reason")]
        reason: DeliveryFailureReasonPayload,
    },
}

/// 失败原因。变体集合与 `DeliveryFailureReasonDto` 1:1,i18n key 复用同一组。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum DeliveryFailureReasonPayload {
    Offline,
    LocalPolicy,
    PeerRejected,
    Io,
    Internal,
}

impl From<DeliveryFailureReason> for DeliveryFailureReasonPayload {
    fn from(reason: DeliveryFailureReason) -> Self {
        match reason {
            DeliveryFailureReason::Offline => Self::Offline,
            DeliveryFailureReason::LocalPolicy => Self::LocalPolicy,
            DeliveryFailureReason::PeerRejected => Self::PeerRejected,
            DeliveryFailureReason::Io => Self::Io,
            DeliveryFailureReason::Internal => Self::Internal,
        }
    }
}

impl From<DeliveryStatusKind> for ClipboardDeliveryStatusPayload {
    fn from(kind: DeliveryStatusKind) -> Self {
        match kind {
            DeliveryStatusKind::Delivered => Self::Delivered,
            DeliveryStatusKind::Duplicate => Self::Duplicate,
            DeliveryStatusKind::Failed { reason } => Self::Failed {
                reason: reason.into(),
            },
        }
    }
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
                new_status,
            }) => {
                let payload = ClipboardDeliveryStatusChanged {
                    entry_id,
                    target_device_id,
                    status: new_status.into(),
                };
                if let Err(err) = payload.emit(&self.handle) {
                    // emit 失败按 emitter port 契约只能返回 EmitError, 但
                    // 我们想避免 dispatch 主路径上看到 Err(因为它本就被吞)
                    // —— 这里 warn + 返回字符串化的错误,让 composite 上游
                    // (CompositeHostEventEmitter)能继续 fan-out 给其它
                    // 下游(daemon WS 等)。
                    warn!(error = %err, "tauri host event emitter: emit failed");
                    return Err(EmitError::Failed(err.to_string()));
                }
            }
            // 其它事件类别本 emitter 不接管;composite 的其它下游(Logging
            // 已经覆盖、daemon WS 已经推过)会负责。保留 silent Ok 避免
            // 重复推送给前端造成抖动。
            HostEvent::Clipboard(_) | HostEvent::Transfer(_) => {}
        }
        Ok(())
    }
}
