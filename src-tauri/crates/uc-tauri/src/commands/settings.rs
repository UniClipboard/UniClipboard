//! Settings Tauri commands
//! 设置相关的 Tauri 命令

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;
use tokio::sync::Mutex as AsyncMutex;
use tracing::{error, info_span, Instrument};
use uc_application::facade::settings::{SettingsPatch, ShortcutKeyView};
use uc_desktop::shortcuts::{self, CurrentShortcuts, QUICK_PANEL_SHORTCUT_SETTINGS_KEY};
use uc_platform::ports::observability::TraceMetadata;

use crate::bootstrap::TauriAppRuntime;
use crate::commands::{record_trace_fields, CommandError};
use crate::quick_panel;

/// 串行化 [`update_keyboard_shortcuts`] 整段 read→OS 注册→facade 持久化→
/// 内存 registry replace 的协调流程。并发调用会让 OS 状态、`CurrentShortcuts`、
/// 和 facade 持久化值相互错位（详见 [`update_keyboard_shortcuts`]），所以整段
/// 必须在锁内独占执行。
#[derive(Default)]
pub struct KeyboardShortcutsUpdateLock(pub AsyncMutex<()>);

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, specta::Type)]
#[serde(untagged)]
pub enum ShortcutKeyDto {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateKeyboardShortcutsResult {
    pub keyboard_shortcuts: HashMap<String, ShortcutKeyDto>,
}

/// 保存键盘快捷键，并同步快捷面板全局快捷键的 OS 注册状态。
#[tauri::command]
#[specta::specta]
pub async fn update_keyboard_shortcuts(
    app: tauri::AppHandle,
    runtime: State<'_, Arc<TauriAppRuntime>>,
    shortcut_registry: State<'_, CurrentShortcuts>,
    update_lock: State<'_, KeyboardShortcutsUpdateLock>,
    shortcuts: HashMap<String, Option<ShortcutKeyDto>>,
    _trace: Option<TraceMetadata>,
) -> Result<UpdateKeyboardShortcutsResult, CommandError> {
    let span = info_span!(
        "command.settings.update_keyboard_shortcuts",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        shortcut_count = shortcuts.len(),
    );
    record_trace_fields(&span, &_trace);

    async {
        // 独占整段协调，避免并发调用让 OS / registry / facade 三者错位。
        let _guard = update_lock.0.lock().await;
        let facade = runtime.app_facade();
        let current = facade.settings.get().await.map_err(CommandError::internal)?;
        let next_keyboard_shortcuts =
            apply_keyboard_shortcut_patch_to_map(current.keyboard_shortcuts, &shortcuts);

        let old_registered_shortcuts = shortcut_registry.current();
        let new_registered_shortcuts =
            quick_panel_shortcuts_from_keyboard_shortcuts(&next_keyboard_shortcuts);

        if old_registered_shortcuts != new_registered_shortcuts {
            update_global_shortcuts_on_main_thread(
                &app,
                old_registered_shortcuts.clone(),
                new_registered_shortcuts.clone(),
            )
            .await?;
        }

        let patch = SettingsPatch {
            keyboard_shortcuts: Some(
                shortcuts
                    .into_iter()
                    .map(|(id, value)| (id, value.map(ShortcutKeyView::from)))
                    .collect(),
            ),
            ..Default::default()
        };

        match facade.settings.update(patch).await {
            Ok(updated) => {
                shortcut_registry.replace(new_registered_shortcuts);
                Ok(UpdateKeyboardShortcutsResult {
                    keyboard_shortcuts: keyboard_shortcuts_to_dto(&updated.keyboard_shortcuts),
                })
            }
            Err(err) => {
                if old_registered_shortcuts != new_registered_shortcuts {
                    if let Err(rollback_err) = update_global_shortcuts_on_main_thread(
                        &app,
                        new_registered_shortcuts,
                        old_registered_shortcuts,
                    )
                    .await
                    {
                        error!(
                            error = %rollback_err,
                            "Failed to rollback quick panel global shortcut after settings save failure"
                        );
                    }
                }
                Err(CommandError::internal(err))
            }
        }
    }
    .instrument(span)
    .await
}

fn apply_keyboard_shortcut_patch_to_map(
    mut current: HashMap<String, ShortcutKeyView>,
    patch: &HashMap<String, Option<ShortcutKeyDto>>,
) -> HashMap<String, ShortcutKeyView> {
    for (id, value) in patch {
        match value {
            Some(shortcut) => {
                current.insert(id.clone(), ShortcutKeyView::from(shortcut.clone()));
            }
            None => {
                current.remove(id);
            }
        }
    }
    current
}

fn quick_panel_shortcuts_from_keyboard_shortcuts(
    shortcuts: &HashMap<String, ShortcutKeyView>,
) -> Vec<String> {
    match shortcuts.get(QUICK_PANEL_SHORTCUT_SETTINGS_KEY) {
        Some(ShortcutKeyView::Single(shortcut)) => {
            shortcuts::resolve_shortcut_values(Some(vec![shortcut.as_str()]))
        }
        Some(ShortcutKeyView::Multiple(shortcuts)) => shortcuts::resolve_shortcut_values(Some(
            shortcuts.iter().map(String::as_str).collect::<Vec<_>>(),
        )),
        None => shortcuts::resolve_shortcut_values(None::<Vec<&str>>),
    }
}

/// 把 `update_shortcuts` 整段协调流程调度到 Tauri main thread 上执行。
///
/// `tauri-plugin-global-shortcut` 的注册 API 必须在 main thread 调用；为
/// 避免在多个 register/unregister 之间反复跳线程，整段协调统一封一次。
async fn update_global_shortcuts_on_main_thread(
    app: &tauri::AppHandle,
    old: Vec<String>,
    new: Vec<String>,
) -> Result<(), CommandError> {
    let handle = app.clone();
    let (tx, rx) = tokio::sync::oneshot::channel();

    app.run_on_main_thread(move || {
        // 在 main thread 闭包内构造 registry：捕获 AppHandle 用作回调上下文，
        // 回调闭包绑定 `quick_panel::toggle`（GUI shell 自身的具体动作）。
        let toggle_handle = handle.clone();
        let registry = quick_panel::TauriGlobalShortcutRegistry::new(handle.clone(), move || {
            quick_panel::toggle(&toggle_handle)
        });
        let result = shortcuts::update_shortcuts(&registry, &old, &new);
        let _ = tx.send(result);
    })
    .map_err(|err| CommandError::internal(format!("failed to dispatch to main thread: {err}")))?;

    rx.await
        .map_err(|_| CommandError::internal("main thread dropped shortcut update result"))?
        .map_err(|e| CommandError::Conflict(e.to_string()))
}

fn keyboard_shortcuts_to_dto(
    shortcuts: &HashMap<String, ShortcutKeyView>,
) -> HashMap<String, ShortcutKeyDto> {
    shortcuts
        .iter()
        .map(|(id, shortcut)| (id.clone(), ShortcutKeyDto::from(shortcut.clone())))
        .collect()
}

impl From<ShortcutKeyDto> for ShortcutKeyView {
    fn from(value: ShortcutKeyDto) -> Self {
        match value {
            ShortcutKeyDto::Single(value) => Self::Single(value),
            ShortcutKeyDto::Multiple(value) => Self::Multiple(value),
        }
    }
}

impl From<ShortcutKeyView> for ShortcutKeyDto {
    fn from(value: ShortcutKeyView) -> Self {
        match value {
            ShortcutKeyView::Single(value) => Self::Single(value),
            ShortcutKeyView::Multiple(value) => Self::Multiple(value),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use uc_application::facade::settings::ShortcutKeyView;

    #[test]
    fn keyboard_shortcuts_patch_null_removes_existing_override() {
        let mut current = HashMap::new();
        current.insert(
            QUICK_PANEL_SHORTCUT_SETTINGS_KEY.to_string(),
            ShortcutKeyView::Single("meta+ctrl+v".to_string()),
        );

        let patch = HashMap::from([(QUICK_PANEL_SHORTCUT_SETTINGS_KEY.to_string(), None)]);
        let next = apply_keyboard_shortcut_patch_to_map(current, &patch);

        assert!(!next.contains_key(QUICK_PANEL_SHORTCUT_SETTINGS_KEY));
    }

    #[test]
    fn keyboard_shortcuts_update_result_uses_camel_case_wire_key() {
        let result = UpdateKeyboardShortcutsResult {
            keyboard_shortcuts: HashMap::from([(
                QUICK_PANEL_SHORTCUT_SETTINGS_KEY.to_string(),
                ShortcutKeyDto::Single("meta+shift+v".to_string()),
            )]),
        };

        let json = serde_json::to_value(result).expect("result serializes");

        assert!(json.get("keyboardShortcuts").is_some());
        assert!(json.get("keyboard_shortcuts").is_none());
    }
}
