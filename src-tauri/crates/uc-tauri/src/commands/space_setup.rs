//! Space-setup Tauri commands —— GUI 走 in-process facade 直调
//! `SpaceSetupFacade` / `EncryptionFacade`（与 mobile_sync 同模式，不经
//! webserver）。
//!
//! 两个对外 command:
//!
//! 1. [`unlock_space_with_passphrase`] —— 用户在前端 modal 输入口令后调用。
//!    `passphrase` **不出 Tauri 进程**: 不经 HTTP / TCP socket，直接
//!    `runtime.app_facade().space_setup.unlock_space(input)`。这是 GUI 端
//!    打破 "keyring 与 keyslot 漂移" 死结的唯一安全通路。
//!
//! 2. [`try_silent_unlock`] —— in-process 等价于历史 HTTP
//!    `POST /encryption/unlock`，启动期 auto-unlock 走它。`passphrase`
//!    **不参与**——只从 keyring 读 KEK + 试 unwrap; keyring miss / 漂移
//!    时返回 `resumed: false`，由前端弹 modal 走 [`unlock_space_with_passphrase`]
//!    兜底。
//!
//! `GuiInProcess` 模式下 daemon 与 Tauri 同进程、共享同一份 `AppFacade`,
//! 所以 in-process 调成功后 `InMemorySession` 立即 ready, 同进程内的
//! clipboard watcher / sync 等 deferred services 由前端继续 POST
//! `/lifecycle/ready` 触发(uc-tauri/AGENTS.md "shell 不重新拼业务流程")。

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;
use tracing::{info_span, Instrument};
use uc_application::facade::space_setup::UnlockSpaceError;
use uc_application::facade::{UnlockSpaceInput, UnlockSpaceResult};
use uc_platform::ports::observability::TraceMetadata;

use crate::bootstrap::TauriAppRuntime;
use crate::commands::record_trace_fields;

// ============================================================================
// unlock_space_with_passphrase ─ 用户主动输入口令解锁
// ============================================================================

/// 前端 modal 提交的解锁请求。
///
/// `passphrase` 在 Tauri IPC 边界以明文存在,**绝不**应当再往 HTTP/TCP
/// 上序列化(那等于送到本机 socket 上,违反 §安全)——这一边界仅止于
/// 同进程内 invoke handler。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnlockSpaceArgs {
    pub passphrase: String,
}

/// 解锁成功后返回的最小元数据。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnlockSpaceResultDto {
    pub space_id: String,
}

impl From<UnlockSpaceResult> for UnlockSpaceResultDto {
    fn from(out: UnlockSpaceResult) -> Self {
        Self {
            space_id: out.space_id.to_string(),
        }
    }
}

/// 前端可 `error.code` switch 的 typed 错误。序列化形态:
/// `{"code": "WRONG_PASSPHRASE"}` / `{"code": "INTERNAL", "message": "..."}`。
#[derive(Debug, Clone, Serialize, thiserror::Error)]
#[serde(
    tag = "code",
    rename_all = "SCREAMING_SNAKE_CASE",
    rename_all_fields = "camelCase"
)]
pub enum UnlockSpaceCommandError {
    /// space_setup facade 尚未装配——bootstrap 还没跑完或装配失败。
    /// 前端应延后重试或提示用户重启应用。
    #[error("space setup facade not available in this runtime")]
    FacadeUnavailable,

    /// 没有 setup 过——前端应引导走 init / join 流程,而不是 unlock。
    #[error("setup has not been completed")]
    SetupNotCompleted,

    /// `setup_status.has_completed=true` 但磁盘 keyslot 缺失。
    /// 前端应引导 factory reset。
    #[error("space is not initialized on this device")]
    SpaceNotInitialized,

    /// 口令不能 unwrap 已存 master key。前端应在 modal 中提示
    /// "口令错误,请重试",**不关闭** modal。
    #[error("wrong passphrase")]
    WrongPassphrase,

    /// keyslot 文件存在但版本不支持 / 反序列化失败。再正确的口令也
    /// 派生不出能解开的 KEK。前端应引导 factory reset 或重新 join。
    #[error("space key material is corrupted")]
    CorruptedKeyMaterial,

    /// 兜底:IO / 序列化 / migration resume 等非预期失败。`message`
    /// 仅面向开发者日志,前端展示通用错误对话框 + Sentry 上报即可。
    #[error("unlock failed: {message}")]
    Internal { message: String },
}

