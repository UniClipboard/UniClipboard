//! System tray icon management.
//!
//! This module provides [`TrayState`] which manages the system tray icon,
//! its context menu, and language-dependent menu item labels.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Mutex;

use tauri::menu::{MenuBuilder, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager};
use tracing::{debug, info, warn};

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
    tray: tauri::tray::TrayIcon,
    /// Phase 96 INDIC-04 状态行 —— 不可交互,用于让用户在不打开主窗口的
    /// 前提下确认 LAN-only Mode 是否已开启(差异图标 OR 状态徽章 二选一,
    /// 这里选状态文案 + tooltip 双重披露)。
    status: MenuItem<tauri::Wry>,
    open: MenuItem<tauri::Wry>,
    settings: MenuItem<tauri::Wry>,
    check_update: MenuItem<tauri::Wry>,
    restart: MenuItem<tauri::Wry>,
    quit: MenuItem<tauri::Wry>,
    language: String,
    lan_only_active: bool,
}

impl TrayState {
    /// Initialize the system tray icon with a context menu.
    ///
    /// This method is idempotent: if the tray is already initialized,
    /// it returns `Ok(())` immediately.
    ///
    /// Phase 96 INDIC-04:`lan_only_active` 反映启动时的 LAN-only Mode 状态
    /// (后端 `settings.network.allow_relay_fallback == false ⇔ ON`),用于
    /// 渲染状态菜单行 + tooltip 后缀。本里程碑承担"重启生效"语义,所以
    /// 进程内不再随设置变化更新此状态(即便设置已切换,要等下次重启才生效)。
    pub fn init(
        &self,
        app: &tauri::AppHandle,
        initial_language: &str,
        lan_only_active: bool,
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
        let status_label = lan_only_status_label(language, lan_only_active);
        let tooltip = lan_only_tooltip(language, lan_only_active);

        // Create menu items with well-known IDs.
        // `tray.status` 是不可交互的状态展示行(`enabled = false`),
        // 用户右键 tray 时一眼可见 LAN-only Mode 是否已生效。
        let status = MenuItem::with_id(app, "tray.status", status_label, false, None::<&str>)?;
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
            .item(&status)
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
            .item(&quit)
            .build()?;

        // Build the tray icon
        let mut builder = TrayIconBuilder::with_id("uc-tray")
            .tooltip(&tooltip)
            .show_menu_on_left_click(false)
            .menu(&menu)
            .on_menu_event(|app, event| match event.id().as_ref() {
                "tray.open" => {
                    show_main_window(app);
                }
                "tray.settings" => {
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
                    // Fire-and-forget。`perform_restart` 内部走 graceful
                    // shutdown → `app.restart()`,后者调用 `process::exit`,
                    // 该 future 在 happy path 永不返回。
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        crate::commands::restart::perform_restart(&app).await;
                    });
                }
                "tray.quit" => {
                    app.exit(0);
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

        // Set the tray icon from the app's default window icon
        match app.default_window_icon() {
            Some(icon) => {
                builder = builder.icon(icon.clone());
            }
            None => {
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
        // 兜底,但 release profile = "abort"(src-tauri/Cargo.toml),
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
            lan_only_active,
            "System tray initialized"
        );

        *guard = Some(TrayHandles {
            tray,
            status,
            open,
            settings,
            check_update,
            restart,
            quit,
            language: language.to_string(),
            lan_only_active,
        });

        Ok(())
    }

    /// Returns `true` once the tray icon has been successfully built.
    ///
    /// Used by the main-window close handler to decide whether hiding to
    /// tray is safe — without a tray there would be no way to bring the
    /// window back, so we let the close proceed normally instead.
    pub fn is_initialized(&self) -> bool {
        self.inner
            .lock()
            .map(|guard| guard.is_some())
            .unwrap_or(false)
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
        let status_label = lan_only_status_label(language, handles.lan_only_active);
        let tooltip = lan_only_tooltip(language, handles.lan_only_active);

        handles.open.set_text(labels.open)?;
        handles.settings.set_text(labels.settings)?;
        handles.check_update.set_text(labels.check_update)?;
        handles.restart.set_text(labels.restart)?;
        handles.quit.set_text(labels.quit)?;
        handles.status.set_text(status_label)?;
        // Tray icon tooltip 也要随语言切换刷新。
        let _ = handles.tray.set_tooltip(Some(&tooltip));
        handles.language = language.to_string();

        debug!("Tray language updated to: {}", language);
        Ok(())
    }
}

/// Phase 96 INDIC-04:LAN-only Mode 状态文案(菜单状态行)。
fn lan_only_status_label(language: &str, lan_only_active: bool) -> &'static str {
    match (language, lan_only_active) {
        ("zh-CN", true) => "LAN-only Mode:已开启",
        ("zh-CN", false) => "LAN-only Mode:未开启",
        (_, true) => "LAN-only Mode: ON",
        (_, false) => "LAN-only Mode: OFF",
    }
}

/// Phase 96 INDIC-04:tray icon tooltip。hover 即可看到 LAN-only Mode 状态。
fn lan_only_tooltip(language: &str, lan_only_active: bool) -> String {
    match (language, lan_only_active) {
        ("zh-CN", true) => "UniClipboard — LAN-only Mode 已开启".to_string(),
        ("zh-CN", false) => "UniClipboard".to_string(),
        (_, true) => "UniClipboard — LAN-only Mode is ON".to_string(),
        (_, false) => "UniClipboard".to_string(),
    }
}

/// Show the main window: make Dock icon visible on macOS, then unminimize, show, and focus.
///
/// Phase 5B: 在所有 caller 共享入口处触发"窗口打开顺手补一次更新检查"。
/// helper 自带 30min 阈值 + `auto_check_update` 双重 gate，远低于阈值时
/// 直接 return，对 UI 路径零延迟。详见
/// [`crate::update_scheduler::window_show_check`]。
pub fn show_main_window(app: &tauri::AppHandle) {
    crate::update_scheduler::maybe_trigger_window_show_check(app);

    #[cfg(target_os = "macos")]
    if let Err(error) = app.set_dock_visibility(true) {
        warn!(error = %error, "Failed to show Dock icon before showing main window");
    }

    match app.get_webview_window("main") {
        Some(window) => {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
        None => {
            warn!("Main window not found");
        }
    }
}

/// Normalize a language string to a supported locale.
///
/// If the language starts with "zh" (case-insensitive), returns `"zh-CN"`.
/// Otherwise returns `"en-US"`.
pub(crate) fn normalize_language(language: &str) -> &'static str {
    if language.to_lowercase().starts_with("zh") {
        "zh-CN"
    } else {
        "en-US"
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
                quit: "退出",
            },
            _ => Self {
                open: "Open",
                settings: "Settings",
                check_update: "Check for Updates…",
                restart: "Restart",
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
