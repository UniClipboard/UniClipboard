//! [`uc_desktop::shortcuts::GlobalShortcutRegistry`] 的 Tauri 适配实现。
//!
//! 把"注册物理快捷键到 OS"的契约落到 `tauri-plugin-global-shortcut` 上，并在
//! 适配层实现 VS Code 风格的两段 chord：
//!
//! - 一个 binding 是一个物理键序列（[`uc_desktop::shortcuts::chord_segments`]），
//!   1 或 2 段。单段直接注册，按下即触发回调。
//! - 两段 chord `[leader, second]`：leader 与 second 都常驻注册到 OS（插件只能
//!   注册单个 accelerator，没有原生序列），按下 leader 进入 pending，在
//!   [`CHORD_WINDOW`] 内按下 second 才触发。两段相同（连按两次同一组合，即用户
//!   说的"双击"）作为时间窗特例：只注册一个键，leader 回调里自己判连按。
//! - 同步实现，假设调用方已在 Tauri main thread 上下文。

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tracing::{error, info, warn};

use uc_desktop::shortcuts::{chord_segments, GlobalShortcutRegistry, ShortcutError};

/// chord 第二段必须在 leader 之后这个时间窗内按下。与前端 `CHORD_WINDOW_MS` 对齐。
const CHORD_WINDOW: Duration = Duration::from_millis(1000);

/// 正在等待第二段的 chord 状态。同一时刻只可能有一个 chord 在进行，所以是
/// 单个 `Option` 而非 per-key map。
struct PendingChord {
    /// 期待的第二段物理键。
    second: String,
    /// leader 按下的时刻，用于超窗判定。
    armed_at: Instant,
}

/// Tauri 全局快捷键注册器。
///
/// 通过 `new` 注入按下回调；chord 进度由一个共享的 `pending` 槽协调。
pub struct TauriGlobalShortcutRegistry {
    app: AppHandle,
    on_pressed: Arc<dyn Fn() + Send + Sync>,
    pending: Arc<Mutex<Option<PendingChord>>>,
}

impl TauriGlobalShortcutRegistry {
    pub fn new(app: AppHandle, on_pressed: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            app,
            on_pressed: Arc::new(on_pressed),
            pending: Arc::new(Mutex::new(None)),
        }
    }

    /// 注册单个物理组合键（无 chord 空格），按下时调 `on_press`。内部先做一次
    /// 防御性反注册，避免上次进程残留导致 "already registered"。
    fn register_combo(
        &self,
        combo: &str,
        on_press: impl Fn() + Send + Sync + 'static,
    ) -> Result<(), ShortcutError> {
        if let Err(e) = self.app.global_shortcut().unregister(combo) {
            warn!(
                error = %e,
                shortcut = %combo,
                "Defensive unregister before registering global shortcut failed"
            );
        }
        self.app
            .global_shortcut()
            .on_shortcut(combo, move |_app, _shortcut, event| {
                if event.state == ShortcutState::Pressed {
                    on_press();
                }
            })
            .map_err(|e| {
                error!(error = %e, shortcut = %combo, "Failed to register global shortcut");
                ShortcutError::backend(format!("Failed to register shortcut '{combo}': {e}"))
            })
    }
}

impl GlobalShortcutRegistry for TauriGlobalShortcutRegistry {
    fn register(&self, shortcut: &str) -> Result<(), ShortcutError> {
        let segments = chord_segments(shortcut);
        match segments.as_slice() {
            [single] => {
                let on_pressed = Arc::clone(&self.on_pressed);
                let label = single.clone();
                self.register_combo(single, move || {
                    info!(shortcut = %label, "Global shortcut triggered");
                    on_pressed();
                })?;
            }
            [leader, second] => {
                let on_pressed = Arc::clone(&self.on_pressed);
                let pending = Arc::clone(&self.pending);
                let leader_combo = leader.clone();
                let second_combo = second.clone();
                self.register_combo(leader, move || {
                    handle_leader(&pending, &on_pressed, &leader_combo, &second_combo);
                })?;
                // Distinct second segment needs its own listener; a same-combo
                // double tap is fully handled inside `handle_leader`.
                if second != leader {
                    let on_pressed = Arc::clone(&self.on_pressed);
                    let pending = Arc::clone(&self.pending);
                    let second_combo = second.clone();
                    self.register_combo(second, move || {
                        handle_second(&pending, &on_pressed, &second_combo);
                    })?;
                }
            }
            _ => {
                warn!(
                    shortcut = %shortcut,
                    "Ignoring global shortcut with unexpected chord segment count"
                );
            }
        }
        info!(shortcut = %shortcut, "Global shortcut registered");
        Ok(())
    }

    fn unregister(&self, shortcut: &str) -> Result<(), ShortcutError> {
        // 契约：未注册视为成功。逐段反注册（chord 可能注册了两个物理键）。
        for combo in chord_segments(shortcut) {
            if let Err(e) = self.app.global_shortcut().unregister(combo.as_str()) {
                warn!(
                    error = %e,
                    shortcut = %combo,
                    "Unregister global shortcut returned error; treating as no-op per trait contract"
                );
            }
        }
        Ok(())
    }
}

fn lock_pending(slot: &Arc<Mutex<Option<PendingChord>>>) -> MutexGuard<'_, Option<PendingChord>> {
    match slot.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// leader 段按下：要么 arm 等待第二段，要么（双击同键）完成触发。
fn handle_leader(
    pending: &Arc<Mutex<Option<PendingChord>>>,
    on_pressed: &Arc<dyn Fn() + Send + Sync>,
    leader: &str,
    second: &str,
) {
    let now = Instant::now();
    let mut guard = lock_pending(pending);

    if second == leader {
        // 双击同键：上一次按下已 arm 且仍在窗口内 → 触发。
        let is_double = matches!(
            &*guard,
            Some(p) if p.second == second && now.duration_since(p.armed_at) <= CHORD_WINDOW
        );
        if is_double {
            *guard = None;
            drop(guard);
            info!(shortcut = %leader, "Global chord (double tap) triggered");
            on_pressed();
            return;
        }
    }

    // 单击 leader（含双击的第一次）：arm，等待第二段。
    *guard = Some(PendingChord {
        second: second.to_string(),
        armed_at: now,
    });
}

/// 第二段（与 leader 不同）按下：若处于该 chord 的窗口内则触发。
fn handle_second(
    pending: &Arc<Mutex<Option<PendingChord>>>,
    on_pressed: &Arc<dyn Fn() + Send + Sync>,
    second: &str,
) {
    let now = Instant::now();
    let mut guard = lock_pending(pending);
    let fires = matches!(
        &*guard,
        Some(p) if p.second == second && now.duration_since(p.armed_at) <= CHORD_WINDOW
    );
    if fires {
        *guard = None;
        drop(guard);
        info!(shortcut = %second, "Global chord (leader+key) triggered");
        on_pressed();
    }
}
