# Findings

## 现有 IPC 表面盘点（2026-05-14）

### Rust 端：33 个 `#[tauri::command]`，分布在 12 个文件

| 文件 | 命令 | 备注 |
|------|------|------|
| `commands/mod.rs` | `get_tauri_pid` `get_device_id` `get_device_meta` | DTO: `DeviceMeta` |
| `commands/startup.rs` | `get_daemon_connection_info` | DTO: `DaemonConnectionPayload`，错误用 `CommandError` |
| `commands/tray.rs` | `set_tray_language` | 无 DTO |
| `commands/restart.rs` | `restart_app` | 错误用 `CommandError` |
| `commands/autostart.rs` | `enable_autostart` `disable_autostart` `is_autostart_enabled` | 错误是 `String` |
| `commands/updater.rs` | `check_for_update` `download_update` `cancel_download` `get_download_progress` `install_update` | DTO: `UpdateMetadata` `DownloadEvent` `DownloadProgressSnapshot` `DownloadPhase`；`install_update` 用 `Channel<DownloadEvent>` |
| `commands/storage.rs` | `open_data_directory` | 错误用 `CommandError` |
| `commands/quick_panel.rs` | `paste_to_previous_app` `dismiss_quick_panel` `set_quick_panel_layout` `finalize_quick_panel_show` | 错误是 `String` |
| `commands/settings.rs` | `update_keyboard_shortcuts` | 入参 `HashMap<String, Option<ShortcutKeyDto>>`；DTO `UpdateKeyboardShortcutsResult` |
| `commands/mobile_sync.rs` | `register_mobile_device` `revoke_mobile_device` `list_mobile_devices` `rotate_mobile_password` `get_mobile_sync_settings` `update_mobile_sync_settings` `list_mobile_lan_interfaces` | 多 DTO；`update_mobile_sync_settings` 三态 `Option<Option<T>>` |
| `commands/space_setup.rs` | `unlock_space_with_passphrase` `try_silent_unlock` | DTO + thiserror enum (`UnlockSpaceCommandError` / `TrySilentUnlockError`) |
| `commands/factory_reset.rs` | `factory_reset_space` | thiserror `FactoryResetCommandError` |
| `plugins/mac_rounded_corners.rs` | `enable_rounded_corners` `enable_modern_window_style` `reposition_traffic_lights` | `<R: Runtime>` 泛型 + `#[cfg(target_os = "macos")]` 平台条件注册 |

合计 **33 个命令**（issue 描述的 "14 个文件" 实际是 12 个文件 33 个命令）。

### 前端调用站点

| 文件 | 通过 | 命令数 |
|------|------|--------|
| `src/api/tauri-command/{mobile_sync,space_setup,settings,factory_reset}.ts` | `invokeWithTrace` 已有 wrapper | 11 |
| `src/api/updater.ts` | `invokeWithTrace` | 5 |
| `src/api/runtime.ts` `src/api/vault.ts` `src/api/storage.ts` `src/api/clipboardItems.ts` | `invokeWithTrace` | ~7 |
| `src/lib/daemon-connection-info.ts` | `invokeWithTrace` | 1 |
| `src/contexts/SettingContext.tsx` 等 | `invokeWithTrace`（散落） | 数个 |
| `src/quick-panel/ClipboardHistoryPanel.tsx` | **裸 `invoke()`** | 4（注意：不走 `invokeWithTrace`） |

## tauri-specta 2.0.0-rc.25 关键 API

来自 `examples/app/src-tauri/src/main.rs`：

```rust
let builder = Builder::<tauri::Wry>::new()
    .semantic_types(semantic::Configuration::default())  // 启用 Date/Uint8Array/URL
    .commands(tauri_specta::collect_commands![
        hello_world, async_hello_world, has_error,
        nested::some_struct, generic::<tauri::Wry>, deprecated,
        with_channel, phase_specific_rename,
        typesafe_errors_using_thiserror,
    ])
    .events(tauri_specta::collect_events![DemoEvent, EmptyEvent])
    .typ::<Custom>()
    .constant("universalConstant", 42);

#[cfg(debug_assertions)]
builder.export(Typescript::default(), "../src/bindings.ts").unwrap();

tauri::Builder::default()
    .invoke_handler(builder.invoke_handler())
    .setup(move |app| { builder.mount_events(app); Ok(()) })
    .run(...)
```

要点：
- `tauri::ipc::Channel<T>` 自动支持
- `generic::<tauri::Wry>` 用显式实例化
- thiserror enum 加 `#[derive(Serialize, Type)]` 自动 typed error
- `#[cfg(...)]` 命令可以在 `collect_commands!` 内部不被列入（用 `#[cfg]` 守护）
- 默认生成 `import { invoke as __TAURI_INVOKE } from "@tauri-apps/api/core"`

## invoke transport 注入

`src/lang/js_ts.rs` 显示生成的 TS 文件 **硬编码** import：
```ts
import { invoke as __TAURI_INVOKE } from "@tauri-apps/api/core";
```

`specta-typescript` 的 `Typescript` 配置不暴露替换该 import 的钩子。
**结论**：保留 `invokeWithTrace` 可观测性的唯一办法是在前端薄包一层：

```ts
// src/lib/ipc.ts
import { commands as raw } from './ipc-bindings.generated'
import { invokeWithTrace } from './tauri-command'

// 把 raw.commands.xxx 替换成走 invokeWithTrace 的等价 typed wrapper。
// 由于 raw 的每个 method 都是固定签名（cmd_name + args），
// 用一个动态 Proxy 即可统一注入 trace。
```

但这意味着 `commands` 的类型签名必须从 generated 文件中拿，
而调用路径走 `invokeWithTrace`。Proxy + 重新映射函数名是最稳的方案。

## TraceMetadata 处理

`TraceMetadata { trace_id: Uuid, timestamp: u64 }` 在 `uc_platform::ports::observability`。
Uuid 需要 `specta` 的 `"uuid"` feature 才能派生 `Type`。
派生后会被生成为 TS `string`（UUID 标准做法）。
