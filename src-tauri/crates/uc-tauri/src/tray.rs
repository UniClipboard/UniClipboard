//! System tray icon management.
//!
//! This module provides [`TrayState`] which manages the system tray icon,
//! its context menu, and language-dependent menu item labels.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Mutex;

use tauri::menu::{MenuBuilder, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Listener, Manager};
use tauri_plugin_dialog::DialogExt;
use tracing::{debug, info, warn};
use uc_daemon_client::{DaemonConnectionState, DaemonSettingsClient};
use uc_daemon_contract::api::dto::settings::{SettingsPatchDto, SyncSettingsPatchDto};

use crate::main_window::show_main_window;

mod device_sync;

/// Managed state that holds the tray icon and its menu item handles.
///
/// Stored via `app.manage(TrayState::default())` and accessed from
/// Tauri commands with `State<'_, TrayState>`.
#[derive(Default)]
pub struct TrayState {
    inner: Mutex<Option<TrayHandles>>,
}

/// Internal handles for the tray icon and its menu items.
struct TrayHandles {
    _tray: tauri::tray::TrayIcon,
    sync: MenuItem<tauri::Wry>,
    device_sync: device_sync::DeviceSyncMenu,
    open: MenuItem<tauri::Wry>,
    settings: MenuItem<tauri::Wry>,
    check_update: MenuItem<tauri::Wry>,
    restart: MenuItem<tauri::Wry>,
    lightweight: MenuItem<tauri::Wry>,
    quit: MenuItem<tauri::Wry>,
    language: String,
    sync_enabled: bool,
    sync_busy: bool,
}

