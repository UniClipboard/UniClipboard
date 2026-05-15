# 引入 tauri-specta 生成 Rust ↔ TS 类型化客户端

> 对应 issue：https://github.com/UniClipboard/UniClipboard/issues/698
>
> Branch: `feat/tauri-specta-ipc`（基于 origin/main）

## 背景

`src-tauri/crates/uc-tauri/src/commands/` 当前 33 个 `#[tauri::command]`
全部是手写 `#[tauri::command]` + 前端 `invoke('cmd_name', { ... })` 字符串
契约。Rust 改字段前端不会编译失败，只能在 runtime 撞 serde 错误。

引入 [`tauri-specta`](https://github.com/specta-rs/tauri-specta)（基于
[`specta`](https://github.com/specta-rs/specta)）让每个 `#[command]` 自动：

1. 生成对应的 TS 函数（typed `invoke` wrapper）。
2. 生成所有 DTO 的 TS 类型。
3. Rust 端改字段时前端 TS 立刻 fail-build。

## Goal

- Rust 端：`uc-tauri` 装上 `specta@2.0.0-rc.25 / specta-typescript@0.0.12 /
  tauri-specta@2.0.0-rc.25`；`tauri::generate_handler!` 换成
  `tauri_specta::Builder::new().commands(collect_commands![...])`，可观测性
  / plugin 注册 / runtime 装配链全部保留。
- 33 个 `#[tauri::command]` 全部加 `#[specta::specta]`，所有 wire DTO
  与 thiserror error enum 派生 `specta::Type`。
- Codegen：`cargo test -p uc-tauri --test specta_export` 把 typed
  bindings 写到 `src/lib/ipc-bindings.generated.ts`。这份产物**纳入
  git**——CI 在前端 build 前跑 cargo test，git diff 必须为空，否则报
  "IPC schema drift"。
- 前端：在 `src/lib/ipc.ts` 用 generated `commands` 包一层 typed
  proxy 走 `invokeWithTrace`，保留 trace_id / Sentry breadcrumb /
  redaction 不丢；逐步把 `src/api/tauri-command/*` + 散落的 `invoke('xxx')`
  / `invokeWithTrace('xxx')` 切换到 `commands.xxx(args)`。

## 验收

- [ ] `bun run build` 时若 Rust DTO 与前端 TS 类型不一致 → CI 红
- [ ] `cargo test --workspace` 全绿
- [ ] `bun run test` 全绿
- [ ] `git diff src/lib/ipc-bindings.generated.ts` 为空（CI drift check）
- [ ] 前端至少切换 mobile_sync / updater / runtime / settings 这几个
      高频 API 走 generated `commands.xxx(args)`，证明 fail-build 链路真实可用

## 范围拆分

### Phase A — Rust 基础设施 + 全量 commands 标注

**Status**: ✅ complete

- `uc-platform`：可选 `specta` feature，让 `TraceMetadata` 派生 `Type`
- `uc-tauri/Cargo.toml`：加 specta/specta-typescript/tauri-specta deps
- `uc-tauri/src/specta_builder.rs`：封装 `Builder<tauri::Wry>::new()` 构造
- `uc-tauri/src/run.rs`：`invoke_handler(builder.invoke_handler())`
- 给 33 个 `#[tauri::command]` 加 `#[specta::specta]`
- 给所有 DTO + thiserror error 派生 `specta::Type`
- 处理特殊形态：
  - `Channel<DownloadEvent>`（updater）
  - `generic<R: Runtime>`（mac_rounded_corners）
  - `Option<Option<T>>`（mobile_sync update settings）
  - `HashMap<String, Option<ShortcutKeyDto>>`（settings）
  - `#[cfg(target_os = "macos")]` 平台命令（mac_rounded_corners）

### Phase B — Codegen pipeline + bindings 文件落地

**Status**: ✅ complete

- `uc-tauri/tests/specta_export.rs`：cargo test 触发 `builder.export()`
  写 `src/lib/ipc-bindings.generated.ts`
- 该文件加进 `.prettierignore` / `eslint-disable`，git track
- `src/lib/ipc.ts`：generated `commands` 的 proxy wrapper，注入 trace + breadcrumb

### Phase C — 前端切换 + drift check

**Status**: ✅ complete (vault.ts / clipboardItems 三个孤儿命令属 pre-existing
out-of-scope，已在 progress.md 标注)

- `src/api/tauri-command/{mobile_sync,space_setup,settings,factory_reset}.ts`
  改成 thin re-export of `commands.xxx`
- `src/api/{updater,runtime,vault,storage,clipboardItems}.ts` 同样替换
- 散落的 `invokeWithTrace('cmd_name', ...)` / `invoke('cmd_name', ...)`
  调用全部替换
- CI workflow 加 cargo test → git diff → 失败即 schema drift
- 更新 AGENTS.md / docs/agent/{rust-tauri,frontend-ui}-rules.md

## 关键决策

| 时间 | 决策 | 理由 |
|------|------|------|
| 2026-05-14 | 选 `tauri-specta` 而非 `ts-rs` | issue 已比较：`ts-rs` 只生成 DTO，invoke 仍是字符串，价值减半 |
| 2026-05-14 | `uc-platform` 用 optional feature `specta` 引 specta | 平台 crate 默认不依赖 specta，零成本；只在 `uc-tauri` 启用 `uc-platform/specta` 时才编进去 |
| 2026-05-14 | bindings.ts **git track**，CI diff check schema drift | tauri-specta 默认 invoke 硬编码 `@tauri-apps/api/core`；要保留 `invokeWithTrace` 必须在前端薄包一层，所以 generated 文件不能直接被前端调用，需要 wrapper |
| 2026-05-14 | Codegen 走 `cargo test --test specta_export` 而非 build.rs | 官方示例在 `main.rs` 里 `#[cfg(debug_assertions)]` 调 `builder.export()`；但项目用 workspace lib crate (`uc-tauri`)，把它放到 dev-test target 更可控，CI 也能稳定触发 |

## 风险与未决问题

- **Channel<DownloadEvent>**：tauri-specta example 显示支持 `tauri::ipc::Channel<i32>`，
  应可工作；如有问题降级方案是把 install_update 标 `skip` 让它走原始 invoke 路径。
- **macOS 平台条件命令**：Builder 内的 `collect_commands!` 也要做 `#[cfg]`，
  确保 non-macOS 平台 builder 不引用未编译命令。
- **`Option<Option<T>>` 三态字段**：specta 对 `Option<Option<T>>` 的支持
  待验证；若不支持需要拆成专用 enum 或 `#[specta(type = ...)]` 注解。
- **生成产物体积**：33 commands + 多个复杂 enum/struct，bindings 文件
  可能上千行；git diff review 时若 noise 太大需要考虑拆成多个文件
  （`Typescript::default().layout(Layout::Files)`）。
