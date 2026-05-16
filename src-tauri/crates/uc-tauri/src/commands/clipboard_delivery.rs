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

use serde::Serialize;
use tracing::{info_span, Instrument};

use uc_application::facade::{
    EntryDeliveryStatusView, EntryDeliveryTargetView, EntryDeliveryView, EntrySource,
    GetEntryDeliveryViewError,
};
use uc_core::clipboard::DeliveryFailureReason;
use uc_core::ids::EntryId;
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