impl TrayState {
    /// Initialize the system tray icon with a context menu.
    ///
    /// This method is idempotent: if the tray is already initialized,
    /// it returns `Ok(())` immediately.
    ///
    pub fn init(
        &self,
        app: &tauri::AppHandle,
        initial_language: &str,
        sync_enabled: bool,
    ) -> tauri::Result<()> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|e| tauri::Error::Anyhow(anyhow::anyhow!("TrayState lock poisoned: {}", e)))?;

        // Idempotent: already initialized
        if guard.is_some() {
            return Ok(());
        }

        let language = normalize_language(initial_language);
        let labels = MenuLabels::for_language(language);
        let device_sync = device_sync::DeviceSyncMenu::new(app, language)?;
        // Create menu items with well-known IDs.
        let sync = MenuItem::with_id(
            app,
            "tray.sync",
            sync_action_label(language, sync_enabled),
            true,
            None::<&str>,
        )?;
        let open = MenuItem::with_id(app, "tray.open", labels.open, true, None::<&str>)?;
        let settings =
            MenuItem::with_id(app, "tray.settings", labels.settings, true, None::<&str>)?;
        let check_update = MenuItem::with_id(
            app,
            "tray.check_update",
            labels.check_update,
            true,
            None::<&str>,
        )?;
        let restart = MenuItem::with_id(app, "tray.restart", labels.restart, true, None::<&str>)?;
        // ADR-008 D3 (P4-3): 轻量模式 — exit the GUI, keep `uniclipd` running.
        let lightweight = MenuItem::with_id(
            app,
            "tray.lightweight",
            labels.lightweight,
            true,
            None::<&str>,
        )?;
        let quit = MenuItem::with_id(app, "tray.quit", labels.quit, true, None::<&str>)?;

        // Debug-only: open the updater window straight from the tray (with dev
        // mock data). Lets us trigger the popup while the app is a no-Dock
        // Accessory background process (main window hidden) — the exact state
        // needed to verify the popup surfaces on top. Not created in release.
        #[cfg(debug_assertions)]
        let dev_open_updater = MenuItem::with_id(
            app,
            "tray.dev_open_updater",
            "打开更新窗 (dev)",
            true,
            None::<&str>,
        )?;

        // Build the context menu.
        // 布局意图:
        //   - "检查更新" 紧贴 "设置",语义上属于"应用维护"组
        //   - "重启" 与 "退出" 同组(都是进程级动作),但用分隔符与上方拉开
        //     一些距离,降低"想点退出却点到重启"的误触
        #[cfg_attr(not(debug_assertions), allow(unused_mut))]
        let mut menu_builder = MenuBuilder::new(app)
            .item(&sync)
            .item(&device_sync.submenu)
            .separator()
            .item(&open)
            .item(&settings)
            .item(&check_update);
        #[cfg(debug_assertions)]
        {
            menu_builder = menu_builder.separator().item(&dev_open_updater);
        }
        let menu = menu_builder
            .separator()
            .item(&restart)
            .item(&lightweight)
            .item(&quit)
            .build()?;

        // Build the tray icon
        let mut builder = TrayIconBuilder::with_id("uc-tray")
            .tooltip("UniClipboard")
            .show_menu_on_left_click(false)
            .menu(&menu)
            .on_menu_event(|app, event| match event.id().as_ref() {
                "tray.sync" => {
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        toggle_sync(&app).await;
                    });
                }
                "tray.open" => {
                    show_main_window(app);
                }
                "tray.settings" => {
                    // Record the deep-link BEFORE showing: `show_main_window`
                    // may have to recreate a destroyed window, and the emit
                    // below would race the fresh webview's listener
                    // registration. The frontend drains the pending route on
                    // boot (`take_pending_navigation`) and discards it after
                    // consuming a live `ui://navigate` event, so the two
                    // channels never double-navigate.
                    app.state::<crate::commands::startup::PendingNavigation>()
                        .set("/settings");
                    show_main_window(app);
                    if let Err(e) = app.emit("ui://navigate", "/settings") {
                        warn!("Failed to emit ui://navigate event: {}", e);
                    }
                }
                "tray.check_update" => {
                    // Fire-and-forget:菜单 handler 是 sync,真正的检查必须
                    // 放到 tokio runtime 上。helper 自带 telemetry + 找到
                    // 新版本时弹更新窗口,所以这里不需要再打开主窗口
                    // —— 没有新版本时用户得到"什么也没发生"的体感,等同于
                    // AboutSection 里点击检查更新但结果为 UpToDate 的情况。
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        crate::commands::updater::perform_manual_check_from_tray(&app).await;
                    });
                }
                #[cfg(debug_assertions)]
                "tray.dev_open_updater" => {
                    // Debug aid: surface the updater window (dev mock data) so we
                    // can verify it pops to the top even from the no-Dock
                    // Accessory state. Mirrors the `dev_open_updater_window`
                    // command but reachable from the tray without the main window.
                    if let Err(e) = crate::update_scheduler::open_or_focus_updater_window(app, true)
                    {
                        warn!("Failed to open updater window from tray (dev): {}", e);
                    }
                }
                "tray.restart" => {
                    // Fire-and-forget。托盘"重启"是 daemon + GUI 的完整重启:
                    // `perform_full_restart` 先重启 daemon、再走 graceful
                    // shutdown → `app.restart()`,后者调用 `process::exit`,
                    // 该 future 在 happy path 永不返回。
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        crate::commands::restart::perform_full_restart(&app).await;
                    });
                }
                "tray.lightweight" => {
                    // ADR-008 D3: GUI process exits, the external daemon keeps
                    // running. Discoverability notification fires first.
                    crate::lightweight::enter_lightweight_mode(app);
                }
                "tray.quit" => {
                    // ADR-008 D3 彻底退出: stop the connected daemon too
                    // (regardless of who spawned it). The `Exit` handler reads the
                    // QuitIntent and runs the graceful stop.
                    crate::lightweight::request_full_quit(app);
                }
                id if id.starts_with("tray.device-sync.") => {
                    let result = (|| -> anyhow::Result<()> {
                        let tray = app.state::<TrayState>();
                        let guard = tray.inner.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
                        if let Some(handles) = guard.as_ref() {
                            handles.device_sync.on_menu_event(id)?;
                        }
                        Ok(())
                    })();
                    if let Err(error) = result {
                        warn!(error = %error, "Failed to handle device sync menu action");
                        show_sync_error(app);
                    }
                }
                _ => {}
            })
            .on_tray_icon_event(|tray, event| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    show_main_window(tray.app_handle());
                }
            });

        #[cfg(target_os = "macos")]
        {
            let tray_icon =
                tauri::image::Image::from_bytes(include_bytes!("../../../icons/tray-icon@2x.png"))?;
            builder = builder.icon(tray_icon).icon_as_template(true);
        }
        #[cfg(not(target_os = "macos"))]
        {
            if let Some(icon) = app.default_window_icon() {
                builder = builder.icon(icon.clone());
            } else {
                warn!("No default window icon available for tray icon");
            }
        }

        // Linux 上 tauri 的 tray-icon → libappindicator-rs 在 dlopen 失败时
        // 走 panic 而不是 Err —— 最常见的两种情况:
        //   1. 用户系统缺 `libayatana-appindicator3-1`(deb)或 `libayatana-appindicator`
        //      (rpm/pacman),Arch / CachyOS / 老 Ubuntu 上特别常见。
        //   2. 系统有 libayatana-ido3 但版本太新,要的 glib 符号
        //      (`g_once_init_leave_pointer`) 在用户的 libglib 里不存在 →
        //      undefined symbol → 加载链断在 ido3 上。
        //
        // 这个 panic 沿 FFI/C 调用栈直接撂倒进程。原本想用 `catch_unwind`
        // 兜底,但 release profile = "abort"(root Cargo.toml),
        // Rust 编译器在 abort 模式下根本不生成 unwind 表,catch_unwind 无法
        // 接住任何 panic —— Sentry UNICLIPBOARD-RUST-G/-10 持续刷,0.10.1-alpha.2
        // AppImage 在 Arch 上 `Aborted (core dumped)` 验证了这一点。
        //
        // 正确做法:在调用 TrayIconBuilder::build **之前**预探 4 个候选 .so,
        // 全部失败就跳过 build,让 libappindicator-sys 的 Lazy::new closure
        // 根本不被触发。`is_initialized()` 保持返回 false,所有依赖 tray 的
        // 菜单更新路径已经 noop。
        #[cfg(target_os = "linux")]
        if !appindicator_lib_available() {
            warn!(
                "libayatana-appindicator3 / appindicator3 not loadable on this \
                 system; skipping system tray init to avoid libappindicator-rs \
                 dlopen panic. Install `libayatana-appindicator3-1` (apt) / \
                 `libayatana-appindicator` (pacman/dnf) and restart to enable it."
            );
            return Ok(());
        }

        // `catch_unwind` 仅作为 panic = unwind profile 下的额外兜底(目前 release
        // = abort,这里实际不生效,但 dev/test profile 下仍可挡住非 dlopen 类
        // panic);Linux 主防线是上面的预探。
        let tray = match catch_unwind(AssertUnwindSafe(|| builder.build(app))) {
            Ok(Ok(tray)) => tray,
            Ok(Err(e)) => return Err(e),
            Err(payload) => {
                let msg = panic_payload_to_string(payload);
                warn!(
                    error = %msg,
                    "System tray init panicked during builder.build(); \
                     continuing without tray."
                );
                return Ok(());
            }
        };

        info!(
            language = %language,
            sync_enabled,
            "System tray initialized"
        );

        *guard = Some(TrayHandles {
            _tray: tray,
            sync,
            device_sync,
            open,
            settings,
            check_update,
            restart,
            lightweight,
            quit,
            language: language.to_string(),
            sync_enabled,
            sync_busy: false,
        });

        let handle = app.clone();
        app.listen("settings://changed", move |event| {
            let result = (|| -> anyhow::Result<()> {
                let notification: SettingsChanged = serde_json::from_str(event.payload())?;
                let settings: SyncSnapshot = serde_json::from_str(&notification.setting_json)?;
                handle
                    .state::<TrayState>()
                    .set_sync_enabled(settings.sync.sync_enabled)?;
                Ok(())
            })();
            if let Err(error) = result {
                warn!(error = %error, "Failed to refresh tray sync state");
            }
        });

        Ok(())
    }

    /// Returns `true` once the tray icon has been successfully built.
    ///
    /// Used by the exit-decision paths: closing the main window destroys it
    /// and the app stays resident only when the tray is alive (see
    /// `lightweight::should_stay_resident`) — without a tray there would be
    /// no way to bring the window back, so the close is allowed to quit the
    /// app instead.
    pub fn is_initialized(&self) -> bool {
        self.inner
            .lock()
            .map(|guard| guard.is_some())
            .unwrap_or(false)
    }

    pub(crate) fn refresh_devices(&self) {
        if let Ok(guard) = self.inner.lock() {
            if let Some(handles) = guard.as_ref() {
                handles.device_sync.refresh();
            }
        }
    }

    /// Update the tray menu labels to match the given language.
    ///
    /// If the tray has not been initialized yet, this is a no-op.
    pub fn set_language(&self, language: &str) -> tauri::Result<()> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|e| tauri::Error::Anyhow(anyhow::anyhow!("TrayState lock poisoned: {}", e)))?;

        let handles = match guard.as_mut() {
            Some(h) => h,
            None => {
                debug!("Tray not initialized, skipping language update");
                return Ok(());
            }
        };

        let language = normalize_language(language);
        let labels = MenuLabels::for_language(language);

        handles.open.set_text(labels.open)?;
        handles.device_sync.set_language(language)?;
        handles.settings.set_text(labels.settings)?;
        handles.check_update.set_text(labels.check_update)?;
        handles.restart.set_text(labels.restart)?;
        handles.lightweight.set_text(labels.lightweight)?;
        handles.quit.set_text(labels.quit)?;
        handles
            .sync
            .set_text(sync_action_label(language, handles.sync_enabled))?;
        handles.language = language.to_string();

        debug!("Tray language updated to: {}", language);
        Ok(())
    }

    fn set_sync_enabled(&self, enabled: bool) -> anyhow::Result<()> {
        let mut guard = self.inner.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        if let Some(handles) = guard.as_mut() {
            handles
                .sync
                .set_text(sync_action_label(&handles.language, enabled))?;
            handles.sync_enabled = enabled;
        }
        Ok(())
    }

    fn set_sync_busy(&self, busy: bool) -> anyhow::Result<bool> {
        let mut guard = self.inner.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let Some(handles) = guard.as_mut() else {
            return Ok(false);
        };
        if busy && handles.sync_busy {
            return Ok(false);
        }
        handles.sync.set_enabled(!busy)?;
        handles.sync_busy = busy;
        Ok(true)
    }
}

