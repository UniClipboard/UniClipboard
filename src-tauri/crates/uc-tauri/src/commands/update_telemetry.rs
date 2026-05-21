//! `capture_update_ui_event` —— 前端纯 UI 更新事件回送到后端 PostHog facade。
//!
//! ## 为什么单一 command
//!
//! 三个 update lifecycle 事件（`update_dialog_opened` / `update_dismissed` /
//! `update_action_invoked`）的触发点只在前端有上下文：
//! - `dialog_opened`：`setUpdateDialogOpen(true)` / `setPackageManagerDialogOpen(true)` 之后
//! - `dismissed`：用户点 "稍后" / 关闭对话框
//! - `action_invoked`（仅 source 透传：实际 `download_bg` / `install` 已由后端 `download_update` /
//!   `install_update` Tauri command 在入口 emit；本 command 仅供前端补 `Cancelled` 等纯 UI 触发场景）
//!
//! 但它们的 `name()` / `properties()` 形态由 `uc_observability::analytics::Event`
//! 单点定义（schema doc §7.8 / §7.9 红线）。让前端走单一白名单 command 把
//! discriminator + 字段送回后端，dispatch 到 `analytics.capture(Event::X)`：
//!
//! 1. 前端不直接 import `uc-observability` 类型，命令边界仍是 typed wire
//! 2. 事件命名 / properties 字段名只在 Rust 一处变（前端通过 specta TS bindings 跟）
//! 3. `install_kind` 由 backend 在接收时反查 install probe 注入，前端零负担
//!
//! ## 为什么 mirror enums
//!
//! `uc_observability` 不依赖 `specta`（observability crate 与 IPC 解耦），
//! 但 wire form 需暴露给前端。沿用 `commands/updater.rs::InstallKind` 的
//! "telemetry 侧 + 命令侧" 双源等价 wire 策略 —— 本文件定义带 `specta::Type`
//! 的 mirror enum，`From<UiX> for AnalyticsX` 显式映射，单测锁死等价。
//!
//! schema doc §8 演化策略：新增变体允许（两侧同步），重命名禁止。

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;
use tracing::{info_span, Instrument};
use uc_observability::analytics::{
    DialogOpenSource, DismissSource, Event, InstallKind, UpdateAction, UpdateActionOutcome,
    UpdatePhase,
};
use uc_platform::ports::observability::TraceMetadata;

use crate::bootstrap::TauriAppRuntime;
use crate::commands::error::CommandError;
use crate::commands::record_trace_fields;
use crate::commands::updater::{detect_install_kind, install_kind_for_telemetry};

// ─── Mirror enums（specta::Type for TS bindings；wire-equivalent to analytics::*）

/// 见 [`analytics::DialogOpenSource`]。wire form: `notification` | `sidebar_icon`。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum UiDialogOpenSource {
    Notification,
    SidebarIcon,
}

/// 见 [`analytics::UpdatePhase`]。wire form: `available` | `downloading` | `ready`。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum UiUpdatePhase {
    Available,
    Downloading,
    Ready,
}

/// 见 [`analytics::DismissSource`]。wire form: `dialog_later` | `dialog_closed` |
/// `package_manager_dialog_closed`。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum UiDismissSource {
    DialogLater,
    DialogClosed,
    PackageManagerDialogClosed,
}

/// 见 [`analytics::UpdateAction`]。wire form: `download_bg` | `install`。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum UiUpdateAction {
    DownloadBg,
    Install,
}

/// 见 [`analytics::UpdateActionOutcome`]。wire form: `started` | `succeeded` |
/// `failed` | `cancelled`。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum UiUpdateActionOutcome {
    Started,
    Succeeded,
    Failed,
    Cancelled,
}

// ─── Tagged event payload ────────────────────────────────────────────────────

/// 前端送来的 UI 触发事件（discriminated union by `kind`）。
///
/// `install_kind` 不出现在任何 variant 里 —— 由后端在 dispatch 时反查注入
/// （schema doc §7.8 落地备注）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UpdateUiEvent {
    /// 用户打开了 `UpdateDialog` / `PackageManagerUpdateDialog`。
    DialogOpened {
        source: UiDialogOpenSource,
        phase: UiUpdatePhase,
    },
    /// 用户放弃了对话框（稍后 / 关闭 / 取消）。
    Dismissed {
        phase: UiUpdatePhase,
        source: UiDismissSource,
    },
    /// 前端纯 UI 路径触发的 action（如 `Cancelled`）。
    ///
    /// `error_kind` 必须是短标识符（< 32 字符，形如 `user_cancelled`）；
    /// **绝不** 含路径 / URL / IP 等可还原用户标识的内容（schema doc §6.1）。
    ActionInvoked {
        action: UiUpdateAction,
        outcome: UiUpdateActionOutcome,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error_kind: Option<String>,
    },
}

