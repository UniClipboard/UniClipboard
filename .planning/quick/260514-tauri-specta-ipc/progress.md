# Progress

## 2026-05-14 (会话启动)

- 完成 issue / 代码调研，盘点出 33 个 `#[tauri::command]`（12 个文件），
  含 4 个特殊形态（Channel / generic<R> / Option<Option<T>> / HashMap+Option）。
- 选定 `tauri-specta@2.0.0-rc.25 + specta@2.0.0-rc.25 + specta-typescript@0.0.12`。
- 创建分支 `feat/tauri-specta-ipc`（基于 `origin/main` @ `c9622078`）。
- Phase A 起步：
  - `uc-platform/Cargo.toml`：加 optional `specta` feature + dep。
  - `uc-platform/src/ports/observability.rs`：`TraceMetadata` 用
    `#[cfg_attr(feature = "specta", derive(specta::Type))]`。
  - 待办：改 `uc-tauri/Cargo.toml` + 写 `specta_builder.rs` + 改 `run.rs` +
    标注所有 commands。

## 2026-05-14 → 2026-05-15 (Phase A/B/C 主体完成，progress.md 当时未同步)

回填记录。源代码为准。

### Phase A — Rust 基础设施 + 全量 commands 标注 ✅

- `uc-tauri/Cargo.toml`：加 `tauri-specta` / `specta` / `specta-typescript`
  + `uc-platform/specta` 启用。
- `uc-tauri/src/specta_builder.rs`（94 行）：封装
  `Builder::<tauri::Wry>::new().commands(collect_commands![...])`。
- `uc-tauri/src/run.rs`：`tauri::Builder` 接 `builder.invoke_handler()`
  + `setup` 里 `mount_events`。
- 33/33 命令带 `#[specta::specta]`（grep 数量一致）。
- `commands/error.rs` 的 `CommandError` + 各 thiserror enum
  （`UnlockSpaceCommandError` / `TrySilentUnlockError` /
  `FactoryResetCommandError`）派生 `specta::Type`。
- 特殊形态全部就位：
  - `install_update` 的 `Channel<DownloadEvent>` 走 `tauri::ipc::Channel`，
    typed 客户端能拿到回调签名。
  - `mac_rounded_corners` 的 `<R: Runtime>` 用 `generic::<tauri::Wry>`
    显式实例化，且 **所有平台都 collect**（非 macOS 返回 no-op），
    保证 binding 跨平台一致。
  - `Option<Option<T>>` 三态字段配 `#[specta(type = Option<T>)]` 注解。
  - `HashMap<String, Option<ShortcutKeyDto>>` 直接 work，无须额外注解。

### Phase B — Codegen pipeline + bindings 文件落地 ✅

- `src-tauri/crates/uc-tauri/tests/specta_export.rs`：cargo test 触发
  `builder.export(Typescript::default(), ...)`，写入
  `src/lib/ipc-bindings.generated.ts`（712 行）。
- 顶部 banner: `@ts-nocheck` + eslint-disable + prettier-ignore。
- `.prettierignore` 加 `src/lib/ipc-bindings.generated.ts`。
- `src/lib/ipc.ts`（215 行）：Proxy wrapper，把生成的 `commands.xxx` 包成
  注入 trace_id / Sentry breadcrumb / 红 ed args 的 typed 调用。
- u64/i64/usize/isize 字段统一标 `Number<...>` 或改 `String`，
  绕开 `bigint_forbidden`。

### Phase C — 前端切换 + drift check ✅（除已知 5 个孤儿命令）

切换到 `commands.xxx`：

- `src/api/tauri-command/{mobile_sync,space_setup,settings,factory_reset}.ts`
- `src/api/{updater,runtime,storage}.ts`
- `src/lib/daemon-connection-info.ts`
- `src/quick-panel/ClipboardHistoryPanel.tsx`
- `src/contexts/{SettingContext,UpdateContext}.tsx`
- `src/components/setting/NetworkSection.tsx`
- `src/components/device/MobileSyncSettingsSheet.tsx`

CI drift check:

- `.github/workflows/pr-check.yml` 加 `cargo test --test specta_export` +
  `git diff --exit-code src/lib/ipc-bindings.generated.ts` 闸门。

文档：

- `docs/agent/rust-tauri-rules.md` +28 行「tauri-specta IPC bindings」
  小节，覆盖 Rust 标注 / codegen / macOS-only 规则。

## 2026-05-15 (收尾会话)

### 步骤 1 — vault / clipboardItems 5 个孤儿命令处理 ✅

确认 Rust 端 grep 不到 `check_vault_status` / `reset_vault` /
`sync_clipboard_items` / `download_file_entry` / `open_file_location`，
是 pre-existing 死代码 / 未实现，与本次 tauri-specta 无关。

按用户决策：

- **删** `src/api/vault.ts`（零调用方，纯死代码）。
- **保留** `clipboardItems.ts` 的三个函数（ActionBar /
  ClipboardContent / FileContextMenu 仍调用），加 TODO 注释指明
  「Rust 端 command 不存在，待 daemon 化或补 Rust command」。

### 步骤 2 — `git add ipc-bindings.generated.ts` ✅

