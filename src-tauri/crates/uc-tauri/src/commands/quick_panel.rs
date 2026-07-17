//! Quick-panel Tauri commands
//! 快捷面板相关的 Tauri 命令

use serde::{Deserialize, Serialize};
use tauri::State;
use tracing::{error, info_span, Instrument};
use uc_core::ports::observability::TraceMetadata;
use uc_core::settings::model::{QuickPanelDoubleTapModifier, QuickPanelPosition};
use uc_daemon_client::{DaemonConnectionState, DaemonSettingsClient};
use uc_daemon_contract::api::dto::settings::{
    QuickPanelDoubleTapModifierDto, QuickPanelPositionDto, QuickPanelSettingsPatchDto,
    SettingsPatchDto,
};
use uc_desktop::modifier_double_tap_monitor::ModifierDoubleTapMonitor;
use uc_desktop::shortcuts::{self, CurrentShortcuts};

use crate::commands::settings::KeyboardShortcutsUpdateLock;
use crate::commands::{record_trace_fields, CommandError};
use crate::modifier_double_tap_platform::{
    modifier_double_tap_availability, ModifierDoubleTapAvailability,
};
use crate::quick_panel;

/// Quick panel placement preference (Tauri command wire form).
///
/// wire form: `center` | `follow_cursor`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum QuickPanelPositionArg {
    Center,
    FollowCursor,
}

