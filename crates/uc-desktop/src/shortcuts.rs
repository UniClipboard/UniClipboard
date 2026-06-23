//! 桌面全局快捷键管理（GUI-framework agnostic）。
//!
//! 本模块承载所有 **不依赖任何 GUI 框架** 的快捷键管理逻辑：
//!
//! - 快捷键字符串归一化（前端 `meta+ctrl+v` → 物理键 `super+ctrl+v`）
//! - 从 [`uc_core::settings::model::Settings`] 中解析出快捷面板要注册的快捷键集合
//! - 当前已注册快捷键集合的状态容器 [`CurrentShortcuts`]
//! - [`GlobalShortcutRegistry`] trait —— 由具体 GUI shell（例如 `uc-tauri`
//!   包装 `tauri-plugin-global-shortcut`）实现
//! - [`update_shortcuts`] 协调函数 —— 注销旧快捷键、注册新快捷键、失败回滚
//!
//! 真正调用 OS 注册 API 的"最后一公里"由 shell 实现，本模块只负责协调与
//! 状态管理。

use std::sync::Mutex;

use thiserror::Error;
use tracing::{error, warn};
use uc_core::settings::model::{Settings, ShortcutKey};

/// 快捷面板默认的全局快捷键（物理键格式，与 `tauri-plugin-global-shortcut`
/// 接受的字符串格式一致）。
///
/// - macOS: `Cmd+Ctrl+V`
/// - Windows / Linux: `Ctrl+Alt+V`
#[cfg(target_os = "macos")]
pub const DEFAULT_QUICK_PANEL_SHORTCUT: &str = "super+ctrl+v";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_QUICK_PANEL_SHORTCUT: &str = "ctrl+alt+v";

/// `Settings.keyboard_shortcuts` 中存放"切换快捷面板"快捷键覆盖的键名。
///
/// 该键名是跨 shell 共享的约定，前端 `SettingContext` 与所有桌面 shell
/// 都通过它读取/写入用户自定义的快捷键。
pub const QUICK_PANEL_SHORTCUT_SETTINGS_KEY: &str = "global.toggleQuickPanel";

/// 单个 binding 最多包含的 chord 段数（leader + second）。
///
/// 与前端 `MAX_CHORD_SEGMENTS` 对齐。chord 运行时只支持两步契约，更长的
/// 空格分隔输入在归一化/拆段时被截断到前两段，避免向 OS 注册器传入
/// 永远不会触发的三段及以上 binding。
pub const MAX_CHORD_SEGMENTS: usize = 2;

/// 全局快捷键注册过程中可能发生的错误。
///
/// 注：本错误是面向**协调层**的契约，shell 实现把底层（OS / 插件）错误
/// 映射成 [`ShortcutError::Backend`] 字符串向上抛。
#[derive(Debug, Error)]
pub enum ShortcutError {
    /// shell 实现（OS API / Tauri 插件等）返回的错误，原文携带。
    #[error("{0}")]
    Backend(String),
}

impl ShortcutError {
    pub fn backend(msg: impl Into<String>) -> Self {
        Self::Backend(msg.into())
    }
}

/// 当前进程已成功注册到 OS 的快捷键列表。
///
/// `update_shortcuts` 完成一轮成功的"卸旧装新"后，调用方负责把新列表
/// 通过 [`replace`](Self::replace) 写回；任意时刻这里保存的都应该是
/// **OS 视角的真相**，可被回滚逻辑用来计算 diff。
pub struct CurrentShortcuts {
    shortcuts: Mutex<Vec<String>>,
}

impl CurrentShortcuts {
    pub fn new(shortcuts: Vec<String>) -> Self {
        Self {
            shortcuts: Mutex::new(shortcuts),
        }
    }

    /// 返回当前已注册的快捷键克隆副本。
    pub fn current(&self) -> Vec<String> {
        match self.shortcuts.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => {
                error!("CurrentShortcuts lock poisoned; recovering");
                poisoned.into_inner().clone()
            }
        }
    }

    /// 覆盖当前已注册快捷键集合。
    pub fn replace(&self, shortcuts: Vec<String>) {
        match self.shortcuts.lock() {
            Ok(mut guard) => *guard = shortcuts,
            Err(poisoned) => {
                error!("CurrentShortcuts lock poisoned while replacing; recovering");
                *poisoned.into_inner() = shortcuts;
            }
        }
    }
}

