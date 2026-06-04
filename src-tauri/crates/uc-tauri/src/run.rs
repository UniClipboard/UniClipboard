//! Tauri shell 主入口。
//!
//! `main.rs` 在外面构造 `ProcessRuntimeContext` 与 `tauri::Context`（后者由
//! `tauri::generate_context!()` 宏生成，必须在 bin crate 里），然后调用
//! [`run`] 把控制权交给 Tauri shell：装配 `TauriAppRuntime`、注册
//! plugins、启动 daemon 拉起/守护、初始化托盘、注册 commands、运行 Tauri
//! 事件循环。
//!
//! 这里是"Tauri shell 的最后一公里"——所有 GUI-framework agnostic 的
//! 桌面宿主能力（runtime 装配、后台任务调度、daemon ownership 协调状态）
//! 都已下沉到 [`uc_desktop`]，本文件只关心怎么把它们落到 Tauri 的
//! `Builder` / `setup` / `RunEvent` 上。

use std::sync::Arc;
use std::time::Duration;

use tauri::webview::PageLoadEvent;
use tauri::Manager;
use tauri_plugin_autostart::MacosLauncher;
use tracing::{error, info, warn};

use uc_bootstrap::build_gui_client_context;
use uc_daemon_client::realtime::RealtimeTopic;
use uc_daemon_client::{DaemonConnectionState, DaemonWsBridge, DaemonWsBridgeConfig};
use uc_desktop::daemon_probe::{
    bootstrap_daemon_in_process, HEALTH_CHECK_TIMEOUT, HEALTH_POLL_INTERVAL,
    INCOMPATIBLE_DAEMON_EXIT_TIMEOUT,
};
use uc_desktop::shortcuts::GlobalShortcutRegistry;
use uc_desktop::DaemonOwnership;

use crate::bootstrap::{ensure_default_device_name, TauriAppRuntime};
use crate::commands::updater::PendingUpdate;
use crate::quick_panel;
use crate::tray::TrayState;

/// 前端事件名——告诉 webview "本 GUI 进程马上重启了，请主动 close 你那条
/// WebSocket"。前端 `daemon-ws-bootstrap.ts` 的 listener 收到后调用
/// `daemonWs.disconnect()` 发送 close frame，让 daemon 端尽快释放这条旧
/// 连接(daemon 是独立进程,重启的是 GUI;新 GUI 起来后会重新连)。
///
/// ADR-008 P3-3 (B2'-3) 起仅 `restart` 路径使用——GUI 正常退出不再需要它
/// (daemon 不随 GUI 关停,见 RunEvent::ExitRequested)。
pub(crate) const FRONTEND_SHUTDOWN_EVENT: &str = "app://shutting-down";

/// 给前端响应 `app://shutting-down` 事件、发出 WebSocket close frame
/// 的时间。100ms 对浏览器 WebSocket close frame 飞过 loopback 来说极宽裕——
/// 用户感知不到这点延迟。
pub(crate) const SHUTDOWN_FRONTEND_GRACE_MS: u64 = 100;

/// 这个 GUI shell 期望 daemon 上报的 `packageVersion`——`probe_daemon_health`
/// 用它做版本兼容性判断。`env!` 拿的是 `uc-tauri` 自己的 cargo 版本，
/// workspace 共享版本号所以与 `uniclipboard` bin 一致。
const EXPECTED_PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// auto-unlock 等待 daemon connection_state 被填充的总上限。
/// `bootstrap_daemon_in_process` 内部 `wait_for_daemon_health` 默认上限 8s
/// （`HEALTH_CHECK_TIMEOUT`）+ legacy daemon 替换路径再加 `INCOMPATIBLE_DAEMON_EXIT_TIMEOUT`，
/// 给 30s 足够覆盖最坏路径。超时只是放弃 auto-unlock，用户改用手动解锁。
const AUTO_UNLOCK_DAEMON_READY_TIMEOUT: Duration = Duration::from_secs(30);
/// 轮询 connection_state 的间隔。
const AUTO_UNLOCK_DAEMON_READY_POLL: Duration = Duration::from_millis(200);

