# FRONTEND (SRC)

Follow root rules in `AGENTS.md`. This file adds frontend-only guidance.

## OVERVIEW

React 19 + TypeScript + Vite UI layer for desktop app flows (setup, unlock, dashboard, devices, settings). Since ADR-008 the UI is primarily a client of the standalone `uniclipd` daemon over loopback HTTP + WebSocket (`src/api/daemon/`, generated SDK in `src/api/generated/`, realtime via `src/lib/daemon-ws.ts`); a shrinking set of native-only operations still go through Tauri `invoke` (`src/api/` + `src/lib/tauri-command.ts`).

## STRUCTURE

```text
src/
|- main.tsx            # frontend entry, Sentry/init logging
|- App.tsx             # router + providers + setup/encryption gating
|- api/                # backend wrappers; api/daemon/ (HTTP) + api/generated/ (hey-api SDK) + residual Tauri invoke
|- lib/tauri-command.ts# invokeWithTrace helper (native invoke path)
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
| Native Tauri command calls  | `src/api/` + `src/lib/tauri-command.ts`        | residual native ops; prefer `invokeWithTrace` over raw invoke |
| Global state                | `src/store/`                                   | `store/api.ts` + slices + hooks              |
| Setup flow UI               | `src/pages/SetupPage.tsx` + `src/pages/setup/` | multi-step onboarding/join flows             |
| Shared UI primitives        | `src/components/ui/`                           | Radix/shadcn-style components                |
| Error/trace redaction       | `src/observability/`                           | breadcrumbs, tracing, sensitive arg masking  |

## CONVENTIONS

- Use alias imports `@/*` (configured in `tsconfig.json` and `vite.config.ts`).
- New or edited imports should use `@/*` alias when targeting `src/*`; avoid adding new relative traversals when alias is available.
- Keep import order strict: builtin -> external -> internal -> parent -> sibling -> index, no blank lines between groups.
- Prefer API wrappers in `src/api/*` + `invokeWithTrace`; avoid ad-hoc direct `invoke()` in pages/components.
- Keep route guards and page gating in `App.tsx`/layout layer, not duplicated in leaf components.
- All async settings mutations (`update*`, `save*`, `mutate*`) must handle rejection (`try/catch` or `.catch`) with observable logging/feedback; no silent failures.
- Within a component, similar mutations should use a consistent error-handling wrapper pattern.
- Frontend tests use Vitest (`jsdom`) with setup file `src/test/setup.ts`; colocate tests in `__tests__`.

## ANTI-PATTERNS

- Calling Tauri commands directly from deeply nested UI without API wrapper.
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
