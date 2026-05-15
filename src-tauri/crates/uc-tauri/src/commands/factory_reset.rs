//! Factory-reset Tauri command —— "重置并重新开始" 兜底入口。
//!
//! 用户在 UnlockPage 上点 "重置并重新开始" 后调用本 command。语义:
//!
//! 1. 调 `SpaceSetupFacade::factory_reset()` —— 删 keyslot + KEK,清
//!    `SetupStatus`,取消任何 pending invitations。
//! 2. 成功后 `EncryptionFacade::state()` 会返回 `initialized=false`,
//!    `App.tsx` 的渲染分支自然把 UI 切回 `SetupPage`。
//!
//! 与 `space_setup::unlock_space_with_passphrase` 同走 in-process facade
//! 路径(无 HTTP / TCP 往返)。`SpaceSetupFacade` 已是 §11.4 合规的对外
//! 入口,本文件作为 Tauri 边界的薄壳,不承担任何业务编排。

use std::sync::Arc;

use serde::Serialize;
use tauri::State;
use tracing::{info_span, Instrument};
use uc_application::facade::FactoryResetError;
use uc_platform::ports::observability::TraceMetadata;

use crate::bootstrap::TauriAppRuntime;
use crate::commands::record_trace_fields;

/// 重置成功后返回的占位元数据。当前无字段——保留为对象以便未来扩展
/// (比如下一步推荐操作的提示),而不必改 wire 形状。
#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FactoryResetResult {}

/// 前端可 `error.code` switch 的 typed 错误。序列化形态:
/// `{"code": "KEY_MATERIAL_WIPE_FAILED", "message": "..."}` 等。
#[derive(Debug, Clone, Serialize, specta::Type, thiserror::Error)]
#[serde(
    tag = "code",
    rename_all = "SCREAMING_SNAKE_CASE",
    rename_all_fields = "camelCase"
)]
pub enum FactoryResetCommandError {
    /// space_setup facade 尚未装配——bootstrap 还没跑完或装配失败。
    /// 前端应延后重试或提示用户重启应用。
    #[error("space setup facade not available in this runtime")]
    FacadeUnavailable,

    /// keyslot / KEK 删除失败。前端应展示通用错误并保留 UnlockPage 状态
    /// (用户至少还能继续尝试输口令);不应跳回 SetupPage——因为残留的
    /// keyslot 会让随后的 init 立即撞 AlreadyInitialized。
    #[error("failed to wipe key material: {message}")]
    KeyMaterialWipeFailed { message: String },

    /// key material 已清但 setup_status 没清。UI 现在处于过渡态:keyslot
    /// 已无、setup_status 仍说 completed。最稳的引导是让用户重启应用。
    #[error("failed to clear setup status after key wipe: {message}")]
    StorageFailed { message: String },

    /// 兜底:其他非预期失败。
    #[error("factory reset failed: {message}")]
    Internal { message: String },
}

impl From<FactoryResetError> for FactoryResetCommandError {
    fn from(err: FactoryResetError) -> Self {
        match err {
            FactoryResetError::KeyMaterialWipeFailed(message) => {
                Self::KeyMaterialWipeFailed { message }
            }
            FactoryResetError::StorageFailed(message) => Self::StorageFailed { message },
            FactoryResetError::Internal(message) => Self::Internal { message },
        }
    }
}

/// 用户主动触发的"重置并重新开始"。
///
/// 调用链:Tauri command → `runtime.app_facade().space_setup` →
/// `SpaceSetupFacade::factory_reset()` →
/// `SpaceAccessPort::factory_reset` + `SetupStatusPort::set_status` +
/// `invitation_holder.cancel_all()`。全程同进程。
///
/// 不接收任何参数:`SpaceAccessAdapter` 用 `current_profile` 推 keyslot
/// 范围,facade 内部 mint 一个新的 `SpaceId` 作为 opaque handle。前端
/// 应在调用本 command **前** 通过二次确认对话框收集用户的明确意图。
#[tauri::command]
#[specta::specta]
pub async fn factory_reset_space(
    runtime: State<'_, Arc<TauriAppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<FactoryResetResult, FactoryResetCommandError> {
    let span = info_span!(
        "command.space_setup.factory_reset_space",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let facade = runtime
            .app_facade()
            .space_setup
            .get()
            .cloned()
            .ok_or(FactoryResetCommandError::FacadeUnavailable)?;
        facade.factory_reset().await?;
        Ok(FactoryResetResult {})
    }
    .instrument(span)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_maps_key_material_wipe_failed() {
        let err: FactoryResetCommandError =
            FactoryResetError::KeyMaterialWipeFailed("disk i/o".to_string()).into();
        assert!(matches!(
            err,
            FactoryResetCommandError::KeyMaterialWipeFailed { ref message } if message == "disk i/o"
        ));
    }

    #[test]
    fn error_maps_storage_failed() {
        let err: FactoryResetCommandError =
            FactoryResetError::StorageFailed("settings db locked".to_string()).into();
        assert!(matches!(
            err,
            FactoryResetCommandError::StorageFailed { ref message } if message == "settings db locked"
        ));
    }

    #[test]
    fn error_maps_internal() {
        let err: FactoryResetCommandError = FactoryResetError::Internal("oops".to_string()).into();
        assert!(matches!(
            err,
            FactoryResetCommandError::Internal { ref message } if message == "oops"
        ));
    }

    #[test]
    fn error_serializes_with_screaming_snake_case_code_tag() {
        let err = FactoryResetCommandError::KeyMaterialWipeFailed {
            message: "boom".to_string(),
        };
        let json = serde_json::to_value(&err).expect("serialize");
        assert_eq!(json["code"], "KEY_MATERIAL_WIPE_FAILED");
        assert_eq!(json["message"], "boom");
    }

    #[test]
    fn facade_unavailable_serializes_without_message_field() {
        let err = FactoryResetCommandError::FacadeUnavailable;
        let json = serde_json::to_value(&err).expect("serialize");
        assert_eq!(json["code"], "FACADE_UNAVAILABLE");
        assert!(json.get("message").is_none());
    }
}
