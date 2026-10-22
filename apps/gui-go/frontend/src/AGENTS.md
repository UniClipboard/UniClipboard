# FRONTEND (SRC)

Follow root rules in `AGENTS.md`. This file adds frontend-only guidance.

## OVERVIEW

React 19 + TypeScript + Vite UI layer for desktop app flows (setup, unlock, dashboard, devices, settings). Since ADR-008 the UI is primarily a client of the standalone `uniclipd` daemon over loopback HTTP + WebSocket (`src/api/daemon/`, generated SDK in `src/api/generated/`, realtime via `src/lib/daemon-ws.ts`); a shrinking set of native-only operations go through the Wails `HostService` bindings (`src/lib/ipc.ts`, wrapped in `src/api/`), and the shell's window, event, opener and notification surface comes from `src/host/`.

## STRUCTURE

```text
src/
|- main.tsx            # frontend entry, Sentry/init logging
|- App.tsx             # router + providers + setup/encryption gating
|- api/                # backend wrappers; api/daemon/ (HTTP) + api/generated/ (hey-api SDK) + host command wrappers
|- lib/ipc.ts          # host `commands`: generated Wails HostService bindings plus trace/Sentry wrapper
|- host/               # Wails host modules (@/host/*) and the per-document host entries; the only path to the native shell
|- lib/daemon-ws.ts    # daemon WebSocket client (realtime events, snapshot, reconnect)
|- store/              # Redux Toolkit + RTK Query
|- pages/              # route pages + setup steps
|- components/         # feature and ui components
|- layouts/            # window/app layout shells
|- contexts/           # app-level providers
|- hooks/              # behavior hooks
|- observability/      # sentry/trace/redaction
`- test/               # vitest setup
```

## WHERE TO LOOK

| Task                        | Location                                       | Notes                                        |
| --------------------------- | ---------------------------------------------- | -------------------------------------------- |
| App bootstrap               | `src/main.tsx`                                 | `Provider`, Sentry init, platform typography |
| Routing and auth-like gates | `src/App.tsx`                                  | setup state + encryption session routing     |
| Daemon HTTP/WS client       | `src/api/daemon/` + `src/api/generated/` + `src/lib/daemon-ws.ts` | ADR-008: primary path to the `uniclipd` daemon |
| Host (native) commands      | `src/api/` + `src/lib/ipc.ts`                  | typed `commands` over the generated Wails bindings (`@host/*`) |
| Global state                | `src/store/`                                   | `store/api.ts` + slices + hooks              |
| Setup flow UI               | `src/pages/SetupPage.tsx` + `src/pages/setup/` | multi-step onboarding/join flows             |
| Shared UI primitives        | `src/components/ui/`                           | Radix/shadcn-style components                |
| Platform UI kit             | `src/ui-kit/`                                  | `@/ui-kit`; contract in `contract.ts`, skins (windows, macos, linux) chosen by `UC_UI_PLATFORM` at build time |
| Kit entry pages             | `src/pages/entry-kit/`                         | startup, status, recovery, unlock and setup pages on `@/ui-kit`; reached only through the `@ui-*-page` / `@ui-*-screen` build-time aliases (macOS/Windows → kit, otherwise the existing pages) |
| Kit Manage Devices | `src/pages/devices-kit/` | macOS/Windows/Linux: the same `Sheet` as Manage Tags over the kit History window, opened from the labelled Manage Devices entry above Settings in the sidebar footer (sidebar device rows filter history by source); `/devices` resolves through `@ui-devices-page` to a redirect that opens it. State comes from `useDevicesPage` and the shared hooks in `hooks/`; browser check: `e2e/devices-kit-browser.mjs` |
| Kit settings | `src/pages/settings-kit/`, `src/components/setting/setting-parts/` | macOS and Windows: `@ui-settings-page` and `@ui-setting-parts` (vite.config.ts) swap the page shell and the setting group/row/switch/select/alert; sections, hooks and save/restart rules in `components/setting/` are shared, Linux keeps the existing page and parts. Popups the sections open portal into the page's `.settings-popups` host, and each skin's `settings.css` maps the shadcn tokens to its own |
| Error/trace redaction       | `src/observability/`                           | breadcrumbs, tracing, sensitive arg masking  |

## CONVENTIONS

- 设备管理中的加入空间与连接手机使用同一个 `Sheet` 的任务态（`variant="task"`），内部以 `TaskPanel` 和各平台原生风格的表单、选择列表呈现；不得嵌入旧 Dialog、标签页或另一层 Sheet。任务主操作由 `TaskPanel.actions` 持有，设备浏览时才显示 Sheet 的完成按钮。浏览器验收入口为 `e2e/device-tasks-browser.mjs`，配合 `UI_FIXTURE_ENTRY=e2e/fixtures/device-tasks.tsx` 的预览服务验证真实皮肤与脚本化 API。

- 修改快捷面板的布局、字体、间距、菜单或交互前，先读 `src/quick-panel/DESIGN.md`；改变规范值时同步更新该文档。

- Use alias imports `@/*` (configured in `tsconfig.json` and `vite.config.ts`).
- New or edited imports should use `@/*` alias when targeting `src/*`; avoid adding new relative traversals when alias is available.
- Keep import order strict: builtin -> external -> internal -> parent -> sibling -> index, no blank lines between groups.
- Prefer API wrappers in `src/api/*` over `commands` from `@/lib/ipc`; avoid calling the Wails runtime directly in pages/components.
- Keep route guards and page gating in `App.tsx`/layout layer, not duplicated in leaf components.
- All async settings mutations (`update*`, `save*`, `mutate*`) must handle rejection (`try/catch` or `.catch`) with observable logging/feedback; no silent failures.
- Within a component, similar mutations should use a consistent error-handling wrapper pattern.
- Entry pages (setup, startup, recovery, unlock): state, validation and actions live in shared hooks (`pages/setup/*`, `pages/unlock/*`, `pages/recovery/*`, `components/app/use*Actions.ts`); the existing (legacy) pages, which only a platform-less build such as vitest still reaches, and the kit pages in `pages/entry-kit/` are two views over the same hooks. Callers import them through the `@ui-*` aliases in `vite.config.ts`/`tsconfig.json` and never branch on the OS.
- 首屏：macOS/Windows/Linux 的静态 HTML 由 `scripts/startup-splash.tsx` 在构建时复用现有皮肤生成（macOS 与 Linux 另内联皮肤 CSS，见 `vite.config.ts` 的 `skinCssFiles`）；无平台构建保留原有 HTML。使用 `e2e/startup-first-frame-browser.mjs --platform macos|windows|linux|none --out <dir>` 检查 HTML 到 React 的交接。`startup-screen.tsx` 与 `pages/entry-kit/StartupKitScreen.tsx` 只通过 `ui-kit/first-frame.ts` 导入 `@ui-skin/first-frame`，不得导入 `@/ui-kit`、Fluent barrel、bootstrap、`@/lib/ipc` 或 store。修改后运行 `node e2e/startup-chunk-check.mjs --platform macos|windows|linux|none`；皮肤构建的预算为 300.4 KiB 基线加 64 KiB gzip，无平台构建加 4 KiB。构建时的服务器渲染模块不得进入浏览器首屏。
- Kit entry pages keep the stable ids / test ids the E2E driver uses (`#unlock-passphrase`, `#recovery-passphrase`, `#device-name`, `#pass1`, `#pass2`, `#join-pass`, `setup-*`, `unlock-content`); `FormField`/`CodeField` accept an `id` for that. Browser checks: `e2e/app-entry-kit-browser.mjs --platform macos|windows`.
- Frontend tests use Vitest (`jsdom`) with setup file `src/test/setup.ts`; colocate tests in `__tests__`.

## ANTI-PATTERNS

- Calling host commands directly from deeply nested UI without API wrapper.
- Introducing fixed px layout values when Tailwind utilities/rem are available.
- Creating parallel state sources (local component cache + Redux) for same domain data.
- Logging sensitive payloads before redaction in observability paths.
- Adding new backend-facing types without matching runtime payload shape used in `src/api/*`.

## COMMANDS

```bash
# from repo root
bun run dev
bun run build
bun run test
bun run lint
```