/// 等待 `DaemonConnectionState` 被 daemon bootstrap 填充。
/// 返回 `true` 表示连接信息已就绪；`false` 表示在 `timeout` 内仍未填充。
async fn wait_for_daemon_connection(
    state: &DaemonConnectionState,
    timeout: Duration,
    poll_interval: Duration,
) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if state.get().is_some() {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(poll_interval).await;
    }
}

#[cfg(target_os = "windows")]
fn configure_main_window_for_platform(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        warn!("Main window not found during Windows window configuration");
        return;
    };

    if let Err(error) = window.set_decorations(false) {
        warn!(error = %error, "Failed to disable Windows main window decorations");
    }
}

#[cfg(not(target_os = "windows"))]
fn configure_main_window_for_platform(_app: &tauri::AppHandle) {}

/// Translate a daemon-WS [`RealtimeEvent`] into the application-layer
/// [`HostEvent`] the activity HUD consumes (ADR-008 P3-3 B2'-3).
///
/// Only the three HUD-relevant variants map to a `HostEvent`; everything else on
/// the subscribed topics (e.g. `ClipboardNewContent`) returns `None` and is
/// ignored by the HUD feed. This is the GUI-side inverse of the daemon's
/// `DaemonApiEventEmitter` (which serialises `HostEvent` → WS).
fn realtime_to_host_event(
    event: uc_daemon_client::realtime::RealtimeEvent,
) -> Option<uc_application::facade::HostEvent> {
    use uc_application::facade::{ClipboardHostEvent, HostEvent, TransferHostEvent};
    use uc_daemon_client::realtime::RealtimeEvent as Re;

    match event {
        Re::FileTransferStatusChanged(e) => {
            Some(HostEvent::Transfer(TransferHostEvent::StatusChanged {
                transfer_id: e.transfer_id,
                entry_id: e.entry_id,
                status: e.status,
                reason: e.reason,
            }))
        }
        Re::FileTransferProgress(e) => Some(HostEvent::Transfer(TransferHostEvent::Progress {
            transfer_id: e.transfer_id,
            entry_id: e.entry_id,
            peer_id: e.peer_id,
            direction: e.direction,
            bytes_transferred: e.bytes_transferred,
            total_bytes: e.total_bytes,
        })),
        Re::ClipboardIncomingPending(e) => {
            Some(HostEvent::Clipboard(ClipboardHostEvent::IncomingPending {
                entry_id: e.entry_id,
                from_device: e.from_device,
                total_bytes: e.total_bytes,
                filenames: e.filenames,
            }))
        }
        _ => None,
    }
}