/// shell 实现的全局快捷键注册器。
///
/// 实现方负责把 `shortcut`（物理键格式字符串）注册到 OS 层；按下时如何
/// 派发回调由实现方在构造期决定（典型做法：构造时注入一个 `Fn()` 闭包，
/// 内部转成 `tauri-plugin-global-shortcut` 要求的 callback 形态）。
///
/// 桌面协调层假设方法是 **同步** 的，并且调用方已经处理好「需要在哪个
/// 线程上下文里执行」的问题（例如 Tauri 实现需要在 main thread 上跑，
/// 由调用方用 `run_on_main_thread` 包住整段协调流程）。
pub trait GlobalShortcutRegistry: Send + Sync {
    /// 把 `shortcut` 注册到 OS 全局快捷键系统。
    ///
    /// 实现方应在内部做一次**防御性反注册**，避免上次进程残留的 OS
    /// 级 hotkey 造成 "already registered" 失败。
    fn register(&self, shortcut: &str) -> Result<(), ShortcutError>;

    /// 反注册 `shortcut`。如果该快捷键未注册，应视为成功而不是报错。
    fn unregister(&self, shortcut: &str) -> Result<(), ShortcutError>;
}

/// 把前端快捷键字符串归一化为物理键格式。
///
/// 输入示例：`"meta+ctrl+v"`、`"mod+shift+v"`、`"Cmd+Alt+V"`，以及 VS Code 风格
/// 的两段 chord（空格分隔）`"meta+ctrl+v meta+ctrl+v"`。
///
/// 归一化规则（逐段、每段逐 token）：
///   - `meta` / `super`（物理 Meta/Win/Cmd 键）→ `super`
///   - `mod` / `cmd` / `command`（**抽象**平台修饰键）→ macOS 上 `super`，
///     其他平台 `ctrl`
///   - 其余字段保留小写形式
///
/// chord 的各段用单空格重新连接。输出格式恰好与 `tauri-plugin-global-shortcut`
/// 接受的 accelerator 串一致（每段单独注册）；这不是与 Tauri 的绑定，而是
/// 桌面层选择的"物理键串"约定 —— 未来其他 shell 想消费同一份归一化结果不
/// 需要做二次转换。
///
/// 超过 [`MAX_CHORD_SEGMENTS`] 段的输入按两步 chord 契约截断到前两段。
pub fn normalize_to_physical_keys(key: &str) -> String {
    key.split(' ')
        .map(str::trim)
        .filter(|seg| !seg.is_empty())
        .take(MAX_CHORD_SEGMENTS)
        .map(normalize_single_combo)
        .collect::<Vec<_>>()
        .join(" ")
}