fn sync_action_label(language: &str, sync_enabled: bool) -> &'static str {
    match (language, sync_enabled) {
        ("zh-CN", true) => "关闭同步",
        ("zh-CN", false) => "开启同步",
        ("zh-TW", true) => "關閉同步",
        ("zh-TW", false) => "開啟同步",
        ("ja-JP", true) => "同期をオフにする",
        ("ja-JP", false) => "同期をオンにする",
        ("ru-RU", true) => "Выключить синхронизацию",
        ("ru-RU", false) => "Включить синхронизацию",
        ("pt-BR", true) => "Desativar sincronização",
        ("pt-BR", false) => "Ativar sincronização",
        (_, true) => "Disable Sync",
        (_, false) => "Enable Sync",
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettingsChanged {
    setting_json: String,
}

#[derive(serde::Deserialize)]
struct SyncSnapshot {
    sync: SyncValue,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncValue {
    sync_enabled: bool,
}

#[tracing::instrument(name = "tray.toggle_sync", skip(app))]
async fn toggle_sync(app: &tauri::AppHandle) {
    let tray = app.state::<TrayState>();
    match tray.set_sync_busy(true) {
        Ok(true) => {}
        Ok(false) => return,
        Err(error) => {
            warn!(error = %error, "Failed to reserve tray sync action");
            return;
        }
    }
    let result = async {
        let client =
            DaemonSettingsClient::new(app.state::<DaemonConnectionState>().inner().clone())?;
        let current = client.get_settings().await?;
        let enabled = !current.sync.sync_enabled;
        let result = client
            .update_settings(SettingsPatchDto {
                sync: Some(SyncSettingsPatchDto {
                    sync_enabled: Some(enabled),
                    auto_sync_enabled: None,
                    sync_frequency: None,
                    content_types: None,
                    sync_on_restore: None,
                }),
                ..Default::default()
            })
            .await?;
        anyhow::ensure!(result.success, "Sync settings update was rejected");
        tray.set_sync_enabled(enabled)?;
        app.emit("settings://sync-changed", ())?;
        info!(sync_enabled = enabled, "Tray sync switch saved");
        Ok::<_, anyhow::Error>(())
    }
    .await;
    if let Err(error) = tray.set_sync_busy(false) {
        warn!(error = %error, "Failed to restore tray sync action");
    }
    if let Err(error) = result {
        warn!(error = %error, error_kind = "sync_toggle_failed", "Failed to toggle sync from tray");
        show_sync_error(app);
    }
}

fn show_sync_error(app: &tauri::AppHandle) {
    let tray = app.state::<TrayState>();
    let language = tray
        .inner
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().map(|handles| handles.language.clone()))
        .unwrap_or_default();
    let message = match language.as_str() {
        "zh-CN" => "无法更改同步状态，请稍后重试。",
        "zh-TW" => "無法變更同步狀態，請稍後重試。",
        "ja-JP" => "同期設定を変更できませんでした。もう一度お試しください。",
        "ru-RU" => "Не удалось изменить синхронизацию. Повторите попытку позже.",
        "pt-BR" => "Não foi possível alterar a sincronização. Tente novamente.",
        _ => "Could not change sync. Please try again.",
    };
    app.dialog()
        .message(message)
        .title("UniClipboard")
        .kind(tauri_plugin_dialog::MessageDialogKind::Error)
        .show(|_| {});
}

/// Normalize a language string to a supported locale.
///
/// Matches case-insensitively on the primary subtag. Traditional Chinese tags
/// (`zh-Hant`, `zh-TW`, `zh-HK`, and `zh-MO`) select `"zh-TW"`; other Chinese
/// tags select `"zh-CN"`. Japanese, Russian, and Portuguese region variants
/// collapse to their respective bundles. Anything without a bundle is `"en-US"`.
///
/// Keep the supported set in sync with `SUPPORTED_LANGUAGES` in `src/i18n/index.ts`,
/// including the frontend's subtag fallbacks in `normalizeLanguage()`.
pub(crate) fn normalize_language(language: &str) -> &'static str {
    // Accept both separators: BCP-47 hands us "pt-BR", POSIX locale envs "pt_BR".
    let mut subtags = language.split(['-', '_']);
    let primary = subtags.next().unwrap_or_default();
    if primary.eq_ignore_ascii_case("zh") {
        return if subtags.any(|subtag| {
            matches!(
                subtag.to_ascii_lowercase().as_str(),
                "hant" | "tw" | "hk" | "mo"
            )
        }) {
            "zh-TW"
        } else {
            "zh-CN"
        };
    }

    match primary.to_ascii_lowercase().as_str() {
        "ja" => "ja-JP",
        "ru" => "ru-RU",
        "pt" => "pt-BR",
        _ => "en-US",
    }
}