/// Builds the pure-client GUI runtime context, spawns/connects the external daemon, and runs the Tauri event loop.
///
/// The provided `tauri_ctx` must be created in the binary crate using `tauri::generate_context!()` (that macro reads the bin crate's tauri.conf.json). This function assembles the GUI client context via `uc_bootstrap::build_gui_client_context()` (file-backed ports only — no sqlite); if assembly fails it returns an `Err`. The daemon is reached as a separate process (probe → connect, or detached spawn). On success the function enters the Tauri event loop and does not return until the application exits.
///
/// # Parameters
///
/// - `tauri_ctx`: the Tauri application context produced by `tauri::generate_context!()` in the binary crate.
///
/// # Returns
///
/// `Ok(())` if the Tauri application was built and the run loop started (the function will complete only after application exit). `Err` if GUI bootstrap or building the Tauri application fails.
///
/// # Examples
///
/// ```rust,ignore
/// // In src-tauri/src/main.rs
/// let ctx = tauri::generate_context!();
/// crate::run(ctx).expect("failed to start tauri application");
/// ```
pub fn run(tauri_ctx: tauri::Context<tauri::Wry>) -> anyhow::Result<()> {
    // ADR-008 P3-3 (B2'-3): the GUI is a pure client of an external `uniclipd`.
    // It assembles ONLY the file-backed ports it needs (settings / setup-status /
    // analytics / device-id / storage paths) via `build_gui_client_context` —
    // it never opens the sqlite pool, builds the in-process `AppFacade`, or runs
    // blob workers (the daemon owns all of that). All business calls go over
    // daemon HTTP/WS (`uc-daemon-client`); host events arrive over the daemon
    // WS (`DaemonWsBridge`), not an in-process `host_event_bus`.
    let client_deps = build_gui_client_context()?;

    let daemon_connection_state = DaemonConnectionState::default();
    let daemon_ownership = DaemonOwnership::default();

    let runtime = Arc::new(TauriAppRuntime::new(client_deps));

    // Startup barrier used to coordinate backend readiness and main window show timing.
    let startup_barrier = Arc::new(crate::commands::startup::StartupBarrier::default());

    let disable_single_instance = std::env::var("UC_DISABLE_SINGLE_INSTANCE").as_deref() == Ok("1");

    // Store TaskRegistry reference for exit hook registration
    let task_registry = runtime.task_registry().clone();

    let builder = tauri::Builder::default()
        // Register TauriAppRuntime for Tauri commands
        .manage(runtime.clone())
        .manage(DaemonConnectionState::clone(&daemon_connection_state))
        .manage(DaemonOwnership::clone(&daemon_ownership))
        .manage(TrayState::default())
        .manage(crate::lightweight::QuitIntent::default())
        .manage(task_registry.clone())
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    // Only hide-to-tray if the tray actually came up. When tray
                    // init fails (treated as non-fatal during setup), hiding
                    // the window plus the Dock icon would leave the app
                    // running with no UI surface to recover or quit it.
                    if window.state::<TrayState>().is_initialized() {
                        api.prevent_close();
                        let _ = window.hide();
                        #[cfg(target_os = "macos")]
                        if let Err(error) = window.app_handle().set_dock_visibility(false) {
                            warn!(error = %error, "Failed to hide Dock icon after hiding main window");
                        }
                        info!("Main window hidden to tray");
                    } else {
                        info!("Tray unavailable; allowing main window close to proceed");
                    }
                }
            }
        })
        .on_page_load(move |webview, payload| {
            if webview.label() != "main" {
                return;
            }

            let event_label = match payload.event() {
                PageLoadEvent::Started => "started",
                PageLoadEvent::Finished => "finished",
            };

            info!(
                window_label = webview.label(),
                event = event_label,
                url = %payload.url(),
                "[StartupTiming] main webview page load"
            );

            if matches!(payload.event(), PageLoadEvent::Finished) {}
        })
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init());

    let builder = if disable_single_instance {
        info!("UC_DISABLE_SINGLE_INSTANCE=1 set; skipping single-instance plugin registration");
        builder
    } else {
        builder.plugin(tauri_plugin_single_instance::init(|_app, _args, _cwd| {}))
    };

    let task_registry_for_run = task_registry.clone();

    // tauri-specta builder —— 命令清单的单一真相源（见 `specta_builder.rs`）。
    // 这里只用 `invoke_handler` 接进 Tauri runtime；`builder.export(...)`
    // 走 `tests/specta_export.rs` 那条路径，CI 跑同一个 test 检查 schema drift。
    let specta_builder = crate::specta_builder::build();

    builder
        // ADR-008 D10（2026-06-04 修订）：登录自启目标 = GUI 自身（daemon 由 GUI
        // 冷启动经下方 setup 的 `bootstrap_daemon_in_process` 拉起）。沿用
        // tauri-plugin-autostart，不自建 OS 原生 daemon 投影 / StartupIntegrationProvider。
        // 注意：launch args 暂为空 → 自启的 GUI 不带 `UC_PROFILE`，且 autolaunch 用
        // 编译期固定 bundle id，故 per-profile 自启（非主 profile）尚未隔离。D19 默认
        // 仅主 profile 注册自启，对主 profile 无害；非主 profile 的隔离留 P4-7。
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![]),
        ))
        .setup(move |app| {
            // Set AppHandle on runtime so it can emit events to frontend
            // In Tauri 2, use app.handle() to get the AppHandle
            runtime.set_app_handle(app.handle().clone());
            info!("AppHandle set on TauriAppRuntime for event emission");
            configure_main_window_for_platform(app.handle());

            // 文件接收 HUD:渲染 macOS 原生 AppKit panel (AirDrop 风格)。
            // ADR-008 P3-3 (B2'-3): GUI 已无 in-process host_event_bus —— HUD
            // 改由 daemon WS 喂。`install` 返回 emitter,下面用 `DaemonWsBridge`
            // 订阅 file-transfer + clipboard topic,把 `RealtimeEvent` 翻成
            // `HostEvent` 喂给它(emitter 的状态机 / actions / 平台 listener /
            // 后台 sweep 装配细节仍收在 install() 内部)。
            let hud_emitter = crate::activity_hud::install(crate::activity_hud::InstallDeps {
                app_handle: app.handle().clone(),
            });

            // daemon WS 桥:连到外部 daemon 的 WS(loopback),把 transfer /
            // incoming-pending 事件喂给 HUD emitter。bridge 在有订阅者时才连,
            // 连接生命周期挂在进程 task_registry 的 CancellationToken 上。
            let hud_bridge = std::sync::Arc::new(DaemonWsBridge::new(
                daemon_connection_state.clone(),
                DaemonWsBridgeConfig::default(),
            ));
            let hud_bridge_for_run = std::sync::Arc::clone(&hud_bridge);
            let hud_bridge_token = runtime.task_registry().token().clone();
            tauri::async_runtime::spawn(async move {
                let mut rx = match hud_bridge
                    .subscribe(
                        "activity_hud",
                        &[RealtimeTopic::FileTransfer, RealtimeTopic::Clipboard],
                    )
                    .await
                {
                    Ok(rx) => rx,
                    Err(error) => {
                        warn!(error = %error, "activity HUD: failed to subscribe daemon WS bridge");
                        return;
                    }
                };
                // 现在有订阅者了,驱动 bridge 连接循环。
                tauri::async_runtime::spawn(hud_bridge_for_run.run(hud_bridge_token));
                while let Some(event) = rx.recv().await {
                    if let Some(host_event) = realtime_to_host_event(event) {
                        use uc_application::facade::HostEventEmitterPort;
                        let _ = hud_emitter.emit(host_event);
                    }
                }
            });

            let daemon_connection_state_for_setup = daemon_connection_state.clone();
            let daemon_ownership_for_setup = daemon_ownership.clone();
            tauri::async_runtime::spawn(async move {
                match bootstrap_daemon_in_process(
                    &daemon_ownership_for_setup,
                    EXPECTED_PACKAGE_VERSION,
                    INCOMPATIBLE_DAEMON_EXIT_TIMEOUT,
                    HEALTH_CHECK_TIMEOUT,
                    HEALTH_POLL_INTERVAL,
                )
                .await
                {
                    Ok(connection_info) => {
                        daemon_connection_state_for_setup.set(connection_info);
                        // ADR-008 P3-3 (B2'-3): daemon 现在永远是外部独立进程
                        // (probe→connect 或 detached spawn)。GUI 不再 owns 它的
                        // 生命周期 —— 崩溃恢复由外部负责;退出语义见 D3 三态
                        // (P4-3 已落地:仅显式「彻底退出」停 daemon,关窗/轻量保留)。
                        // 自启=GUI (D10 2026-06-04 修订):登录起 GUI → 这里必拉起
                        // daemon,即"自启 GUI 等于后台同步就绪"的闭环。
                    }
                    Err(error) => {
                        // Display 只暴露 thiserror 外层 message，会把 anyhow source chain
                        // 截掉 —— root cause 全丢；用 Debug 把整条 chain 一起打出来。
                        error!(
                            error = %error,
                            error_chain = ?error,
                            "Daemon startup/probe failed during Tauri bootstrap"
                        );
                    }
                }
            });

            // Load startup settings for tray and silent start
            // `quick_panel_enabled`:决定是否在启动期注册全局快捷键 +
            // 预创建快捷面板窗口。默认（用户未显式开启）为 false,
            // 避免对用不到该功能的用户造成全局快捷键占用 / 资源浪费。
            // 运行期的开关切换由 `set_quick_panel_enabled` command 协调，
            // 这里只负责"以最近持久化的偏好启动"。
            let (
                silent_start,
                initial_language,
                lan_only_active,
                quick_panel_enabled,
                auto_start,
                settings_loaded,
            ) = {
                let settings_port = runtime.settings_port();
                match tauri::async_runtime::block_on(settings_port.load()) {
                    Ok(settings) => {
                        let silent = settings.general.silent_start;
                        let lang = settings.general.language.unwrap_or_default();
                        // Phase 96 INDIC-04:反向命名唯一翻译点之一,UI/Tray
                        // = "LAN-only ON" ⇔ 后端 `allow_relay_fallback = false`。
                        // 与 NetworkSection.tsx / SpaceMembersPanel.tsx 同源。
                        let lan_only = !settings.network.allow_relay_fallback;
                        let quick_panel = settings.quick_panel.enabled;
                        let auto = settings.general.auto_start;
                        (silent, lang, lan_only, quick_panel, auto, true)
                    }
                    Err(e) => {
                        warn!("Failed to load settings for startup: {}, using defaults", e);
                        (false, "en-US".to_string(), false, false, false, false)
                    }
                }
            };

            // Reconcile the OS launch-at-login registration with the persisted
            // preference. When enabled this always rewrites the entry to the
            // current executable path, self-healing stale entries left by older
            // installs / dev builds / moved binaries — the root cause of
            // silently-broken autostart. setup runs on the main thread, where
            // the autostart plugin's APIs are safe to call.
            //
            // Gate on `settings_loaded`: a transient settings read failure falls
            // back to `auto_start = false`, and reconciling on that stale default
            // would remove a launch-at-login entry the user had actually enabled.
            // When settings didn't load we leave the existing OS state untouched.
            if settings_loaded {
                let port = crate::adapters::autostart::TauriAutostart::new(app.handle().clone());
                if let Err(error) = crate::adapters::autostart::reconcile_autostart(&port, auto_start)
                {
                    warn!(error = %error, auto_start, "Failed to reconcile OS autostart on startup");
                }
            } else {
                warn!("Skipping OS autostart reconcile: startup settings failed to load");
            }

            // Initialize system tray
            let tray_state = app.state::<TrayState>();
            if let Err(e) = tray_state.init(app.handle(), &initial_language, lan_only_active) {
                error!("Failed to initialize system tray: {}", e);
                // Non-fatal: continue startup without tray
            }

            // 仅在静默启动时隐藏 Dock。非静默启动时 app 以 `Regular` 起步,
            // 紧接着会 `show_main_window`;若此处先翻成 `Accessory` 再翻回
            // `Regular`,macOS(尤其 Sequoia/Tahoe)会把 app 重新塞回 Dock 却
            // 不重读 bundle 图标,留下「运行小圆点 + 空白图标」。静默启动没有
            // 这次紧接着的回翻,照常隐藏即可。
            #[cfg(target_os = "macos")]
            if silent_start {
                if let Err(error) = app.handle().set_dock_visibility(false) {
                    warn!(error = %error, "Failed to hide Dock icon during startup");
                }
            }

            // Register global shortcut plugin (empty — shortcuts registered dynamically).
            // `#[cfg(desktop)]` is normally injected by `tauri-build` in the bin crate;
            // here we spell it out explicitly so it compiles in this lib crate too.
            //
            // 即使 `quick_panel_enabled = false`,plugin 本身仍然注册:它只是
            // 把 `tauri-plugin-global-shortcut` 接进运行时,真正的快捷键注册
            // 由下面的循环按需进行。用户后续通过 `set_quick_panel_enabled`
            // 打开开关时,plugin 已就绪,可直接复用同样的注册流程。
            let mut registered_quick_panel_shortcuts = Vec::new();

            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            {
                app.handle()
                    .plugin(tauri_plugin_global_shortcut::Builder::new().build())?;

                if quick_panel_enabled {
                    // 从设置读取快捷键覆盖；未配置或为空则回落到桌面层默认。
                    let shortcuts = {
                        let settings_port = runtime.settings_port();
                        match tauri::async_runtime::block_on(settings_port.load()) {
                            Ok(settings) => {
                                uc_desktop::shortcuts::resolve_quick_panel_shortcuts(&settings)
                            }
                            Err(e) => {
                                warn!("Failed to load settings for shortcut: {}, using default", e);
                                vec![
                                    uc_desktop::shortcuts::DEFAULT_QUICK_PANEL_SHORTCUT.to_string(),
                                ]
                            }
                        }
                    };

                    // 启动期 setup callback 已在 main thread 上下文，可直接构造 Tauri
                    // 适配器并调注册器。回调闭包绑定 `quick_panel::toggle`，避免桌面
                    // 协调层耦合任何 GUI shell 概念。
                    let toggle_handle = app.handle().clone();
                    let registry = quick_panel::TauriGlobalShortcutRegistry::new(
                        app.handle().clone(),
                        move || quick_panel::toggle(&toggle_handle),
                    );
                    for shortcut_str in &shortcuts {
                        if let Err(e) = registry.register(shortcut_str) {
                            tracing::error!(error = %e, shortcut = %shortcut_str, "Failed to register global shortcut during startup");
                        } else {
                            registered_quick_panel_shortcuts.push(shortcut_str.clone());
                        }
                    }
                } else {
                    info!("Quick panel disabled in settings, skipping global shortcut registration");
                }
            }

            app.manage(uc_desktop::shortcuts::CurrentShortcuts::new(
                registered_quick_panel_shortcuts,
            ));
            app.manage(crate::commands::settings::KeyboardShortcutsUpdateLock::default());

            // Pre-create quick panel (hidden) so the first
            // shortcut press doesn't activate the app via WebviewWindowBuilder::build()
            //
            // 同样按 `quick_panel_enabled` 门控:禁用时不预创建窗口,避免占用
            // webview 资源。用户在设置页开启时由 `set_quick_panel_enabled`
            // 即时补一次 `pre_create`,不需要重启 GUI。
            if quick_panel_enabled {
                quick_panel::pre_create(app.handle());
            }

            // Show window based on silent_start setting
            if !silent_start {
                crate::tray::show_main_window(app.handle());
                info!("Main window show requested (silent_start=false)");
            } else {
                info!("Silent start enabled, main window stays hidden");
            }

            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            app.handle()
                .plugin(tauri_plugin_updater::Builder::new().build())?;

            app.manage(PendingUpdate::new());
            // `LastCheckAt` 跟踪上次任意 source 的 check 完成时间，供 scheduler
            // 被原生唤醒源叫醒时的墙钟 guard 判断「距上次检查是否够久」。初始化为
            // 当前 epoch 而非 0——避免启动后紧接着的一次原生唤醒（如 Windows
            // resume）误判「从没检查过」而在 scheduler 首次 check 之后立刻重复检查。
            app.manage(crate::update_scheduler::LastCheckAt::initialized_now());

            // ADR-008 P3-3 B2': startup file-cache hygiene (reconcile + TTL
            // cleanup) now runs in the daemon (`DaemonApp::run`), which owns the
            // sqlite pool and iroh-blobs actor. The GUI no longer drives it —
            // see `run_startup_file_cache_hygiene` in uc-daemon.

            // Clone handles for async blocks
            let app_handle_for_startup = app.handle().clone();
            let startup_barrier_for_backend = startup_barrier.clone();

            // Spawn the initialization task immediately (don't wait for frontend)
            let runtime = runtime.clone();
            let silent_start_for_barrier = silent_start;
            tauri::async_runtime::spawn(async move {
                info!("Starting backend initialization");

                // 0. Ensure device name is initialized (runs on every startup)
                if let Err(e) = ensure_default_device_name(runtime.settings_port()).await {
                    warn!("Failed to initialize default device name: {}", e);
                    // Non-fatal: continue startup even if device name initialization fails
                }

                // Mark backend-side startup tasks completed. We now finish startup based on backend readiness
                // to avoid deadlocks when the main window is hidden; frontend handles its own loading state.
                info!("[Startup] Backend startup tasks completed, marking backend_ready");
                startup_barrier_for_backend.mark_backend_ready();
                if !silent_start_for_barrier {
                    startup_barrier_for_backend.try_finish(&app_handle_for_startup);
                } else {
                    info!("[Startup] Silent start: skipping startup barrier window show");
                }

                // 1. Auto-unlock (non-blocking) entirely over daemon loopback HTTP.
                //
                // ADR-008 P3-3 B2': the GUI is becoming a pure client, so it can
                // no longer reach an in-process `AppFacade`. All three steps now
                // run as RPCs against the daemon, which owns the encryption
                // session and settings: read `auto_unlock_enabled` via
                // `GET /settings`, then `POST /encryption/unlock` (silent keyring
                // resume — no passphrase; the daemon endpoint preserves the
                // original semantics), then `POST /lifecycle/retry` to advance
                // the daemon-side deferred services. Because every step is an
                // RPC, we wait for `connection_state` to be populated up front
                // instead of unlocking before the daemon is reachable.
                let daemon_conn_for_unlock = daemon_connection_state.clone();
                tauri::async_runtime::spawn(async move {
                    if !wait_for_daemon_connection(
                        &daemon_conn_for_unlock,
                        AUTO_UNLOCK_DAEMON_READY_TIMEOUT,
                        AUTO_UNLOCK_DAEMON_READY_POLL,
                    )
                    .await
                    {
                        warn!(
                            timeout_secs = AUTO_UNLOCK_DAEMON_READY_TIMEOUT.as_secs(),
                            "[Startup] Daemon connection not ready in time; skipping auto-unlock + lifecycle retry"
                        );
                        return;
                    }

                    let settings_client = uc_daemon_client::DaemonSettingsClient::new(
                        daemon_conn_for_unlock.clone(),
                    );
                    let auto_unlock_enabled = match settings_client.get_settings().await {
                        Ok(settings) => settings.security.auto_unlock_enabled,
                        Err(e) => {
                            warn!(error = %e, "[Startup] Failed to load settings for auto unlock");
                            false
                        }
                    };

                    if !auto_unlock_enabled {
                        info!("[Startup] Auto unlock disabled by settings");
                        return;
                    }

                    let client = uc_daemon_client::DaemonQueryClient::new(daemon_conn_for_unlock);
                    match client.unlock_encryption().await {
                        Ok(true) => {
                            info!("[Startup] Encryption auto-unlocked via daemon");
                        }
                        Ok(false) => {
                            info!(
                                "[Startup] Encryption not initialized or keyring miss; skip auto-unlock"
                            );
                            return;
                        }
                        Err(e) => {
                            warn!(
                                error = %e,
                                "[Startup] Daemon auto-unlock failed; user will need to enter passphrase via Unlock modal"
                            );
                            return;
                        }
                    }

                    // Lifecycle retry drives the daemon-side deferred services
                    // (clipboard watcher / sync) into their running state.
                    if let Err(e) = client.lifecycle_retry().await {
                        warn!("[Startup] Daemon lifecycle retry failed: {}", e);
                    } else {
                        info!("[Startup] Daemon lifecycle boot completed");
                    }
                });

                // 2. Update scheduler (Phase 3C).
                //
                // `update_scheduler::run` 内部先 poll `setup_status.has_completed`，
                // 所以这里可以立即 spawn，无需 gate 在 device-name / auto-unlock
                // 之后。挂在 `task_registry` 上，`ExitRequested` 路径
                // (`task_registry_for_run.token().cancel()`) 会级联取消 child token，
                // scheduler 的 `tokio::select!` 立即返回。
                //
                // `LastNotifiedUpdateStore` 一次性 load 到 Mutex —— Phase 4B 通知
                // 去重时通过 `deps.last_notified` 写入并 persist。
                let last_notified_path =
                    runtime.storage_paths().last_notified_update_path();
                let store = crate::update_scheduler::LastNotifiedUpdateStore::load(
                    &last_notified_path,
                )
                .await;
                // 同一个 Arc<NotifyContext> 同时给 scheduler 和托盘手动检查
                // 用：app.manage 一份，SchedulerDeps 收一份。
                // 共享意味着去重 mutex / 落盘路径 / analytics 出口完全一致。
                let notify_ctx = Arc::new(crate::update_scheduler::NotifyContext {
                    app_handle: app_handle_for_startup.clone(),
                    analytics: runtime.analytics(),
                    last_notified: Arc::new(tokio::sync::Mutex::new(store)),
                    last_notified_path,
                });
                app_handle_for_startup.manage(notify_ctx.clone());
                let scheduler_deps = crate::update_scheduler::SchedulerDeps {
                    settings_port: runtime.settings_port(),
                    setup_status_port: runtime.setup_status_port(),
                    notify: notify_ctx,
                };

                // 平台原生唤醒源：让后台周期检查在 macOS App Nap / Windows Modern
                // Standby 下也能发车——否则 scheduler 的 tokio::sleep 被系统挂起，
                // 更新检查只有在打开主窗口时才触发（被反复误修的老症状）。
                //
                // channel 容量 1：堆积多个 tick 无意义，满了 try_send 直接丢即可。
                // 一份 sender 交给唤醒源，另一份作为 keepalive 移进 task——这样在
                // 没有原生唤醒源的平台（Linux）上 channel 也不会提前关闭，
                // `wake_rx.recv()` 不会返回 None 触发退化路径。
                let (wake_tx, wake_rx) = tokio::sync::mpsc::channel::<()>(1);
                crate::update_scheduler::start_wake_source(
                    &app_handle_for_startup,
                    wake_tx.clone(),
                    crate::update_scheduler::scheduler::SUCCESS_BASE_INTERVAL,
                );
                runtime
                    .task_registry()
                    .spawn("update_scheduler", move |token| async move {
                        use tracing::Instrument;
                        let _wake_keepalive = wake_tx;
                        crate::update_scheduler::run(scheduler_deps, wake_rx, token)
                            .instrument(tracing::info_span!("update_scheduler"))
                            .await;
                    })
                    .await;
            });

            info!("App runtime initialized, backend initialization started");
            Ok(())
        })
        // 命令清单从 `specta_builder.rs` 收口；这里只把 builder 装进 runtime。
        .invoke_handler(specta_builder.invoke_handler())
        .build(tauri_ctx)
        .map_err(|error| anyhow::anyhow!("error building tauri application: {error}"))?
        .run(move |app_handle, event| {
            match event {
                tauri::RunEvent::ExitRequested { .. } => {
                    info!("App exit requested, cancelling all tracked tasks");
                    task_registry_for_run.token().cancel();
                    // ADR-008 D3 (P4-3): three-state quit. The daemon is always a
                    // separate process. Only an explicit "彻底退出" (tray Quit)
                    // sets QuitIntent → stop the daemon (regardless of who spawned
                    // it; revised D3). Window close (hide), lightweight mode, Cmd-Q
                    // and restart all leave the daemon running. The daemon's own
                    // SIGTERM handler (D21) drains in-flight work; the GUI does not
                    // block. Identity + legacy-in-process safety live in the helper.
                    if app_handle
                        .state::<crate::lightweight::QuitIntent>()
                        .should_stop_daemon()
                    {
                        let stopped = uc_desktop::daemon_probe::stop_local_daemon_on_full_quit();
                        info!(stopped, "full quit: local daemon stop attempt complete");
                    }
                }
                tauri::RunEvent::Exit => {
                    info!("Application exiting");
                }
                // macOS: 点击 Dock 图标时，若没有可见窗口则恢复主窗口。
                #[cfg(target_os = "macos")]
                tauri::RunEvent::Reopen {
                    has_visible_windows: false,
                    ..
                } => {
                    info!("Dock reopen with no visible windows, showing main window");
                    crate::tray::show_main_window(app_handle);
                }
                _ => {}
            }
        });

    Ok(())
}
