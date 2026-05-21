//! Entry delivery view —— GUI detail 面板的"来源 + 每对端同步状态"查询命令。
//!
//! 为什么需要这个模块:
//! Phase 1 已经把"投递事实"沉到 `EntryDeliveryRepositoryPort` + view use case,
//! `ClipboardSyncFacade::get_entry_delivery_view` 在 in-process facade 上透出
//! 完整视图。前端要在 quick-panel 与主窗口两套 detail 上挂"来自哪台设备 /
//! 同步到了哪些设备 / 哪台失败"区域,只需要一个跨 IPC 的薄读命令。
//!
//! 命令走 in-process facade(`runtime.app_facade()`),不经 HTTP/webserver。
//! DTO 在本文件就地定义、camelCase 序列化,语义与 use case 输出 1:1 对应;
//! 不进 `uc-daemon-contract`,因为这是 GUI-only 路径,没有 LAN/mobile 协议。

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tracing::{info_span, Instrument};

use uc_application::facade::{
    EntryDeliveryStatusView, EntryDeliveryTargetView, EntryDeliveryView, EntrySource,
    GetEntryDeliveryViewError, NotResendableReason, ResendEntryCommand, ResendEntryError,
    ResendReport,
};
use uc_core::clipboard::DeliveryFailureReason;
use uc_core::ids::{DeviceId, EntryId};
use uc_platform::ports::observability::TraceMetadata;

use crate::bootstrap::TauriAppRuntime;
use crate::commands::error::CommandError;
use crate::commands::record_trace_fields;

/// `EntryDeliveryView` 的前端可序列化镜像。字段命名与领域模型保持一致,
/// 只在序列化时改写成 camelCase。
#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EntryDeliveryViewDto {
    pub entry_id: String,
    pub source: EntrySourceDto,
    pub deliveries: Vec<EntryDeliveryTargetDto>,
}

/// entry 来源描述。`tag` 字段供前端 discriminated union 直接 switch。
#[derive(Debug, Serialize, specta::Type)]
#[serde(tag = "tag", rename_all = "camelCase")]
pub enum EntrySourceDto {
    /// 本机捕获。
    Local,
    /// 远端推送。`deviceId` 是来源设备,`deviceName` 取自空间成员目录;
    /// 不命中时为 `null`,前端 fallback 到 device_id 截断。
    Remote {
        #[serde(rename = "deviceId")]
        device_id: String,
        #[serde(rename = "deviceName")]
        device_name: Option<String>,
    },
    /// 追踪机制启用前已存在的老 entry,无可靠投递信息。
    Historical,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EntryDeliveryTargetDto {
    pub target_device_id: String,
    /// 取自空间成员目录中的人类可读名;不命中时为 `null`,前端 fallback
    /// 到 `targetDeviceId` 截断。
    pub target_device_name: Option<String>,
    pub status: EntryDeliveryStatusDto,
    /// 失败时的 wire 层错误细节,供 UI tooltip / 详情展开使用。
    pub reason_detail: Option<String>,
    /// `Pending` 时为 `None`(从未尝试过)。`#[specta(type = ...)]` 告诉 specta
    /// 这是个 ms 精度的 epoch,实际值不会超过 JS Number 安全整数范围 (~285 万年),
    /// 用 `Number<i64>` 包装绕过 BigInt 禁用规则,与 mobile_sync.rs 的 `last_seen_at_ms`
    /// 同源。
    #[specta(type = Option<specta_typescript::Number<i64>>)]
    pub updated_at_ms: Option<i64>,
}

/// 状态枚举:`tag` + `reason` 形式,便于前端区分四档与失败子分类。
#[derive(Debug, Serialize, specta::Type)]
#[serde(tag = "tag", rename_all = "camelCase")]
pub enum EntryDeliveryStatusDto {
    Pending,
    Delivered,
    Duplicate,
    Failed {
        #[serde(rename = "reason")]
        reason: DeliveryFailureReasonDto,
    },
}

/// 失败原因。i18n key 命名约定:`delivery.failureReason.<variant 小驼峰>`。
#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum DeliveryFailureReasonDto {
    Offline,
    LocalPolicy,
    PeerRejected,
    Io,
    Internal,
}

