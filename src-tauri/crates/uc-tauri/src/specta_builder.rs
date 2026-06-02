//! tauri-specta builder —— IPC commands 的"单一真相源"。
//!
//! ## 为什么需要这个模块
//!
//! 历史上 `tauri::generate_handler![...]` 是 `run.rs` 里硬编码的命令清单，
//! 改一个命令的 DTO 字段名，前端要等 runtime invoke 报 serde 错才知道。
//! 引入 `tauri-specta` 后我们用 [`build`] 把同一份命令清单同时喂给两条管道：
//!
//! 1. **运行时**：`builder.invoke_handler()` 直接接进 `tauri::Builder::invoke_handler`。
//! 2. **codegen**：`tests/specta_export.rs` 调 `builder.export(...)` 写出
//!    `src/lib/ipc-bindings.generated.ts`，CI 跑同一个 test → `git diff
//!    --exit-code` 检查 schema drift。
//!
//! 两条管道用同一个 `Builder` 实例的好处：清单只在一个地方维护，
//! "前端看到的 API 表面" 与 "后端注册的 invoke handler" 在编译期就被
//! 强制对齐——少注册一个命令，前端 TS 就少一个函数；多注册一个，TS 就
//! 多一个但前端不调它，CI drift check 会立刻报错。
//!
//! ## 平台一致性
//!
//! 所有命令在所有 OS 上都 collect，保证任何 runner 跑 `cargo test
//! --test specta_export` 得到同一份 binding（CI 可以用单一 Linux runner
//! 做 schema drift check）。当前 33 条命令都不依赖平台特定 mod 编译。

use tauri_specta::{collect_commands, collect_events, Builder};

/// 构造 IPC commands 的 tauri-specta `Builder`。
///
/// 调用方两种：
/// - `crate::run::run()` —— `builder.invoke_handler()` 接给 Tauri runtime
/// - `tests/specta_export.rs` —— `builder.export(...)` 写 binding TS 文件
///
/// 两边必须拿到 *结构上等价* 的 builder（命令列表、events、constants 都一致），
/// 否则前端 TS 类型与后端实际可调用的命令会漂移。所以这里把两条路径
/// 都收口到这一个函数。
pub fn build() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .events(collect_events![
            // Issue #747 Phase 5:dispatch fan-out 完成、delivery 写入后
            // 由 `TauriHostEventEmitter` emit;事件 payload 只携带
            // (entry_id, target_device_id),前端按 entry_id 匹配后 refetch
            // view 拿 status 真相 —— 事件本身不承载状态。事件丢失被前端
            // 的幂等 refetch 吸收,所以不在 specta 层加任何"必须收到"
            // 的保护。
            crate::host_event_emitter::ClipboardDeliveryStatusChanged,
        ])
        .commands(collect_commands![
            // ── tray ────────────────────────────────────────────────────────────
            crate::commands::tray::set_tray_language,
            // ── lifecycle / device ──────────────────────────────────────────────
            crate::commands::get_tauri_pid,
            crate::commands::get_device_id,
            crate::commands::get_device_meta,
            crate::commands::startup::get_daemon_connection_info,
            crate::commands::startup::get_daemon_session,
            // ── restart (Phase 95) ──────────────────────────────────────────────
            crate::commands::restart::restart_app,
            // ── autostart ───────────────────────────────────────────────────────
            crate::commands::autostart::update_autostart,
            // ── updater ─────────────────────────────────────────────────────────
            crate::commands::updater::check_for_update,
            crate::commands::updater::download_update,
            crate::commands::updater::cancel_download,
            crate::commands::updater::get_download_progress,
            crate::commands::updater::install_update,
            crate::commands::updater::get_install_kind,
            crate::commands::updater::dev_open_updater_window,
            // ── update telemetry (Phase 5A) ─────────────────────────────────────
            crate::commands::update_telemetry::capture_update_ui_event,
            // ── storage ─────────────────────────────────────────────────────────
            crate::commands::storage::open_data_directory,
            crate::commands::storage::open_logs_directory,
            // ── clipboard delivery view (entry detail "源 + 同步状态") ──────────
            crate::commands::clipboard_delivery::clipboard_entry_delivery_view,
            // ── clipboard resend (ADR-005 Stage 1a) ─────────────────────────────
            crate::commands::clipboard_delivery::clipboard_resend_entry,
            // ── file transfer (P1: 接收方主动取消进行中的大文件传输) ───────────
            crate::commands::file_transfer::cancel_file_transfer,
            // ── quick panel ─────────────────────────────────────────────────────
            crate::commands::quick_panel::paste_to_previous_app,
            crate::commands::quick_panel::dismiss_quick_panel,
            crate::commands::quick_panel::set_quick_panel_layout,
            crate::commands::quick_panel::finalize_quick_panel_show,
            crate::commands::quick_panel::set_quick_panel_enabled,
            // ── settings ────────────────────────────────────────────────────────
            crate::commands::settings::update_keyboard_shortcuts,
            crate::commands::settings::probe_relay_url,
            // ── mobile sync ─────────────────────────────────────────────────────
            crate::commands::mobile_sync::register_mobile_device,
            crate::commands::mobile_sync::revoke_mobile_device,
            crate::commands::mobile_sync::list_mobile_devices,
            crate::commands::mobile_sync::rotate_mobile_password,
            crate::commands::mobile_sync::get_mobile_sync_settings,
            crate::commands::mobile_sync::update_mobile_sync_settings,
            crate::commands::mobile_sync::list_mobile_lan_interfaces,
            // ── space setup ─────────────────────────────────────────────────────
            crate::commands::space_setup::unlock_space_with_passphrase,
            crate::commands::space_setup::try_silent_unlock,
            // ── factory reset ───────────────────────────────────────────────────
            crate::commands::factory_reset::factory_reset_space,
            // ── window chrome (macOS traffic lights) ────────────────────────────
            crate::commands::window_chrome::set_traffic_light_position,
        ])
}