impl From<QuickPanelPositionArg> for QuickPanelPosition {
    fn from(value: QuickPanelPositionArg) -> Self {
        match value {
            QuickPanelPositionArg::Center => Self::Center,
            QuickPanelPositionArg::FollowCursor => Self::FollowCursor,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum QuickPanelDoubleTapModifierArg {
    Disabled,
    Alt,
    Control,
    Meta,
}

impl From<QuickPanelDoubleTapModifierArg> for QuickPanelDoubleTapModifier {
    fn from(value: QuickPanelDoubleTapModifierArg) -> Self {
        match value {
            QuickPanelDoubleTapModifierArg::Disabled => Self::Disabled,
            QuickPanelDoubleTapModifierArg::Alt => Self::Alt,
            QuickPanelDoubleTapModifierArg::Control => Self::Control,
            QuickPanelDoubleTapModifierArg::Meta => Self::Meta,
        }
    }
}

/// Which side the inline preview opens toward (Tauri command wire form).
///
/// wire form: `right` | `left`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum QuickPanelExpandSide {
    Right,
    Left,
}

impl From<quick_panel::ExpandSide> for QuickPanelExpandSide {
    fn from(value: quick_panel::ExpandSide) -> Self {
        match value {
            quick_panel::ExpandSide::Right => Self::Right,
            quick_panel::ExpandSide::Left => Self::Left,
        }
    }
}

/// Dismiss the quick panel and return focus to the previous app (no paste).
///
/// 关闭快捷面板并将焦点返回到之前的应用（不粘贴）。
#[tauri::command]
#[specta::specta]
pub async fn dismiss_quick_panel(
    app: tauri::AppHandle,
    _trace: Option<TraceMetadata>,
) -> Result<(), String> {
    let span = info_span!(
        "command.quick_panel.dismiss",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async {
        let handle = app.clone();
        app.run_on_main_thread(move || {
            quick_panel::dismiss(&handle);
        })
        .map_err(|e| format!("Failed to dispatch to main thread: {e}"))?;
        Ok(())
    }
    .instrument(span)
    .await
}

/// Hide the quick panel, re-activate the previous app, and paste.
///
/// 隐藏快捷面板，重新激活之前的应用，并粘贴。
#[tauri::command]
#[specta::specta]
pub async fn paste_to_previous_app(
    app: tauri::AppHandle,
    _trace: Option<TraceMetadata>,
) -> Result<(), String> {
    let span = info_span!(
        "command.quick_panel.paste",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async {
        let handle = app.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        app.run_on_main_thread(move || {
            let result = quick_panel::paste(&handle);
            let _ = tx.send(result);
        })
        .map_err(|e| format!("Failed to dispatch to main thread: {e}"))?;
        rx.await
            .map_err(|_| "Main thread dropped result".to_string())?
    }
    .instrument(span)
    .await
}

/// Finalize the quick panel show after the frontend has cleared stale state.
///
/// 前端清理完旧状态后，实际显示快捷面板窗口。
#[tauri::command]
#[specta::specta]
pub async fn finalize_quick_panel_show(
    app: tauri::AppHandle,
    _trace: Option<TraceMetadata>,
) -> Result<(), String> {
    let span = info_span!(
        "command.quick_panel.finalize_show",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async {
        let handle = app.clone();
        app.run_on_main_thread(move || {
            quick_panel::finalize_show(&handle);
        })
        .map_err(|e| format!("Failed to dispatch to main thread: {e}"))?;
        Ok(())
    }
    .instrument(span)
    .await
}

/// Return the current platform capability for global modifier observation.
#[tauri::command]
#[specta::specta]
pub async fn get_quick_panel_double_tap_availability(
    _trace: Option<TraceMetadata>,
) -> Result<ModifierDoubleTapAvailability, CommandError> {
    let span = info_span!(
        "command.quick_panel.get_double_tap_availability",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async { Ok(modifier_double_tap_availability()) }
        .instrument(span)
        .await
}

/// Persist and apply the standalone modifier double-tap trigger.
#[tauri::command]
#[specta::specta]
pub async fn set_quick_panel_double_tap_modifier(
    connection_state: State<'_, DaemonConnectionState>,
    modifier_monitor: State<'_, ModifierDoubleTapMonitor>,
    update_lock: State<'_, KeyboardShortcutsUpdateLock>,
    modifier: QuickPanelDoubleTapModifierArg,
    _trace: Option<TraceMetadata>,
) -> Result<(), CommandError> {
    let span = info_span!(
        "command.quick_panel.set_double_tap_modifier",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        modifier = ?modifier,
    );
    record_trace_fields(&span, &_trace);

    async {
        let _guard = update_lock.0.lock().await;
        let client = DaemonSettingsClient::new(connection_state.inner().clone());
        let current = client
            .get_settings()
            .await
            .map_err(CommandError::internal)?;
        let target: QuickPanelDoubleTapModifier = modifier.into();
        let target_dto = QuickPanelDoubleTapModifierDto::from(target);

        let persisted_matches = current.quick_panel.double_tap_modifier == target_dto;
        let availability = modifier_double_tap_availability();
        if current.quick_panel.enabled && target != QuickPanelDoubleTapModifier::Disabled {
            match availability {
                ModifierDoubleTapAvailability::Supported => {}
                ModifierDoubleTapAvailability::AccessibilityPermissionRequired => {
                    return Err(CommandError::AccessibilityPermissionRequired(
                        "macOS Accessibility permission is required for modifier double-tap"
                            .to_string(),
                    ));
                }
                ModifierDoubleTapAvailability::UnsupportedDisplaySession => {
                    return Err(CommandError::Conflict(
                        "modifier double-tap is unavailable in this display session".to_string(),
                    ));
                }
            }
        }

        let old_live_modifier = current_modifier_nonblocking(&modifier_monitor).await?;
        let target_live_modifier =
            desired_live_modifier(current.quick_panel.enabled, availability, target);
        set_modifier_nonblocking(&modifier_monitor, target_live_modifier).await?;

        if persisted_matches {
            return Ok(());
        }

        let patch = SettingsPatchDto {
            quick_panel: Some(QuickPanelSettingsPatchDto {
                double_tap_modifier: Some(target_dto),
                ..Default::default()
            }),
            ..Default::default()
        };

        match client.update_settings(patch).await {
            Ok(_) => Ok(()),
            Err(err) => {
                if let Err(rollback_err) =
                    set_modifier_nonblocking(&modifier_monitor, old_live_modifier).await
                {
                    error!(
                        error = %rollback_err,
                        "Failed to rollback quick panel modifier double-tap listener"
                    );
                }
                Err(CommandError::internal(err))
            }
        }
    }
    .instrument(span)
    .await
}

/// 实时启用/禁用快捷面板。
///
/// 与 `update_keyboard_shortcuts` 走同一把 [`KeyboardShortcutsUpdateLock`],
/// 因为两者都会改 OS 全局快捷键的注册状态——并发执行会让 OS 状态、
/// [`CurrentShortcuts`] 内存视图、和 daemon 持久化值互相错位。
///
/// 流程:
///   1. 拿锁,读当前 settings。
///   2. 与 `enabled` 比较:无变化直接返回。
///   3. 计算 desired OS 快捷键列表(开启时 = `resolve_quick_panel_shortcuts`,
///      关闭时 = `[]`)。
///   4. 在 main thread 上一次性完成:开启 → `pre_create` + `register`,
///      关闭 → 只 `unregister`,**不**销毁面板窗口。`tauri-plugin-global-shortcut`
///      与 webview 创建都要求 main thread。
///   5. 经 daemon loopback HTTP 持久化 patch。失败时反向回滚 OS 副作用,避免出现
///      "OS 已生效但磁盘没存"或反过来的撕裂状态。
///   6. 成功后 `shortcut_registry.replace(...)`,让后续 `update_keyboard_shortcuts`
///      能算对 old/new diff。
///
/// **关闭路径不彻底释放 webview**:macOS 上销毁 NSPanel 会与 ObjC 类替换 +
/// on_window_event 异步任务发生 race 而崩溃。所以关闭只反注册 OS 快捷键,
/// 隐藏的 WKWebView / WebContent XPC 进程依旧存在,UI 会提示用户重启 GUI
/// 才能完全释放资源。下次启动期 `quick_panel.enabled = false` 会跳过
/// `pre_create`,自然不会再有这些进程。
#[tauri::command]
#[specta::specta]
pub async fn set_quick_panel_enabled(
    app: tauri::AppHandle,
    connection_state: State<'_, DaemonConnectionState>,
    shortcut_registry: State<'_, CurrentShortcuts>,
    modifier_monitor: State<'_, ModifierDoubleTapMonitor>,
    update_lock: State<'_, KeyboardShortcutsUpdateLock>,
    enabled: bool,
    _trace: Option<TraceMetadata>,
) -> Result<(), CommandError> {
    let span = info_span!(
        "command.quick_panel.set_enabled",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        enabled = enabled,
    );
    record_trace_fields(&span, &_trace);

    async {
        let _guard = update_lock.0.lock().await;
        // ADR-008 P3-3 B2': read-modify-write the settings domain through the
        // daemon over loopback HTTP instead of the in-process facade. The OS
        // global-shortcut register/rollback below stays native.
        let client = DaemonSettingsClient::new(connection_state.inner().clone());
        let current = client
            .get_settings()
            .await
            .map_err(CommandError::internal)?;

        if current.quick_panel.enabled == enabled {
            return Ok(());
        }

        // Reconstruct a domain `Settings`-shaped view of the current keyboard
        // shortcuts so we can reuse `resolve_quick_panel_shortcuts`. We only
        // need the `keyboard_shortcuts` field for that helper.
        let target_shortcuts = if enabled {
            let tmp = uc_core::settings::model::Settings {
                keyboard_shortcuts: current
                    .keyboard_shortcuts
                    .iter()
                    .map(|(id, key)| (id.clone(), key.clone().into()))
                    .collect(),
                ..Default::default()
            };
            shortcuts::resolve_quick_panel_shortcuts(&tmp)
        } else {
            Vec::new()
        };

        let old_shortcuts = shortcut_registry.current();
        let old_modifier = current_modifier_nonblocking(&modifier_monitor).await?;
        let target_modifier = desired_live_modifier(
            enabled,
            modifier_double_tap_availability(),
            current.quick_panel.double_tap_modifier.into(),
        );
        apply_quick_panel_state(
            &app,
            &modifier_monitor,
            enabled,
            &old_shortcuts,
            &target_shortcuts,
            old_modifier,
            target_modifier,
        )
        .await?;

        let patch = SettingsPatchDto {
            quick_panel: Some(QuickPanelSettingsPatchDto {
                enabled: Some(enabled),
                ..Default::default()
            }),
            ..Default::default()
        };

        match client.update_settings(patch).await {
            Ok(_) => {
                shortcut_registry.replace(target_shortcuts);
                Ok(())
            }
            Err(err) => {
                // Persist failed → undo OS side effects so on-disk state and
                // live state agree. Note that rollback reverses both args: if
                // we just enabled, rollback disables; if we just disabled,
                // rollback re-enables (re-registers shortcuts + pre-creates).
                if let Err(rollback_err) = apply_quick_panel_state(
                    &app,
                    &modifier_monitor,
                    !enabled,
                    &target_shortcuts,
                    &old_shortcuts,
                    target_modifier,
                    old_modifier,
                )
                .await
                {
                    error!(
                        error = %rollback_err,
                        "Failed to roll back quick panel side effects after settings save failure"
                    );
                }
                Err(CommandError::internal(err))
            }
        }
    }
    .instrument(span)
    .await
}

pub(crate) fn desired_live_modifier(
    quick_panel_enabled: bool,
    availability: ModifierDoubleTapAvailability,
    persisted: QuickPanelDoubleTapModifier,
) -> QuickPanelDoubleTapModifier {
    if quick_panel_enabled && availability.is_supported() {
        persisted
    } else {
        QuickPanelDoubleTapModifier::Disabled
    }
}

async fn apply_quick_panel_state(
    app: &tauri::AppHandle,
    modifier_monitor: &ModifierDoubleTapMonitor,
    target_enabled: bool,
    old_shortcuts: &[String],
    new_shortcuts: &[String],
    old_modifier: QuickPanelDoubleTapModifier,
    new_modifier: QuickPanelDoubleTapModifier,
) -> Result<(), CommandError> {
    apply_global_shortcuts_on_main_thread(app, target_enabled, old_shortcuts, new_shortcuts)
        .await?;

    if let Err(error) = set_modifier_nonblocking(modifier_monitor, new_modifier).await {
        if let Err(rollback_error) = apply_global_shortcuts_on_main_thread(
            app,
            !target_enabled,
            new_shortcuts,
            old_shortcuts,
        )
        .await
        {
            error!(
                error = %rollback_error,
                "Failed to rollback global shortcuts after modifier listener failure"
            );
        }
        if let Err(rollback_error) = set_modifier_nonblocking(modifier_monitor, old_modifier).await
        {
            error!(
                error = %rollback_error,
                "Failed to rollback modifier listener after quick panel state transition failure"
            );
        }
        return Err(error);
    }

    Ok(())
}

async fn current_modifier_nonblocking(
    modifier_monitor: &ModifierDoubleTapMonitor,
) -> Result<QuickPanelDoubleTapModifier, CommandError> {
    let modifier_monitor = modifier_monitor.clone();
    tokio::task::spawn_blocking(move || modifier_monitor.current())
        .await
        .map_err(CommandError::internal)
}

async fn set_modifier_nonblocking(
    modifier_monitor: &ModifierDoubleTapMonitor,
    modifier: QuickPanelDoubleTapModifier,
) -> Result<(), CommandError> {
    let modifier_monitor = modifier_monitor.clone();
    tokio::task::spawn_blocking(move || modifier_monitor.set_modifier(modifier))
        .await
        .map_err(CommandError::internal)?
        .map_err(CommandError::Conflict)
}

/// Apply the accelerator portion of a quick-panel state transition on the
/// Tauri main thread. The modifier-only listener is coordinated separately.
async fn apply_global_shortcuts_on_main_thread(
    app: &tauri::AppHandle,
    target_enabled: bool,
    old: &[String],
    new: &[String],
) -> Result<(), CommandError> {
    let handle = app.clone();
    let old = old.to_vec();
    let new = new.to_vec();
    let (tx, rx) = tokio::sync::oneshot::channel();

    app.run_on_main_thread(move || {
        let result = (|| {
            if target_enabled {
                // Pre-create before registering: the shortcut callback toggles
                // the panel, so the window should exist before users can press
                // the hotkey. `pre_create` is a no-op if already created.
                quick_panel::pre_create(&handle);
            }

            let toggle_handle = handle.clone();
            let registry =
                quick_panel::TauriGlobalShortcutRegistry::new(handle.clone(), move || {
                    quick_panel::toggle(&toggle_handle)
                });
            shortcuts::update_shortcuts(&registry, &old, &new)
                .map_err(|e| CommandError::Conflict(e.to_string()))?;

            // On disable we deliberately leave the (now-hidden) panel window
            // alive. Destroying it on macOS races with the NSPanel ObjC class
            // swap + on_window_event async tasks and crashes the process; even
            // a hide-then-close shuffle did not free the underlying WKWebView's
            // WebContent XPC. The UI surfaces a "restart to fully release
            // resources" hint instead — see `QuickPanelSection`. The OS-level
            // shortcut is already gone via `update_shortcuts` above, so the
            // dormant webview cannot be reached by the user.
            Ok(())
        })();
        let _ = tx.send(result);
    })
    .map_err(|err| CommandError::internal(format!("failed to dispatch to main thread: {err}")))?;

    rx.await
        .map_err(|_| CommandError::internal("main thread dropped quick panel update result"))?
}

#[cfg(test)]
mod double_tap_state_tests {
    use super::*;

    #[test]
    fn unsupported_session_keeps_modifier_listener_disabled_when_panel_is_enabled() {
        assert_eq!(
            desired_live_modifier(
                true,
                ModifierDoubleTapAvailability::UnsupportedDisplaySession,
                QuickPanelDoubleTapModifier::Meta,
            ),
            QuickPanelDoubleTapModifier::Disabled
        );
    }

    #[test]
    fn supported_session_restores_persisted_modifier() {
        assert_eq!(
            desired_live_modifier(
                true,
                ModifierDoubleTapAvailability::Supported,
                QuickPanelDoubleTapModifier::Alt,
            ),
            QuickPanelDoubleTapModifier::Alt
        );
    }
}

/// Persist the quick panel placement preference and update the live cache.
///
/// Unlike `set_quick_panel_enabled`, this has no OS-registration side effects:
/// the placement only changes where the *next* `show()` puts the window. So
/// the flow is simply "persist via daemon, then refresh the cached mode that
/// the synchronous main-thread `show()` reads". Persist first so a save
/// failure leaves the cache matching disk.
///
/// 持久化快捷面板出现位置偏好，并刷新供 `show()` 读取的缓存。
#[tauri::command]
#[specta::specta]
pub async fn set_quick_panel_position(
    connection_state: State<'_, DaemonConnectionState>,
    position: QuickPanelPositionArg,
    _trace: Option<TraceMetadata>,
) -> Result<(), CommandError> {
    let span = info_span!(
        "command.quick_panel.set_position",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        position = ?position,
    );
    record_trace_fields(&span, &_trace);

    async {
        // ADR-008 P3-3 B2': persist the placement preference through the daemon
        // over loopback HTTP instead of the in-process facade. The in-memory
        // cache refresh below stays native.
        let core_position = QuickPanelPosition::from(position);
        let client = DaemonSettingsClient::new(connection_state.inner().clone());
        let patch = SettingsPatchDto {
            quick_panel: Some(QuickPanelSettingsPatchDto {
                position: Some(QuickPanelPositionDto::from(core_position)),
                ..Default::default()
            }),
            ..Default::default()
        };

        client
            .update_settings(patch)
            .await
            .map_err(CommandError::internal)?;

        // Refresh the cache consumed by the synchronous, main-thread show().
        quick_panel::set_position(core_position);
        Ok(())
    }
    .instrument(span)
    .await
}

/// Update quick panel size and position from the active UI scale and whether
/// the inline preview is expanded (flipping the preview left near the right edge).
#[tauri::command]
#[specta::specta]
pub async fn set_quick_panel_layout(
    app: tauri::AppHandle,
    scale: f64,
    preview_expanded: bool,
    _trace: Option<TraceMetadata>,
) -> Result<(), String> {
    let span = info_span!(
        "command.quick_panel.set_layout",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        scale = scale,
        preview_expanded = preview_expanded,
    );
    record_trace_fields(&span, &_trace);

    async {
        let handle = app.clone();
        app.run_on_main_thread(move || {
            quick_panel::set_layout(&handle, scale, preview_expanded);
        })
        .map_err(|e| format!("Failed to dispatch to main thread: {e}"))?;
        Ok(())
    }
    .instrument(span)
    .await
}

/// Resolve which side the inline preview will open toward, *without* moving the
/// window.
///
/// The frontend calls this before expanding so it can reverse its flex layout
/// (preview-left) ahead of the window reposition — otherwise the history pane
/// would visibly jump when the window shifts left to open the preview leftward.
/// Read-only: it only inspects the remembered anchor and the monitor geometry.
///
/// 在不移动窗口的前提下，解析 preview 将朝哪一侧展开（供前端先翻转布局）。
#[tauri::command]
#[specta::specta]
pub async fn resolve_quick_panel_expand_side(
    app: tauri::AppHandle,
    scale: f64,
    _trace: Option<TraceMetadata>,
) -> Result<QuickPanelExpandSide, String> {
    let span = info_span!(
        "command.quick_panel.resolve_expand_side",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        scale = scale,
    );
    record_trace_fields(&span, &_trace);

    async {
        let handle = app.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        // Monitor/cursor APIs must run on the Tauri main thread.
        app.run_on_main_thread(move || {
            let side = quick_panel::resolve_expand_side(&handle, scale);
            let _ = tx.send(side);
        })
        .map_err(|e| format!("Failed to dispatch to main thread: {e}"))?;
        let side = rx
            .await
            .map_err(|_| "Main thread dropped expand-side result".to_string())?;
        Ok(QuickPanelExpandSide::from(side))
    }
    .instrument(span)
    .await
}