impl From<EntryDeliveryView> for EntryDeliveryViewDto {
    fn from(view: EntryDeliveryView) -> Self {
        Self {
            entry_id: view.entry_id.as_str().to_string(),
            source: view.source.into(),
            deliveries: view.deliveries.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<EntrySource> for EntrySourceDto {
    fn from(source: EntrySource) -> Self {
        match source {
            EntrySource::Local => EntrySourceDto::Local,
            EntrySource::Remote {
                device_id,
                device_name,
            } => EntrySourceDto::Remote {
                device_id: device_id.as_str().to_string(),
                device_name,
            },
            EntrySource::Historical => EntrySourceDto::Historical,
        }
    }
}

impl From<EntryDeliveryTargetView> for EntryDeliveryTargetDto {
    fn from(target: EntryDeliveryTargetView) -> Self {
        Self {
            target_device_id: target.target_device_id.as_str().to_string(),
            target_device_name: target.target_device_name,
            status: target.status.into(),
            reason_detail: target.reason_detail,
            updated_at_ms: target.updated_at_ms,
        }
    }
}

impl From<EntryDeliveryStatusView> for EntryDeliveryStatusDto {
    fn from(status: EntryDeliveryStatusView) -> Self {
        match status {
            EntryDeliveryStatusView::Pending => EntryDeliveryStatusDto::Pending,
            EntryDeliveryStatusView::Delivered => EntryDeliveryStatusDto::Delivered,
            EntryDeliveryStatusView::Duplicate => EntryDeliveryStatusDto::Duplicate,
            EntryDeliveryStatusView::Failed { reason } => EntryDeliveryStatusDto::Failed {
                reason: reason.into(),
            },
        }
    }
}

impl From<DeliveryFailureReason> for DeliveryFailureReasonDto {
    fn from(reason: DeliveryFailureReason) -> Self {
        match reason {
            DeliveryFailureReason::Offline => DeliveryFailureReasonDto::Offline,
            DeliveryFailureReason::LocalPolicy => DeliveryFailureReasonDto::LocalPolicy,
            DeliveryFailureReason::PeerRejected => DeliveryFailureReasonDto::PeerRejected,
            DeliveryFailureReason::Io => DeliveryFailureReasonDto::Io,
            DeliveryFailureReason::Internal => DeliveryFailureReasonDto::Internal,
        }
    }
}

impl From<GetEntryDeliveryViewError> for CommandError {
    fn from(err: GetEntryDeliveryViewError) -> Self {
        match err {
            GetEntryDeliveryViewError::EntryNotFound(id) => {
                CommandError::NotFound(format!("entry not found: {id}"))
            }
            GetEntryDeliveryViewError::Storage(msg) => CommandError::InternalError(msg),
        }
    }
}

/// 拉一条 entry 的同步状态视图。
///
/// 前端 detail 面板在 entry 切换时调用,失败时(facade 未装配 / DB 故障)
/// 返回 `InternalError`;entry 不存在返回 `NotFound`,前端据此降级渲染。
#[tauri::command]
#[specta::specta]
pub async fn clipboard_entry_delivery_view(
    runtime: tauri::State<'_, Arc<TauriAppRuntime>>,
    entry_id: String,
    _trace: Option<TraceMetadata>,
) -> Result<EntryDeliveryViewDto, CommandError> {
    let span = info_span!(
        "command.clipboard.entry_delivery_view",
        entry_id = %entry_id,
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    let runtime = runtime.inner().clone();
    async move {
        let entry = EntryId::from_string(entry_id);
        let view = runtime.app_facade().get_entry_delivery_view(&entry).await?;
        Ok(EntryDeliveryViewDto::from(view))
    }
    .instrument(span)
    .await
}

// ============================================================================
// Resend command —— ADR-005 Stage 1a, commit E
// ============================================================================
//
// 与 `clipboard_entry_delivery_view` 共域 (entry delivery)，但不复用
// `CommandError`:resend 的 6 个 `ResendEntryError` 变体需要前端按 `code`
// 精细 i18n,而 `CommandError::Conflict(String)` 只能编码单字符串。
// 跟随 `mobile_sync.rs` 的 typed-enum 模板,前端拿到 `{ code, ... }` 直接
// pattern match。

/// `clipboard_resend_entry` 入参。
///
/// `targetDeviceIds`:
/// - 字段缺失 / `null` → `None` → use case 派生 `trusted_peer \
///   (Delivered ∪ Duplicate)` 差集；
/// - `[]` → `Some(vec![])`，等价于零目标，use case 返回 `NoEligibleTargets`；
/// - `["dev-a", "dev-b"]` → 仅向列出的设备重发，列表里的 device 必须在
///   trusted_peer 列表内，否则返回 `TargetNotTrusted`。
#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ResendEntryArgs {
    pub entry_id: String,
    #[serde(default)]
    pub target_device_ids: Option<Vec<String>>,
}

/// `clipboard_resend_entry` 成功返回 —— fan-out 后的聚合计数。语义同
/// [`ResendReport`](uc_application::facade::ResendReport)，camelCase 后给前端
/// 渲染 toast / detail badge。
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ResendEntryReportDto {
    /// 已落 `Delivered` 投递记录的目标数。
    #[specta(type = specta_typescript::Number<usize>)]
    pub accepted: usize,
    /// 对端确认为重复内容 (content_hash 已存在) 的目标数。
    #[specta(type = specta_typescript::Number<usize>)]
    pub duplicate: usize,
    /// 对端不可达 (presence offline / dial 失败) 的目标数。
    #[specta(type = specta_typescript::Number<usize>)]
    pub offline: usize,
    /// 其他错误 (peer rejected / IO / internal) 的目标数。
    #[specta(type = specta_typescript::Number<usize>)]
    pub errored: usize,
    /// fan-out deadline 内未 settle、被搬到后台继续 join 的目标数。
    /// 后台完成时会写 delivery record 并发 `ClipboardDeliveryStatusChanged`
    /// 事件,前端 detail badge 据此自动刷新。
    #[specta(type = specta_typescript::Number<usize>)]
    pub pending: usize,
}

/// `clipboard_resend_entry` 失败时的 typed 错误。前端 `error.code` 即
/// discriminator,各变体的额外字段对应 ADR §2.5.4 的失败语义。
///
/// 不复用 `CommandError`:`Conflict(String)` 只能塞一段文本,而本错误
/// 集合需要 `deviceId` / `reason` / `entryId` 等结构化字段供 i18n key
/// 选择与文案占位。参考 `mobile_sync::MobileSyncError` 同模式。
#[derive(Debug, Clone, Serialize, specta::Type, thiserror::Error)]
#[serde(
    tag = "code",
    rename_all = "SCREAMING_SNAKE_CASE",
    rename_all_fields = "camelCase"
)]
pub enum ResendEntryCommandError {
    #[error("entry not found: {entry_id}")]
    EntryNotFound { entry_id: String },