/// Localized labels for the tray menu's interactive items.
///
/// Held as `&'static str` because every supported locale's strings are
/// compile-time literals — `MenuItem::set_text` only requires
/// `impl Into<String>`, but keeping these as static slices avoids per-call
/// allocations and makes the table grep-friendly.
struct MenuLabels {
    open: &'static str,
    settings: &'static str,
    check_update: &'static str,
    restart: &'static str,
    lightweight: &'static str,
    quit: &'static str,
}

impl MenuLabels {
    fn for_language(language: &str) -> Self {
        match language {
            "zh-CN" => Self {
                open: "打开",
                settings: "设置",
                check_update: "检查更新…",
                restart: "重启",
                lightweight: "轻量模式（后台同步）",
                quit: "退出",
            },
            "zh-TW" => Self {
                open: "開啟",
                settings: "設定",
                check_update: "檢查更新…",
                restart: "重新啟動",
                lightweight: "輕量模式（背景同步）",
                quit: "結束",
            },
            "ja-JP" => Self {
                open: "開く",
                settings: "設定",
                check_update: "アップデートを確認…",
                restart: "再起動",
                lightweight: "軽量モード（バックグラウンド同期）",
                quit: "終了",
            },
            "ru-RU" => Self {
                open: "Открыть",
                settings: "Настройки",
                check_update: "Проверить обновления…",
                restart: "Перезапустить",
                lightweight: "Лёгкий режим (фоновая синхронизация)",
                quit: "Выйти",
            },
            "pt-BR" => Self {
                open: "Abrir",
                settings: "Configurações",
                check_update: "Verificar atualizações…",
                restart: "Reiniciar",
                lightweight: "Modo Leve (sincronização em segundo plano)",
                quit: "Sair",
            },
            _ => Self {
                open: "Open",
                settings: "Settings",
                check_update: "Check for Updates…",
                restart: "Restart",
                lightweight: "Lightweight Mode (Background Sync)",
                quit: "Quit",
            },
        }
    }
}