impl From<UnlockSpaceError> for UnlockSpaceCommandError {
    fn from(err: UnlockSpaceError) -> Self {
        match err {
            UnlockSpaceError::SetupNotCompleted => Self::SetupNotCompleted,
            UnlockSpaceError::SpaceNotInitialized => Self::SpaceNotInitialized,
            UnlockSpaceError::WrongPassphrase => Self::WrongPassphrase,
            UnlockSpaceError::CorruptedKeyMaterial => Self::CorruptedKeyMaterial,
            UnlockSpaceError::Internal(message) => Self::Internal { message },
        }
    }
}

/// 用户主动输入口令解锁(in-process)。
///
/// 调用链:Tauri command → `runtime.app_facade().space_setup` →
/// `SpaceSetupFacade::unlock_space(input)` →
/// `UnlockSpaceUseCase` → `SpaceAccessPort::unlock(space_id, passphrase)`。
/// 全程同进程,`passphrase` 不进任何 socket。
///
/// 成功后 `InMemorySession::set_master_key` 已写入,session 立即 ready;
/// 前端继续调 `POST /lifecycle/ready` 触发 daemon deferred services
/// (clipboard watcher / sync 等)启动。
#[tauri::command]
pub async fn unlock_space_with_passphrase(
    runtime: State<'_, Arc<TauriAppRuntime>>,
    args: UnlockSpaceArgs,
    _trace: Option<TraceMetadata>,
) -> Result<UnlockSpaceResultDto, UnlockSpaceCommandError> {
    let span = info_span!(
        "command.space_setup.unlock_space_with_passphrase",
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
            .ok_or(UnlockSpaceCommandError::FacadeUnavailable)?;
        let input = UnlockSpaceInput {
            passphrase: args.passphrase,
        };
        let out = facade.unlock_space(input).await?;
        Ok(UnlockSpaceResultDto::from(out))
    }
    .instrument(span)
    .await
}

// ============================================================================
// try_silent_unlock ─ 启动期 / modal 弹出前的 keyring resume 探测
// ============================================================================

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrySilentUnlockResult {
    /// `true` = keyring 命中 + unwrap 成功,session 已 ready;
    /// `false` = "没什么可恢复的"(还没 setup / setup 完但 keyslot 缺失);
    /// 注意 keyring 与 keyslot **漂移**会走 `Err` 而不是 `Ok(false)`,
    /// 前端据此区分"该弹解锁 modal" vs "该走 init/join 引导"。
    pub resumed: bool,
}

#[derive(Debug, Clone, Serialize, thiserror::Error)]
#[serde(
    tag = "code",
    rename_all = "SCREAMING_SNAKE_CASE",
    rename_all_fields = "camelCase"
)]
pub enum TrySilentUnlockError {
    /// app facade 尚未装配。
    #[error("app facade not available in this runtime")]
    FacadeUnavailable,

    /// keyring 中 KEK 与磁盘 keyslot 漂移 / 损坏 / 其他非预期错误。
    /// 前端应弹出 "Unlock space" modal 收集明文口令,走
    /// [`unlock_space_with_passphrase`] 兜底。
    #[error("silent unlock failed: {message}")]
    Internal { message: String },
}

/// In-process 等价于历史 HTTP `POST /encryption/unlock`(silent keyring
/// resume,不接受 passphrase)。**这是替换 `DaemonQueryClient::unlock_encryption()`
/// 的 in-process 路径**——GUI 与 daemon 同进程,完全没必要再绕 HTTP。
///
/// 语义保持原 endpoint 一致: `Ok(true)` keyring 命中、`Ok(false)`
/// "nothing to resume"(空 profile / 还没 setup)、`Err` 异常 / 漂移。
#[tauri::command]
pub async fn try_silent_unlock(
    runtime: State<'_, Arc<TauriAppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<TrySilentUnlockResult, TrySilentUnlockError> {
    let span = info_span!(
        "command.space_setup.try_silent_unlock",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let resumed = runtime
            .app_facade()
            .encryption
            .unlock()
            .await
            .map_err(|err| TrySilentUnlockError::Internal {
                message: err.to_string(),
            })?;
        Ok(TrySilentUnlockResult { resumed })
    }
    .instrument(span)
    .await
}