之前是 untracked，现已 staged。CI drift check 才能真正起作用。

### 步骤 3 — 本地验证

| 验收项 | 命令 | 结果 |
|--------|------|------|
| Specta drift | `cargo test -p uc-tauri --test specta_export` | ✅ pass，`git diff` 为空 |
| Frontend tests | `bun run test` | ⚠️ 471 pass / 1 fail |
| Frontend build (typecheck) | `bun run build` | ✅ pass，built in 19.65s |
| Workspace tests | `cargo test --workspace --locked` | ⚠️ unit/integration ✅；13 个 doctest 失败 |

**前端 1 个失败**：`src/components/search/__tests__/AdvancedSearch.test.tsx:110`
`expect(onKeyDown).not.toHaveBeenCalled()`。`git diff main...HEAD --
src/components/search/**` 为空，本分支未碰；最近一次改动是
`56da8e8c feat: local encrypted search`。判定为 pre-existing flaky test，
与本次 IPC 迁移无关。

**Cargo workspace 13 个 doctest 失败**：全部在
`src-tauri/crates/uc-daemon-local/src/{auth,socket}.rs` 的 `///` 示例块里，
找不到 `parse_bearer_token` / `repair_token_permissions` /
`resolve_daemon_http_addr` 等函数（缺 `use` 路径或函数本身私有）。

- `git diff main...HEAD --name-only -- 'src-tauri/crates/uc-daemon-local/**'`
  为空 —— 本分支未碰这些文件。
- CI 实际只跑 `cargo check --workspace --locked`（`pr-check.yml:142`），
  不跑 `cargo test --workspace`，所以这些 doctest 在 main 上也一直失败但
  没被门控住。
- 判定为 pre-existing，不阻塞 IPC PR；可以另开 issue 修。

unit / integration tests 全部通过（输出 50 行只剩 doctest 失败 tail，
前面所有 lib/test target 的 `test result: ok` 行被 `tail -50` 截掉了）。

### 步骤 4 — 文档

- `docs/agent/rust-tauri-rules.md`: 已在 Phase C 完成。
- `docs/agent/frontend-ui-rules.md`: 加「Calling Tauri commands (issue #698)」
  小节，给出 typed `commands.xxx` 用法示例 + 引用 rust-tauri-rules。
- `AGENTS.md`: 不动。它是索引；新规范已落到上述两个 focused doc。

### 仍未完成

- commit + 创建 PR（issue #698）。

### 已知 follow-up（不在本 PR 范围）

- `clipboardItems.ts` 的 `syncClipboardItems` / `downloadFileEntry` /
  `openFileLocation` 三个函数：Rust 端 command 缺失，UI 触发时 runtime 报
  "command not found"。已加 TODO 注释指向 issue #698 follow-up。
- `uc-daemon-local::auth` / `socket` 模块 13 个 pre-existing doctest 失败，
  CI 没门控；建议另开 issue 跟进。
- `AdvancedSearch.test.tsx:110` keydown 测试 pre-existing flaky。

## 2026-05-15 (CI 修复 + 死代码清理)

### Rebase onto origin/main (10 commits ahead → 1 ahead)

冲突解决：

- `Cargo.lock` 取 main 后 cargo check 重解（加回 specta deps）。
- `MobileSyncSettingsSheet.tsx` 被 main #726 删（替换为 `MobileSyncSettingsDialog`），
  接受删除；新 Dialog 的 `MobileSyncError` switch 因 typed enum 暴露
  pre-existing 笔误 `ENDPOINT_INFO_PROBE_FAILED` →
  `ENDPOINT_INFO_FAILED`（i18n key 同步改），独立 commit `fix(mobile-sync):`。

### CI 红：mac_rounded_corners 跨平台编译失败

`pr-check.yml` Linux runner 报 `cannot find mac_rounded_corners in plugins`：
`plugins/mod.rs` 用 `#[cfg(target_os = "macos")]` 关掉了整个 mod，
但我在 `specta_builder.rs` 里所有平台都引用了它的三个 `<R: Runtime>` 命令。

调研发现：

- 前端 `TitleBar.tsx` 用的是 npm `@cloudworxx/tauri-plugin-mac-rounded-corners`
  独立 plugin，不调本地 Rust 命令。
- Rust 端这三个函数除自身定义 + `specta_builder.rs` 注册外零调用。
- `objc2` deps 仍被 `quick_panel/macos.rs` 用，不能动。

直接删整个 `plugins/` 目录（mod.rs + mac_rounded_corners.rs）+ `lib.rs`
的 `pub mod plugins;` + `specta_builder.rs` 的三行注册。重新跑
`cargo test --test specta_export` 后 binding 文件 -9 行
（`enableRoundedCorners` / `enableModernWindowStyle` /
`repositionTrafficLights` 三个 typed entry 全清）。

命令计数 33 → 30。

更新文档：

- `specta_builder.rs` 的「macOS 平台命令」段改成「平台一致性」。
- `docs/agent/rust-tauri-rules.md` 第 3 条改成「平台条件命令必须跨平台编译」
  指引（避免后人再踩同样的 mod cfg gate 坑）。