/// 探测 libappindicator-sys 加载链上的 4 个候选 .so 是否能 dlopen 成功。
///
/// 与上游 `libappindicator-sys-0.9.0/src/lib.rs` 的 `Lazy<LIB>` 探测顺序完全
/// 一致(`.so.1` 后缀两条 + backcompat feature 启用时不带后缀两条),只要任
/// 一条能加载就视为可用 —— 这跟上游 closure 在 `Library::new(...).is_ok()`
/// 处直接 return 的逻辑等价。全部失败再返回 false,这时调用方应当跳过
/// `TrayIconBuilder::build` 以避免触发上游 panic。
///
/// 注意:dlopen 成功并不等于 indicator 上 GTK 一定能跑(比如 libayatana-ido3
/// 在用户机器上是装了但 glib 符号缺失),那种情况依旧会在 build 内部 panic。
/// 但 Sentry 现网数据表明 RUST-G/-10 几乎都是"so 文件本身不在"这一类,
/// 优先解决主流场景。glib ABI skew 那条路径如果再次浮上来,届时再叠加
/// `dlsym` 探测关键符号。
#[cfg(target_os = "linux")]
fn appindicator_lib_available() -> bool {
    const CANDIDATES: &[&str] = &[
        "libayatana-appindicator3.so.1",
        "libappindicator3.so.1",
        "libayatana-appindicator3.so",
        "libappindicator3.so",
    ];
    for name in CANDIDATES {
        // SAFETY: `Library::new` 加载共享库是天然 unsafe(初始化 ctors 可能
        // 有副作用),这里仅用于探测可加载性,Library handle 离开作用域时
        // 自动 dlclose。
        if unsafe { libloading::Library::new(*name) }.is_ok() {
            return true;
        }
    }
    false
}