    #[error("entry {entry_id} is not resendable: {reason:?}")]
    EntryNotResendable {
        entry_id: String,
        reason: NotResendableReasonDto,
    },

    #[error("target device {device_id} is not a trusted peer")]
    TargetNotTrusted { device_id: String },

    #[error("no eligible targets for resend")]
    NoEligibleTargets,

    #[error("storage failure: {message}")]
    Storage { message: String },

    #[error("dispatch failure: {message}")]
    Dispatch { message: String },
}

/// 不可重发的细分原因。i18n key 命名约定:`delivery.resend.error.notResendable.<variant>`。
#[derive(Debug, Clone, Copy, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum NotResendableReasonDto {
    /// entry 来自远端 peer。视图层已对远端 entry 隐藏重发按钮;走到此处
    /// 说明前端绕过视图直接调命令,或视图状态过期。
    RemoteOrigin,
    /// 本机已不持有 plaintext / 必要 blob (paste rep `Lost` / 文件被 GC /
    /// blob store 已清理)。前端提示用户该 entry 已无法重新发送。
    PayloadLost,
}

impl From<NotResendableReason> for NotResendableReasonDto {
    fn from(reason: NotResendableReason) -> Self {
        match reason {
            NotResendableReason::RemoteOrigin => Self::RemoteOrigin,
            NotResendableReason::PayloadLost => Self::PayloadLost,
        }
    }
}

