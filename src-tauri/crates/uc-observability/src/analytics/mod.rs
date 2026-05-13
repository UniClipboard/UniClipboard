//! 产品 telemetry 事件的类型骨架与上报抽象。
//!
//! 详细 schema、隐私契约与命名规范见
//! `docs/architecture/telemetry-events.md`。本模块只承担类型定义与
//! sink 抽象，不耦合任何具体后端（PostHog Cloud SDK 接入将作为独立的
//! adapter 实现 [`AnalyticsPort`]，详见后续 Slice）。
//!
//! ## 边界
//!
//! - 本模块**不允许**读取或派生自 `uc-core::DeviceId`——见 schema doc §3.1。
//! - 本模块的所有事件类型为 pure data，可被 `uc-application` 的 use case
//!   直接构造，再交给一个实现 [`AnalyticsPort`] 的 adapter 上报。
//! - 任何事件构造前调用方都必须先查询
//!   [`crate::analytics_gate::is_analytics_enabled`]，关闭时连事件对象都
//!   不应该被构造。

pub mod context;
pub mod events;
pub mod ids;
pub mod port;
pub mod probe;
pub mod sinks;

pub use context::{
    build_event_context, clear_global_event_context, global_event_context,
    set_global_event_context, AppChannel, Arch, EventContext, EventContextInputs, InstallSource,
    Os,
};
pub use events::{
    Direction, Event, FailureReason, LatencyBucket, NameLengthBucket, PairingFailureReason,
    PairingMethod, PayloadSizeBucket, PayloadType, SetupEntry, SyncDeferReason, SyncDeferredProps,
    SyncEventProps, SyncFailureStage, TransportType,
};
pub use ids::{load_or_create as load_or_create_ids, reset as reset_ids, AnalyticsIds};
pub use port::{AnalyticsPort, NoopAnalyticsSink};
pub use sinks::{build_event_payload, GatedAnalyticsSink, PosthogSink, StdoutSink};