/// 把单个组合键（不含 chord 空格）归一化为物理键格式。
fn normalize_single_combo(combo: &str) -> String {
    combo
        .split('+')
        .map(|part| match part.trim().to_lowercase().as_str() {
            "meta" | "super" => "super".to_string(),
            "mod" | "cmd" | "command" => if cfg!(target_os = "macos") {
                "super"
            } else {
                "ctrl"
            }
            .to_string(),
            other => other.to_string(),
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// Split a (physical-key) binding into its chord segments. A single combo
/// yields one segment; a two-step chord (space-separated) yields two. Inputs
/// longer than [`MAX_CHORD_SEGMENTS`] are clamped to the first two segments to
/// match the two-step chord runtime contract.
pub fn chord_segments(binding: &str) -> Vec<String> {
    binding
        .split(' ')
        .map(str::trim)
        .filter(|seg| !seg.is_empty())
        .take(MAX_CHORD_SEGMENTS)
        .map(str::to_string)
        .collect()
}

/// 把一组前端快捷键字符串归一化、去空、转为可注册的物理键格式列表。
///
/// 当 `values` 为 `None` 或归一化后为空时，返回 `[DEFAULT_QUICK_PANEL_SHORTCUT]`
/// —— 调用方拿到的列表**永远非空**，可以直接喂给注册器。
pub fn resolve_shortcut_values<'a, I>(values: Option<I>) -> Vec<String>
where
    I: IntoIterator<Item = &'a str>,
{
    let Some(values) = values else {
        return vec![DEFAULT_QUICK_PANEL_SHORTCUT.to_string()];
    };

    let shortcuts: Vec<String> = values
        .into_iter()
        .map(normalize_to_physical_keys)
        .filter(|s| !s.is_empty())
        .collect();

    if shortcuts.is_empty() {
        vec![DEFAULT_QUICK_PANEL_SHORTCUT.to_string()]
    } else {
        shortcuts
    }
}

/// 从领域 `Settings` 中解析出"切换快捷面板"快捷键的物理键格式列表。
///
/// 未配置 / 配置为空时回落到默认快捷键。
pub fn resolve_quick_panel_shortcuts(settings: &Settings) -> Vec<String> {
    match settings
        .keyboard_shortcuts
        .get(QUICK_PANEL_SHORTCUT_SETTINGS_KEY)
    {
        Some(ShortcutKey::Single(s)) => resolve_shortcut_values(Some(vec![s.as_str()])),
        Some(ShortcutKey::Multiple(v)) => {
            resolve_shortcut_values(Some(v.iter().map(String::as_str).collect::<Vec<_>>()))
        }
        None => resolve_shortcut_values(None::<Vec<&str>>),
    }
}

/// 原子地把 `old` 反注册并注册 `new`。
///
/// 失败时**尽力回滚**——把已经成功注册的 new 都反注册掉，再尝试恢复
/// 所有 old；回滚阶段的错误只会被记录到日志（`error!`），不会被作为
/// 主返回值，因为外层已经在处理首要错误。
///
/// 协调层不感知线程模型；如果具体 [`GlobalShortcutRegistry`] 实现要求
/// 在特定线程（如 Tauri main thread）执行，调用方负责把整次
/// `update_shortcuts` 调用调度到那里。
pub fn update_shortcuts(
    registry: &dyn GlobalShortcutRegistry,
    old: &[String],
    new: &[String],
) -> Result<(), ShortcutError> {
    // 1. 反注册所有旧快捷键。
    for shortcut in old {
        if let Err(e) = registry.unregister(shortcut) {
            warn!(error = %e, shortcut = %shortcut, "Failed to unregister old global shortcut");
        }
    }

    // 2. 防御性反注册新快捷键（可能因上次部分更新或启动残留已注册）。
    for shortcut in new {
        if !old.contains(shortcut) {
            let _ = registry.unregister(shortcut);
        }
    }

    // 3. 注册新快捷键；任意一个失败立刻回滚。
    for (idx, shortcut) in new.iter().enumerate() {
        if let Err(e) = registry.register(shortcut) {
            warn!(error = %e, shortcut = %shortcut, "New shortcut registration failed, rolling back");

            // 卸掉已经注册成功的部分 new。
            for already in &new[..idx] {
                let _ = registry.unregister(already);
            }
            // 恢复 old。
            for old_shortcut in old {
                if let Err(rb_err) = registry.register(old_shortcut) {
                    error!(
                        error = %rb_err,
                        shortcut = %old_shortcut,
                        "Failed to rollback old global shortcut"
                    );
                }
            }
            return Err(e);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    // ── normalize_to_physical_keys ───────────────────────────────────

    #[test]
    fn normalize_meta_to_super() {
        assert_eq!(normalize_to_physical_keys("meta+ctrl+v"), "super+ctrl+v");
        assert_eq!(normalize_to_physical_keys("Meta+Shift+V"), "super+shift+v");
    }

    #[test]
    fn normalize_mod_is_platform_specific() {
        let out = normalize_to_physical_keys("mod+v");
        if cfg!(target_os = "macos") {
            assert_eq!(out, "super+v");
        } else {
            assert_eq!(out, "ctrl+v");
        }
    }

    #[test]
    fn normalize_preserves_unknown_parts() {
        assert_eq!(normalize_to_physical_keys("ctrl+alt+f1"), "ctrl+alt+f1");
    }

    #[test]
    fn normalize_chord_sequence_normalizes_each_segment() {
        // 两段 chord（空格分隔）逐段归一化，空格重新连接。
        assert_eq!(
            normalize_to_physical_keys("meta+ctrl+v meta+ctrl+v"),
            "super+ctrl+v super+ctrl+v"
        );
        let mixed = normalize_to_physical_keys("mod+k mod+c");
        if cfg!(target_os = "macos") {
            assert_eq!(mixed, "super+k super+c");
        } else {
            assert_eq!(mixed, "ctrl+k ctrl+c");
        }
    }

    #[test]
    fn chord_segments_splits_one_or_two() {
        assert_eq!(chord_segments("super+v"), vec!["super+v".to_string()]);
        assert_eq!(
            chord_segments("super+v super+v"),
            vec!["super+v".to_string(), "super+v".to_string()]
        );
        assert!(chord_segments("").is_empty());
    }

    #[test]
    fn chord_segments_clamps_to_two() {
        // 运行时只支持两步 chord，三段及以上输入截断到前两段。
        assert_eq!(
            chord_segments("a b c"),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn normalize_clamps_multi_segment_to_two() {
        // "a b c" 这类不受支持的多段值被限制到两段。
        assert_eq!(
            normalize_to_physical_keys("meta+a meta+b meta+c"),
            "super+a super+b"
        );
    }

    // ── resolve_shortcut_values ─────────────────────────────────────

    #[test]
    fn resolve_none_returns_default() {
        let out = resolve_shortcut_values(None::<Vec<&str>>);
        assert_eq!(out, vec![DEFAULT_QUICK_PANEL_SHORTCUT.to_string()]);
    }

    #[test]
    fn resolve_empty_returns_default() {
        let out = resolve_shortcut_values(Some(Vec::<&str>::new()));
        assert_eq!(out, vec![DEFAULT_QUICK_PANEL_SHORTCUT.to_string()]);
    }

    #[test]
    fn resolve_normalizes_each_entry() {
        let out = resolve_shortcut_values(Some(vec!["meta+ctrl+v", "ctrl+alt+v"]));
        assert_eq!(
            out,
            vec!["super+ctrl+v".to_string(), "ctrl+alt+v".to_string()]
        );
    }

    // ── resolve_quick_panel_shortcuts ───────────────────────────────

    #[test]
    fn resolve_quick_panel_uses_default_when_unset() {
        let settings = Settings::default();
        let out = resolve_quick_panel_shortcuts(&settings);
        assert_eq!(out, vec![DEFAULT_QUICK_PANEL_SHORTCUT.to_string()]);
    }

    #[test]
    fn resolve_quick_panel_reads_single_override() {
        let mut settings = Settings::default();
        settings.keyboard_shortcuts.insert(
            QUICK_PANEL_SHORTCUT_SETTINGS_KEY.to_string(),
            ShortcutKey::Single("meta+shift+v".to_string()),
        );
        let out = resolve_quick_panel_shortcuts(&settings);
        assert_eq!(out, vec!["super+shift+v".to_string()]);
    }

    #[test]
    fn resolve_quick_panel_reads_multiple_override() {
        let mut settings = Settings::default();
        settings.keyboard_shortcuts.insert(
            QUICK_PANEL_SHORTCUT_SETTINGS_KEY.to_string(),
            ShortcutKey::Multiple(vec!["meta+v".into(), "ctrl+alt+v".into()]),
        );
        let out = resolve_quick_panel_shortcuts(&settings);
        assert_eq!(out, vec!["super+v".to_string(), "ctrl+alt+v".to_string()]);
    }

    // ── update_shortcuts coordinator ────────────────────────────────

    /// 把每次 register/unregister 调用追加进日志，便于在测试里断言顺序。
    struct FakeRegistry {
        log: Mutex<Vec<String>>,
        /// `(shortcut, attempt_idx) → 是否让本次 register 失败`。attempt_idx
        /// 从 0 开始，per-shortcut 自增；用于"先成功后失败"或"重试时成功"场景。
        register_fail: HashMap<String, Vec<bool>>,
        register_attempts: Mutex<HashMap<String, usize>>,
    }

    impl FakeRegistry {
        fn new() -> Self {
            Self {
                log: Mutex::new(Vec::new()),
                register_fail: HashMap::new(),
                register_attempts: Mutex::new(HashMap::new()),
            }
        }

        fn fail_register(mut self, shortcut: &str, pattern: Vec<bool>) -> Self {
            self.register_fail.insert(shortcut.to_string(), pattern);
            self
        }

        fn log(&self) -> Vec<String> {
            self.log.lock().unwrap().clone()
        }
    }

    impl GlobalShortcutRegistry for FakeRegistry {
        fn register(&self, shortcut: &str) -> Result<(), ShortcutError> {
            let mut attempts = self.register_attempts.lock().unwrap();
            let attempt = attempts.entry(shortcut.to_string()).or_insert(0);
            let should_fail = self
                .register_fail
                .get(shortcut)
                .and_then(|pattern| pattern.get(*attempt))
                .copied()
                .unwrap_or(false);
            *attempt += 1;
            drop(attempts);

            self.log
                .lock()
                .unwrap()
                .push(format!("register:{shortcut}"));
            if should_fail {
                Err(ShortcutError::backend(format!("fake fail: {shortcut}")))
            } else {
                Ok(())
            }
        }

        fn unregister(&self, shortcut: &str) -> Result<(), ShortcutError> {
            self.log
                .lock()
                .unwrap()
                .push(format!("unregister:{shortcut}"));
            Ok(())
        }
    }

    #[test]
    fn update_shortcuts_happy_path_unregisters_old_then_registers_new() {
        let registry = FakeRegistry::new();
        let old = vec!["super+ctrl+v".to_string()];
        let new = vec!["super+shift+v".to_string()];

        update_shortcuts(&registry, &old, &new).expect("happy path succeeds");

        let log = registry.log();
        // 期望顺序：unregister old → 防御性 unregister new → register new
        assert_eq!(log[0], "unregister:super+ctrl+v");
        assert_eq!(log[1], "unregister:super+shift+v");
        assert_eq!(log[2], "register:super+shift+v");
    }

    #[test]
    fn update_shortcuts_skips_defensive_unregister_for_unchanged_entries() {
        let registry = FakeRegistry::new();
        let old = vec!["super+v".to_string()];
        let new = vec!["super+v".to_string()];

        update_shortcuts(&registry, &old, &new).unwrap();

        let log = registry.log();
        // old 含 super+v → 防御性那一轮跳过
        let defensive_unregisters = log.iter().filter(|l| l.starts_with("unregister:")).count();
        assert_eq!(defensive_unregisters, 1);
    }

    #[test]
    fn update_shortcuts_rolls_back_on_registration_failure() {
        // 让 super+shift+v 第一次注册（new 阶段）失败、第二次（回滚 old 阶段）
        // 不在失败 pattern 里 → 不影响。
        let registry = FakeRegistry::new().fail_register("super+shift+v", vec![true]);
        let old = vec!["super+ctrl+v".to_string()];
        let new = vec!["super+shift+v".to_string()];

        let err = update_shortcuts(&registry, &old, &new).expect_err("should fail");
        assert!(matches!(err, ShortcutError::Backend(_)));

        let log = registry.log();
        // 必须看到回滚：register:super+ctrl+v
        assert!(
            log.contains(&"register:super+ctrl+v".to_string()),
            "expected rollback to re-register old shortcut, got log: {log:?}"
        );
    }

    // ── CurrentShortcuts ────────────────────────────────────────────

    #[test]
    fn current_shortcuts_replace_overwrites() {
        let state = CurrentShortcuts::new(vec!["a".into()]);
        assert_eq!(state.current(), vec!["a".to_string()]);
        state.replace(vec!["b".into(), "c".into()]);
        assert_eq!(state.current(), vec!["b".to_string(), "c".to_string()]);
    }
}