impl From<ResendEntryError> for ResendEntryCommandError {
    fn from(err: ResendEntryError) -> Self {
        match err {
            ResendEntryError::EntryNotFound(id) => Self::EntryNotFound {
                entry_id: id.inner().clone(),
            },
            ResendEntryError::EntryNotResendable { entry_id, reason } => Self::EntryNotResendable {
                entry_id: entry_id.inner().clone(),
                reason: reason.into(),
            },
            ResendEntryError::TargetNotTrusted(device_id) => Self::TargetNotTrusted {
                device_id: device_id.as_str().to_string(),
            },
            ResendEntryError::NoEligibleTargets => Self::NoEligibleTargets,
            ResendEntryError::Storage(message) => Self::Storage { message },
            ResendEntryError::Dispatch(message) => Self::Dispatch { message },
        }
    }
}

impl From<ResendReport> for ResendEntryReportDto {
    fn from(report: ResendReport) -> Self {
        Self {
            accepted: report.accepted,
            duplicate: report.duplicate,
            offline: report.offline,
            errored: report.errored,
            pending: report.pending,
        }
    }
}

/// 用户主动 resend 一条本机 entry。命令走 in-process facade
/// (`AppFacade::resend_entry` → `ClipboardOutboundFacade::resend_entry`),
/// 与 `clipboard_entry_delivery_view` 一致;不经 HTTP/webserver。
///
/// 入参:
/// - `entryId` —— 要重发的 entry id (字符串形态);
/// - `targetDeviceIds` —— `None` 派生差集,`Some(list)` 显式 fan-out;
///   `Some(vec![])` 与差集为空等价,返回 `NO_ELIGIBLE_TARGETS`。
///
/// 返回 `ResendEntryReportDto` (各 bucket 计数);失败返回 typed
/// `ResendEntryCommandError`,前端按 `code` 字段做 i18n。详细错误语义见
/// [`ResendEntryError`](uc_application::facade::ResendEntryError)。
#[tauri::command]
#[specta::specta]
pub async fn clipboard_resend_entry(
    runtime: tauri::State<'_, Arc<TauriAppRuntime>>,
    args: ResendEntryArgs,
    _trace: Option<TraceMetadata>,
) -> Result<ResendEntryReportDto, ResendEntryCommandError> {
    let span = info_span!(
        "command.clipboard.resend_entry",
        entry_id = %args.entry_id,
        target_count = args
            .target_device_ids
            .as_ref()
            .map(Vec::len)
            .unwrap_or(0),
        filter_mode = args.target_device_ids.is_some(),
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    let runtime = runtime.inner().clone();
    async move {
        let cmd = ResendEntryCommand {
            entry_id: EntryId::from_string(args.entry_id),
            target_filter: args
                .target_device_ids
                .map(|ids| ids.into_iter().map(DeviceId::new).collect()),
        };
        let report = runtime.app_facade().resend_entry(cmd).await?;
        Ok(ResendEntryReportDto::from(report))
    }
    .instrument(span)
    .await
}

#[cfg(test)]
mod resend_tests {
    //! 边界层 wiring:error 翻译表 + Tauri-friendly DTO 序列化形态。
    //! 业务语义在 `uc-application::usecases::clipboard_sync::resend_entry`
    //! 的 7 个 verdict 已覆盖。

    use super::*;

    #[test]
    fn report_serializes_with_camel_case_keys() {
        let report = ResendReport {
            accepted: 2,
            duplicate: 1,
            offline: 0,
            errored: 0,
            pending: 1,
        };
        let dto = ResendEntryReportDto::from(report);
        let json = serde_json::to_value(dto).expect("dto serializes");

        assert_eq!(json["accepted"], 2);
        assert_eq!(json["duplicate"], 1);
        assert_eq!(json["offline"], 0);
        assert_eq!(json["errored"], 0);
        assert_eq!(json["pending"], 1);
    }

    #[test]
    fn entry_not_found_maps_with_entry_id_payload() {
        let err = ResendEntryError::EntryNotFound(EntryId::from_str("ent-missing"));
        let mapped: ResendEntryCommandError = err.into();
        let json = serde_json::to_value(&mapped).expect("error serializes");

        assert_eq!(json["code"], "ENTRY_NOT_FOUND");
        assert_eq!(json["entryId"], "ent-missing");
    }

    #[test]
    fn entry_not_resendable_propagates_reason_discriminator_and_entry_id() {
        let remote = ResendEntryError::EntryNotResendable {
            entry_id: EntryId::from_str("ent-remote"),
            reason: NotResendableReason::RemoteOrigin,
        };
        let lost = ResendEntryError::EntryNotResendable {
            entry_id: EntryId::from_str("ent-lost"),
            reason: NotResendableReason::PayloadLost,
        };

        let remote_json =
            serde_json::to_value::<ResendEntryCommandError>(remote.into()).expect("ok");
        let lost_json = serde_json::to_value::<ResendEntryCommandError>(lost.into()).expect("ok");

        assert_eq!(remote_json["code"], "ENTRY_NOT_RESENDABLE");
        assert_eq!(remote_json["reason"], "remoteOrigin");
        assert_eq!(remote_json["entryId"], "ent-remote");
        assert_eq!(lost_json["code"], "ENTRY_NOT_RESENDABLE");
        assert_eq!(lost_json["reason"], "payloadLost");
        assert_eq!(lost_json["entryId"], "ent-lost");
    }

    #[test]
    fn target_not_trusted_carries_device_id() {
        let err = ResendEntryError::TargetNotTrusted(DeviceId::new("dev-ghost"));
        let mapped: ResendEntryCommandError = err.into();
        let json = serde_json::to_value(&mapped).expect("ok");

        assert_eq!(json["code"], "TARGET_NOT_TRUSTED");
        assert_eq!(json["deviceId"], "dev-ghost");
    }

    #[test]
    fn no_eligible_targets_has_no_extra_fields() {
        let err = ResendEntryError::NoEligibleTargets;
        let mapped: ResendEntryCommandError = err.into();
        let json = serde_json::to_value(&mapped).expect("ok");

        assert_eq!(json["code"], "NO_ELIGIBLE_TARGETS");
        // Only the `code` discriminator field is present.
        let obj = json.as_object().expect("object");
        assert_eq!(obj.len(), 1, "extra fields: {obj:?}");
    }

    #[test]
    fn storage_and_dispatch_propagate_message() {
        let storage = ResendEntryError::Storage("db down".to_string());
        let dispatch = ResendEntryError::Dispatch("encrypt session locked".to_string());

        let storage_json =
            serde_json::to_value::<ResendEntryCommandError>(storage.into()).expect("ok");
        let dispatch_json =
            serde_json::to_value::<ResendEntryCommandError>(dispatch.into()).expect("ok");

        assert_eq!(storage_json["code"], "STORAGE");
        assert_eq!(storage_json["message"], "db down");
        assert_eq!(dispatch_json["code"], "DISPATCH");
        assert_eq!(dispatch_json["message"], "encrypt session locked");
    }

    #[test]
    fn args_deserializes_with_camel_case_and_optional_filter() {
        let no_filter: ResendEntryArgs =
            serde_json::from_str(r#"{"entryId":"ent-1"}"#).expect("no filter");
        assert_eq!(no_filter.entry_id, "ent-1");
        assert!(no_filter.target_device_ids.is_none());

        let null_filter: ResendEntryArgs =
            serde_json::from_str(r#"{"entryId":"ent-1","targetDeviceIds":null}"#)
                .expect("null filter");
        assert!(null_filter.target_device_ids.is_none());

        let empty_filter: ResendEntryArgs =
            serde_json::from_str(r#"{"entryId":"ent-1","targetDeviceIds":[]}"#)
                .expect("empty filter");
        assert_eq!(empty_filter.target_device_ids, Some(vec![]));

        let explicit: ResendEntryArgs =
            serde_json::from_str(r#"{"entryId":"ent-1","targetDeviceIds":["dev-a","dev-b"]}"#)
                .expect("explicit list");
        assert_eq!(
            explicit.target_device_ids,
            Some(vec!["dev-a".to_string(), "dev-b".to_string()])
        );
    }
}