// ─── Mappings (mirror → analytics) ───────────────────────────────────────────

impl From<UiDialogOpenSource> for DialogOpenSource {
    fn from(value: UiDialogOpenSource) -> Self {
        match value {
            UiDialogOpenSource::Notification => DialogOpenSource::Notification,
            UiDialogOpenSource::SidebarIcon => DialogOpenSource::SidebarIcon,
        }
    }
}

impl From<UiUpdatePhase> for UpdatePhase {
    fn from(value: UiUpdatePhase) -> Self {
        match value {
            UiUpdatePhase::Available => UpdatePhase::Available,
            UiUpdatePhase::Downloading => UpdatePhase::Downloading,
            UiUpdatePhase::Ready => UpdatePhase::Ready,
        }
    }
}

impl From<UiDismissSource> for DismissSource {
    fn from(value: UiDismissSource) -> Self {
        match value {
            UiDismissSource::DialogLater => DismissSource::DialogLater,
            UiDismissSource::DialogClosed => DismissSource::DialogClosed,
            UiDismissSource::PackageManagerDialogClosed => {
                DismissSource::PackageManagerDialogClosed
            }
        }
    }
}

impl From<UiUpdateAction> for UpdateAction {
    fn from(value: UiUpdateAction) -> Self {
        match value {
            UiUpdateAction::DownloadBg => UpdateAction::DownloadBg,
            UiUpdateAction::Install => UpdateAction::Install,
        }
    }
}

impl From<UiUpdateActionOutcome> for UpdateActionOutcome {
    fn from(value: UiUpdateActionOutcome) -> Self {
        match value {
            UiUpdateActionOutcome::Started => UpdateActionOutcome::Started,
            UiUpdateActionOutcome::Succeeded => UpdateActionOutcome::Succeeded,
            UiUpdateActionOutcome::Failed => UpdateActionOutcome::Failed,
            UiUpdateActionOutcome::Cancelled => UpdateActionOutcome::Cancelled,
        }
    }
}

impl UpdateUiEvent {
    /// True iff dispatching this payload needs an `InstallKind` probe.
    ///
    /// 只有 `DialogOpened` 包含 `install_kind` 字段（schema doc §7.8）；其他
    /// variant 不需要 probe，可省一次 `spawn_blocking`。
    fn requires_install_kind(&self) -> bool {
        matches!(self, UpdateUiEvent::DialogOpened { .. })
    }

    /// Convert into the analytics `Event`. `install_kind` is consumed only by
    /// the `DialogOpened` arm.
    fn into_event(self, install_kind: InstallKind) -> Event {
        match self {
            UpdateUiEvent::DialogOpened { source, phase } => Event::UpdateDialogOpened {
                source: source.into(),
                phase: phase.into(),
                install_kind,
            },
            UpdateUiEvent::Dismissed { phase, source } => Event::UpdateDismissed {
                phase: phase.into(),
                source: source.into(),
            },
            UpdateUiEvent::ActionInvoked {
                action,
                outcome,
                error_kind,
            } => Event::UpdateActionInvoked {
                action: action.into(),
                outcome: outcome.into(),
                error_kind,
            },
        }
    }
}

/// 反查 install kind 用于注入 `UpdateDialogOpened.install_kind`。
///
/// 内部包 `spawn_blocking` —— Linux 上首次调用会跑 dpkg-query / rpm 子进程
/// （之后 `OnceLock` 缓存），不能阻塞 tokio worker。panic / spawn 错误兜底
/// `Unknown`（与 `update_scheduler::detect_install_kind_async` 同策略，便于
/// dashboard 上 `unknown` 占比是稳定的统计指标而非随机噪音）。
async fn probe_install_kind() -> InstallKind {
    match tokio::task::spawn_blocking(detect_install_kind).await {
        Ok(raw) => install_kind_for_telemetry(raw),
        Err(_) => InstallKind::Unknown,
    }
}

// ─── Tauri command ───────────────────────────────────────────────────────────