/// Stringify a `catch_unwind` payload — panics carry either `&'static str`
/// or `String`; anything else stays opaque to avoid re-panicking inside the
/// formatter.
fn panic_payload_to_string(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_region_variants_onto_their_bundle() {
        assert_eq!(normalize_language("zh-CN"), "zh-CN");
        assert_eq!(normalize_language("zh-TW"), "zh-TW");
        assert_eq!(normalize_language("zh-Hant"), "zh-TW");
        assert_eq!(normalize_language("zh-HK"), "zh-TW");
        assert_eq!(normalize_language("zh-MO"), "zh-TW");
        assert_eq!(normalize_language("zh-SG"), "zh-CN");
        assert_eq!(normalize_language("ja-JP"), "ja-JP");
        assert_eq!(normalize_language("ru-BY"), "ru-RU");
        assert_eq!(normalize_language("pt-BR"), "pt-BR");
        // European Portuguese has no bundle; Brazilian copy beats falling back to English.
        assert_eq!(normalize_language("pt-PT"), "pt-BR");
    }

    #[test]
    fn falls_back_to_english_without_a_bundle() {
        assert_eq!(normalize_language("fr-FR"), "en-US");
        assert_eq!(normalize_language("en-US"), "en-US");
        assert_eq!(normalize_language(""), "en-US");
    }

    #[test]
    fn ignores_case_and_accepts_posix_separators() {
        assert_eq!(normalize_language("JA_jp"), "ja-JP");
        assert_eq!(normalize_language("RU-ru"), "ru-RU");
        assert_eq!(normalize_language("PT"), "pt-BR");
        assert_eq!(normalize_language("pt_BR"), "pt-BR");
        assert_eq!(normalize_language("zh_CN"), "zh-CN");
        assert_eq!(normalize_language("ZH_tw"), "zh-TW");
    }

    #[test]
    fn matches_the_primary_subtag_not_a_bare_prefix() {
        // A starts_with() check would have claimed these as Portuguese/Chinese.
        assert_eq!(normalize_language("ptx"), "en-US");
        assert_eq!(normalize_language("zhx-Hant"), "en-US");
    }

    #[test]
    fn every_supported_locale_has_tray_labels() {
        // MenuLabels falls through to English, so a locale added to normalize_language
        // without a label arm would silently ship an English tray menu.
        for locale in ["zh-CN", "zh-TW", "ja-JP", "ru-RU", "pt-BR"] {
            assert_ne!(
                MenuLabels::for_language(locale).quit,
                MenuLabels::for_language("en-US").quit,
                "{locale} has no tray labels of its own"
            );
        }
    }
}