/// 把前端 UI 触发的 update lifecycle 事件回送到后端 PostHog facade。
///
/// 仅 `DialogOpened` 会触发 `install_kind` probe；其他 variant 走直通映射。
/// `analytics::capture` 自身是 fire-and-forget，所以本 command 不返回 wire 错误
/// （即使 sink 在异步路径上失败，前端也无可处置；同 `commands/updater.rs`
/// 内部 emission 一致）。
#[tauri::command]
#[specta::specta]
pub async fn capture_update_ui_event(
    runtime: State<'_, Arc<TauriAppRuntime>>,
    event: UpdateUiEvent,
    _trace: Option<TraceMetadata>,
) -> Result<(), CommandError> {
    let span = info_span!(
        "command.update_telemetry.capture_update_ui_event",
        kind = event_kind_tag(&event),
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    let analytics = runtime.analytics();

    async move {
        let install_kind = if event.requires_install_kind() {
            probe_install_kind().await
        } else {
            // Sentinel: not embedded in any wire output for non-DialogOpened
            // variants. `into_event` discards this for those arms.
            InstallKind::Unknown
        };
        analytics.capture(event.into_event(install_kind));
        Ok(())
    }
    .instrument(span)
    .await
}

fn event_kind_tag(event: &UpdateUiEvent) -> &'static str {
    match event {
        UpdateUiEvent::DialogOpened { .. } => "dialog_opened",
        UpdateUiEvent::Dismissed { .. } => "dismissed",
        UpdateUiEvent::ActionInvoked { .. } => "action_invoked",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn deserialize_event(value: Value) -> UpdateUiEvent {
        serde_json::from_value(value).expect("UpdateUiEvent deserializes")
    }

    #[test]
    fn dialog_opened_wire_shape_round_trips() {
        let event = deserialize_event(json!({
            "kind": "dialog_opened",
            "source": "notification",
            "phase": "available",
        }));
        assert_eq!(
            event,
            UpdateUiEvent::DialogOpened {
                source: UiDialogOpenSource::Notification,
                phase: UiUpdatePhase::Available,
            }
        );
    }

    #[test]
    fn dismissed_wire_shape_round_trips() {
        let event = deserialize_event(json!({
            "kind": "dismissed",
            "phase": "ready",
            "source": "package_manager_dialog_closed",
        }));
        assert_eq!(
            event,
            UpdateUiEvent::Dismissed {
                phase: UiUpdatePhase::Ready,
                source: UiDismissSource::PackageManagerDialogClosed,
            }
        );
    }

    #[test]
    fn action_invoked_wire_shape_round_trips() {
        let event = deserialize_event(json!({
            "kind": "action_invoked",
            "action": "install",
            "outcome": "failed",
            "error_kind": "signature_mismatch",
        }));
        assert_eq!(
            event,
            UpdateUiEvent::ActionInvoked {
                action: UiUpdateAction::Install,
                outcome: UiUpdateActionOutcome::Failed,
                error_kind: Some("signature_mismatch".into()),
            }
        );
    }

    #[test]
    fn action_invoked_omits_error_kind_when_absent() {
        let event = deserialize_event(json!({
            "kind": "action_invoked",
            "action": "download_bg",
            "outcome": "cancelled",
        }));
        assert_eq!(
            event,
            UpdateUiEvent::ActionInvoked {
                action: UiUpdateAction::DownloadBg,
                outcome: UiUpdateActionOutcome::Cancelled,
                error_kind: None,
            }
        );
    }

    #[test]
    fn unknown_kind_is_rejected() {
        let result: Result<UpdateUiEvent, _> = serde_json::from_value(json!({
            "kind": "totally_made_up",
            "phase": "available",
        }));
        assert!(
            result.is_err(),
            "unknown discriminator must fail to deserialize"
        );
    }

    #[test]
    fn unknown_enum_value_is_rejected() {
        let result: Result<UpdateUiEvent, _> = serde_json::from_value(json!({
            "kind": "dialog_opened",
            "source": "tray_icon",  // not a valid DialogOpenSource
            "phase": "available",
        }));
        assert!(
            result.is_err(),
            "unknown enum value must fail to deserialize"
        );
    }

    #[test]
    fn requires_install_kind_only_for_dialog_opened() {
        assert!(UpdateUiEvent::DialogOpened {
            source: UiDialogOpenSource::Notification,
            phase: UiUpdatePhase::Available,
        }
        .requires_install_kind());

        assert!(!UpdateUiEvent::Dismissed {
            phase: UiUpdatePhase::Available,
            source: UiDismissSource::DialogLater,
        }
        .requires_install_kind());

        assert!(!UpdateUiEvent::ActionInvoked {
            action: UiUpdateAction::Install,
            outcome: UiUpdateActionOutcome::Started,
            error_kind: None,
        }
        .requires_install_kind());
    }

    #[test]
    fn dialog_opened_dispatch_carries_install_kind() {
        let ui = UpdateUiEvent::DialogOpened {
            source: UiDialogOpenSource::SidebarIcon,
            phase: UiUpdatePhase::Downloading,
        };
        let event = ui.into_event(InstallKind::Macos);
        match event {
            Event::UpdateDialogOpened {
                source,
                phase,
                install_kind,
            } => {
                assert_eq!(source, DialogOpenSource::SidebarIcon);
                assert_eq!(phase, UpdatePhase::Downloading);
                assert_eq!(install_kind, InstallKind::Macos);
            }
            other => panic!("expected UpdateDialogOpened, got {other:?}"),
        }
    }

    #[test]
    fn dismissed_dispatch_drops_install_kind() {
        let ui = UpdateUiEvent::Dismissed {
            phase: UiUpdatePhase::Available,
            source: UiDismissSource::DialogClosed,
        };
        // Sentinel value should be discarded by the Dismissed arm.
        let event = ui.into_event(InstallKind::Unknown);
        match event {
            Event::UpdateDismissed { phase, source } => {
                assert_eq!(phase, UpdatePhase::Available);
                assert_eq!(source, DismissSource::DialogClosed);
            }
            other => panic!("expected UpdateDismissed, got {other:?}"),
        }
    }

    #[test]
    fn action_invoked_dispatch_preserves_error_kind() {
        let ui = UpdateUiEvent::ActionInvoked {
            action: UiUpdateAction::DownloadBg,
            outcome: UiUpdateActionOutcome::Failed,
            error_kind: Some("io_error".into()),
        };
        let event = ui.into_event(InstallKind::Unknown);
        match event {
            Event::UpdateActionInvoked {
                action,
                outcome,
                error_kind,
            } => {
                assert_eq!(action, UpdateAction::DownloadBg);
                assert_eq!(outcome, UpdateActionOutcome::Failed);
                assert_eq!(error_kind.as_deref(), Some("io_error"));
            }
            other => panic!("expected UpdateActionInvoked, got {other:?}"),
        }
    }

    /// Schema doc §7.8 / §7.9 锁死的 wire form — mirror enum 与 analytics
    /// enum 的字符串等价必须由 serde 保证。本测试遍历所有变体直接做 JSON
    /// 等价断言，防止未来某一侧悄悄改 rename_all 之后无人发现。
    #[test]
    fn mirror_enums_share_wire_form_with_analytics_enums() {
        fn assert_eq_serde<U: serde::Serialize, A: serde::Serialize>(ui: U, analytics: A) {
            assert_eq!(
                serde_json::to_value(ui).unwrap(),
                serde_json::to_value(analytics).unwrap()
            );
        }

        assert_eq_serde(
            UiDialogOpenSource::Notification,
            DialogOpenSource::Notification,
        );
        assert_eq_serde(
            UiDialogOpenSource::SidebarIcon,
            DialogOpenSource::SidebarIcon,
        );

        assert_eq_serde(UiUpdatePhase::Available, UpdatePhase::Available);
        assert_eq_serde(UiUpdatePhase::Downloading, UpdatePhase::Downloading);
        assert_eq_serde(UiUpdatePhase::Ready, UpdatePhase::Ready);

        assert_eq_serde(UiDismissSource::DialogLater, DismissSource::DialogLater);
        assert_eq_serde(UiDismissSource::DialogClosed, DismissSource::DialogClosed);
        assert_eq_serde(
            UiDismissSource::PackageManagerDialogClosed,
            DismissSource::PackageManagerDialogClosed,
        );

        assert_eq_serde(UiUpdateAction::DownloadBg, UpdateAction::DownloadBg);
        assert_eq_serde(UiUpdateAction::Install, UpdateAction::Install);

        assert_eq_serde(UiUpdateActionOutcome::Started, UpdateActionOutcome::Started);
        assert_eq_serde(
            UiUpdateActionOutcome::Succeeded,
            UpdateActionOutcome::Succeeded,
        );
        assert_eq_serde(UiUpdateActionOutcome::Failed, UpdateActionOutcome::Failed);
        assert_eq_serde(
            UiUpdateActionOutcome::Cancelled,
            UpdateActionOutcome::Cancelled,
        );
    }

    #[test]
    fn event_kind_tag_matches_discriminator() {
        assert_eq!(
            event_kind_tag(&UpdateUiEvent::DialogOpened {
                source: UiDialogOpenSource::Notification,
                phase: UiUpdatePhase::Available,
            }),
            "dialog_opened"
        );
        assert_eq!(
            event_kind_tag(&UpdateUiEvent::Dismissed {
                phase: UiUpdatePhase::Ready,
                source: UiDismissSource::DialogClosed,
            }),
            "dismissed"
        );
        assert_eq!(
            event_kind_tag(&UpdateUiEvent::ActionInvoked {
                action: UiUpdateAction::Install,
                outcome: UiUpdateActionOutcome::Started,
                error_kind: None,
            }),
            "action_invoked"
        );
    }
}
