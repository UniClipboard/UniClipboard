# KNOWLEDGE.md — Project Patterns and Lessons Learned

> Append-only. Lessons that save future agents from repeating investigation.

---

## Rust pub(crate) visibility does not cross crate boundaries

**Pattern:** When a function is declared `pub(crate)` in module `foo` of crate `A`, it is NOT accessible from crate `B` that depends on `A`. `pub(crate)` means "public within crate A" — crate B is a different crate.

**Lesson:** If uc-daemon needs a utility from uc-app, and it's only `pub(crate)`, either:

1. Make it `pub` in uc-app (if appropriate), or
2. Re-implement inline in uc-daemon (chosen in D001)

**Seen in:** M002-zldd9y / S03 — `uc-app/usecases/storage::dir_size` was `pub(crate)`, so `compute_dir_size()` was re-implemented in `storage.rs`.

---

## L4 destructive operation pattern: JsonRejection + explicit false check

**Pattern:** When an HTTP endpoint requires explicit user confirmation for a destructive action:

```rust
// 1. JsonRejection catches missing body
let Json(req) = body_result else {
    return (StatusCode::BAD_REQUEST, Json(confirmation_error())).into_response();
};

// 2. Explicit false check
if !req.confirmed {
    return (StatusCode::BAD_REQUEST, Json(confirmation_error())).into_response();
}
```

**Lesson:** Both missing body (JsonRejection) and explicit `confirmed: false` must return 400. JsonRejection alone only catches missing/malformed JSON — a body of `{}` would parse as valid JSON but have no `confirmed` field.

**Seen in:** M002-zldd9y / S03 — `POST /storage/clear-cache`.

---

## Tauri one-shot event bootstrap over invoke for one-time config delivery

**Pattern:** When the frontend needs a one-time config payload from the Rust side (e.g., daemon connection info), use a Tauri `emit()` + frontend `listen()` event pair instead of `invoke()`.

```typescript
// Frontend: listen for one-shot event
function waitForConnectionEvent(): Promise<Payload> {
  return new Promise((resolve) => {
    listen<Payload>('daemon://connection-info', (event) => {
      resolve(event.payload)
    })
  })
}
```

**Lesson:** `invoke()` requires a matching Tauri command in Rust (registered with `#[tauri::command]`). If no such command exists, an event-driven approach works without requiring a Rust change. Events are one-shot (auto-unsubscribe after first receipt) and are the correct primitive for "here is your initial config" messages.

**Seen in:** M003-fbgash / S01 / T01 — `daemon://connection-info` event carries `{ baseUrl, wsUrl, token }`. The plan assumed `invoke('daemon_connect_info')` but that command doesn't exist.

---

## Rust serde field naming is inconsistent between endpoints — always match Rust source

**Pattern:** Different Rust endpoints use different `#[serde(rename_all = "...")]` conventions. TypeScript must match the actual serialization, not assume a consistent convention.

| Module | Rust convention | TS field names |
|---|---|---|
| `/settings/*` | Default (snake_case) | `sync_settings.enabled` |
| `/encryption/*` | `rename_all = "camelCase"` | `sessionReady` |

**Lesson:** Always read the actual Rust `serde` attributes in the handler before writing TypeScript types. Never assume snake_case or camelCase without checking. A mismatch causes silent `undefined` values at runtime.

**Seen in:** M003-fbgash / S01 / T05 (settings — snake_case) and T06 (encryption — camelCase).

---

## globalThis.process is not accessible in all JS environments — use type assertion

**Pattern:** Accessing `globalThis.process` in a webview (non-Node context) causes TypeScript errors (`globalThis` has no index signature).

```typescript
// ❌ Type error: 'process' doesn't exist on type 'typeof globalThis'
pid: globalThis.process?.pid ?? 0

// ✅ Type assertion
pid: (globalThis as unknown as { process?: { pid?: number } }).process?.pid ?? 0
```

**Lesson:** In Tauri webviews, `globalThis` is the browser's `Window` object, not Node's `global`. The `process` object may be shimmed but TypeScript's `lib.es5.d.ts` doesn't know about it. Use a type assertion to access it safely.

**Seen in:** M003-fbgash / S01 / T04 — `daemon-auth.ts` PID field.

---

## Known issue: daemon 401 invalid_session_token on dev startup

**Symptom:** After `tauri:dev` launches, the frontend console floods with:

```text
Failed to initialize setup realtime store:
daemon setup request /setup/state failed with status 401 Unauthorized: {"error":"invalid_session_token"}
```

The `setupRealtimeStore.ts` retry loop (line ~106) retries indefinitely, generating noise.

**Root cause:** Not yet diagnosed. The session token the frontend sends to the daemon does not match what the daemon expects. Likely a race condition or stale token during dev startup — the frontend may attempt `/setup/state` before the daemon has registered the token emitted via `daemon://connection-info`.

**Impact:** The app cannot proceed past setup initialization. All daemon-dependent features (clipboard list, stats, etc.) are unreachable until this is resolved.

**Where to investigate:**
- `src/store/setupRealtimeStore.ts` — retry loop and token source
- `src/api/daemon/client.ts` — how the session token is attached to requests
- `src/lib/daemon-auth.ts` — token acquisition from Tauri event
- Rust side: daemon session token generation and validation logic

**Seen in:** M003-fbgash / S02 / T03 — observed during browser smoke test attempt.

---

## Daemon clipboard endpoints return preview projections only — full content still via Tauri

**Pattern:** GET /clipboard/entries returns EntryProjectionDto (preview data: text preview, size, timestamps, thumbnails, link metadata). The full clipboard entry detail (decrypted text, full content, resource metadata) is still served by the Tauri command `get_clipboard_entry_detail`.

**Lesson:** ClipboardSlice transforms daemon DTOs into ClipboardItemResponse using a local `transformDtoToItemResponse()` helper. This keeps the slice independent of clipboardItems.ts. The old clipboardItems.ts is retained only for type/enum imports — no function calls. When the daemon gains a full content endpoint, the Tauri path in clipboardItems.ts can be replaced without touching clipboardSlice.

**Seen in:** M003-fbgash / S02 / T01 — `src/api/daemon/clipboard.ts` and `src/store/slices/clipboardSlice.ts`.

---

## Grep audit for invoke() must be scoped to migrated files only

**Pattern:** When migrating from Tauri invoke() to daemon HTTP, the old Tauri module (clipboardItems.ts) intentionally retains invoke() calls for backward compatibility — types/enums are still imported from it. Broader `rg 'invoke' src/` will always find matches in the old module.

**Lesson:** The migration audit grep must be scoped to `src/store/slices/ src/api/daemon/` (the migrated layer) and exclude `src/api/clipboardItems.ts`. The T03 task plan correctly scoped it this way.

**Seen in:** M003-fbgash / S02 / T03 — audit confirmed zero invoke() clipboard calls in migrated files.

---

## bun test lacks vitest mocking APIs — React hook tests need npx vitest

**Pattern:** bun test does not expose `vi.stubGlobal`, `vi.advanceTimersByTimeAsync`, `vi.mocked`, `vi.importActual`, or jsdom DOM environment. Tests for React hooks and WebSocket clients that need mocking must run under `npx vitest` instead.

**Lesson:** If a test file uses `vi.fn()`, `vi.mock()`, `vi.useFakeTimers()`, or needs a DOM environment (jsdom), it must be run with `npx vitest run` — not `bun test`. The existing `daemon-ws.test.ts` (17 tests) and `useUINavigateListener.test.tsx` also fail under bun test for the same reasons. Prefer `npx vitest run` for all frontend unit tests going forward.

**Seen in:** M003-fbgash / S03 / T01 (daemon-ws.test.ts — 13 of 17 tests fail) and T02 (useDaemonEvents.test.ts — requires npx vitest with jsdom).

---

## WebSocket factory injection via constructor for testability

**Pattern:** `DaemonWsClient` accepts a `_wsFactory` constructor parameter (defaults to global `WebSocket`). Tests pass a mock factory that returns a mock WebSocket instance.

```typescript
// Production: uses native WebSocket
const daemonWs = new DaemonWsClient()

// Test: inject mock
const mockWs = new MockWebSocket()
const client = new DaemonWsClient((url) => mockWs)
```

**Lesson:** Global `WebSocket` cannot be stubbed in bun test. Constructor injection with a protected `_wsFactory` field enables tests to inject controlled mock objects. The `reset()` method clears singleton state between test cases.

**Seen in:** M003-fbgash / S03 / T01 — `daemon-ws.ts`.

---

## daemonWs.subscribe() auto-reconnects and re-subscribes — no manual reconnect listener needed

**Pattern:** `daemonWs` maintains `_activeTopics` internally and automatically re-subscribes all active topics on every reconnect. Callers do not need to listen for `daemon://ws-reconnected` to re-establish subscriptions.

**Lesson:** After a daemon restart or network blip, `daemonWs` handles reconnection and re-subscription transparently. Hooks that call `daemonWs.subscribe()` in `useEffect` will get fresh subscriptions automatically — the hook itself does not need to know about reconnection events. The `daemon://ws-reconnected` Tauri event and associated `useDaemonReconnectedListener` are no longer needed for WS subscription management.

**Seen in:** M003-fbgash / S03 / T03 — replaced `listen('daemon://ws-reconnected', ...)` with implicit re-subscription via `daemonWs`.

---

## clearClipboardItems has no daemon endpoint — falls back to Tauri invoke

**Pattern:** The `POST /clipboard/entries/clear` or similar endpoint does not exist in the daemon. The `clearAllItems` thunk in clipboardSlice falls back to the Tauri invoke path.

**Lesson:** When adding clipboard endpoints to the daemon, `POST /clipboard/entries/clear` should be included. The fallback in clipboardSlice.ts is marked with a `// TODO: Replace with daemon API once /clipboard/entries clear endpoint is available` comment.

**Seen in:** M003-fbgash / S02 / T02 — `src/store/slices/clipboardSlice.ts`, `clearAllItems` thunk.

---

## vi.spyOn cannot intercept ES module-level fetch captures — use vi.mock with module-level state

**Pattern:** When testing code that imports a module-level singleton (e.g., `daemonClient` in `@/api/daemon/client`) which captures `fetch` at module scope, `vi.spyOn(globalThis, 'fetch')` will not work — it only intercepts calls made after the spy is installed.

```typescript
// ❌ vi.spyOn cannot intercept module-level fetch capture
vi.spyOn(globalThis, 'fetch').mockResolvedValueOnce(mockResponse);
import { daemonClient } from '@/api/daemon/client'; // captures fetch HERE
await daemonClient.request(); // uses captured fetch, NOT the spy

// ✅ vi.mock replaces the module, bypassing the capture
const sharedState = { capturedRequests: [] };
vi.mock('@/api/daemon/client', () => ({
  daemonClient: {
    request: (opts) => { sharedState.capturedRequests.push(opts); return mockResponse; }
  }
}));
import { daemonClient } from '@/api/daemon/client'; // gets mock
await daemonClient.request();
assert(sharedState.capturedRequests.length > 0);
```

**Lesson:** Module-level captures happen at import time, before test setup runs. `vi.mock` replaces the module at the source — it never had the original `fetch` captured. The `vi.mock` factory can share state with the test via closure (module-level `sharedState` object).

**Seen in:** M003-fbgash / S05 / T03 — `daemon-client.test.ts` and `daemon-auth.test.ts`.

---

## Fake timers (vi.useFakeTimers) conflict with EventTarget — use per-test, not globally

**Pattern:** `vi.useFakeTimers()` globally replaces `EventTarget`, which breaks any class that extends `EventTarget` (including `MockWebSocket`).

```typescript
// ❌ Global fake timers — breaks EventTarget subclasses
beforeAll(() => vi.useFakeTimers()); // EventTarget is now broken
afterAll(() => vi.useRealTimers());

// ✅ Per-test fake timers — only affects the reconnect test
test('reconnect uses exponential backoff', () => {
  vi.useFakeTimers(); // scoped here
  // ... test code
  vi.useRealTimers(); // restore immediately
});
```

**Lesson:** Use `vi.useFakeTimers()` per individual test that needs timer control, not in `beforeAll`. Always call `vi.useRealTimers()` in `afterEach` or at the end of the test to restore. The `MockWebSocket` class must extend `EventTarget` to fire real `message` and `open` events.

**Seen in:** M003-fbgash / S05 / T02 — `daemon-ws.test.ts`.

---

## Current log files live under app.uniclipboard.desktop*/logs, not the older uniclipboard path

**Pattern:** The current app directory naming is resolved in `src-tauri/crates/uc-platform/src/app_dirs.rs` with `APP_DIR_NAME = "app.uniclipboard.desktop"`, optionally suffixed by `UC_PROFILE` (for example `app.uniclipboard.desktop-dev`). `src-tauri/crates/uc-app/src/app_paths.rs` then defines `logs_dir` as `{app_data_root}/logs`.

**Lesson:** When looking for desktop app logs on macOS, check:

- `~/Library/Application Support/app.uniclipboard.desktop/logs/`
- `~/Library/Application Support/app.uniclipboard.desktop-<profile>/logs/`

Do not assume the older `~/Library/Application Support/uniclipboard/logs/` path is current. That older directory may still exist from legacy builds, but current code resolves logs under the `app.uniclipboard.desktop*` root. On this machine, both `app.uniclipboard.desktop/logs` and `app.uniclipboard.desktop-dev/logs` exist and contain `uniclipboard.json.YYYY-MM-DD` files.

**Seen in:** 2026-04-11 repository audit — verified against `uc-platform::app_dirs`, `uc-app::AppPaths`, and local filesystem.

---

## vi.mock for Tauri events must be in the test file itself (hoisting requirement)

**Pattern:** When mocking Tauri events in Vitest, the `vi.mock('@tauri-apps/api/event', ...)` call must be in the test file itself — not in a helper module — because Vitest hoists `vi.mock` calls to the top of the file before any imports.

```typescript
// ❌ Mock in helper module — hoisting doesn't reach helper imports
// helpers/tauri-mock.ts
export const listeners = new Map();
vi.mock('@tauri-apps/api/event', () => ({
  listen: (name, cb) => { listeners.set(name, cb); return Promise.resolve({}); }
}));

// ❌ Still broken — helper module already imported before vi.mock runs
import { listeners } from './helpers/tauri-mock'; // loaded too early

// ✅ Mock in test file — hoisting works correctly
const listeners = new Map(); // shared closure
vi.mock('@tauri-apps/api/event', () => ({
  listen: (name, cb) => { listeners.set(name, cb); return Promise.resolve({}); }
}));
// Now both the mock AND the shared `listeners` map are in the same hoisted scope
```

**Lesson:** Vitest hoisting applies only within a single test file. Imports from helper modules are resolved before hoisting happens. Both the mock and the shared state must be in the same test file (or both in the same hoisted block). Alternatively, the Tauri event module can be passed as a dependency to the code under test (like `DaemonWsClient._wsFactory`).

**Seen in:** M003-fbgash / S05 / T03 — `daemon-auth.test.ts`.

---

## Math.random mocking for deterministic exponential backoff testing

**Pattern:** Exponential backoff uses `Math.random()` to add jitter to delays. For deterministic test results, mock `Math.random` to a fixed value.

```typescript
test('exponential backoff delay is calculated correctly', () => {
  vi.useFakeTimers();
  // mock Math.random = 0.5 → baseDelay * (0.5 + 0.5) = baseDelay * 1.0
  const randomSpy = vi.spyOn(Math, 'random').mockReturnValue(0.5);

  const ws = new DaemonWsClient(mockWsFactory);
  ws.connect('ws://localhost');
  mockWsFactory().close(); // trigger reconnect

  // With Math.random = 0.5 and baseDelay = 1000: delay = 1000 * 2^attempt * (0.5 + 0.5) = 2000
  vi.advanceTimersByTime(2000); // exactly enough to trigger reconnect

  vi.useRealTimers();
  randomSpy.mockRestore();
});
```

**Lesson:** Without mocking `Math.random`, backoff delays are unpredictable. Mock to a fixed value to get deterministic delay calculations. Always `mockRestore()` after the test.

**Seen in:** M003-fbgash / S05 / T02 — `daemon-ws.test.ts`.

---

## Storage API module did not exist in daemon client — created for test coverage

**Pattern:** When the test plan references an API module that does not exist in the codebase, create the module following the established pattern rather than skipping the test.

```typescript
// src/api/daemon/storage.ts — created to satisfy test plan
// Follows the same GET / POST pattern as clipboard.ts, settings.ts, encryption.ts
export async function getStorageStats(): Promise<StorageStats> {
  return daemonClient.request({ method: 'GET', path: '/storage/stats' });
}

export async function clearCache(confirmed: boolean): Promise<void> {
  if (!confirmed) throw new Error('CONFIRMATION_REQUIRED');
  await daemonClient.request({ method: 'POST', path: '/storage/clear-cache', body: { confirmed: true } });
}
```

**Lesson:** Test coverage requirements drove the creation of a missing API module. The module should be created in the same PR as the tests. Add it to `src/api/daemon/index.ts` for clean exports.

**Seen in:** M003-fbgash / S05 / T01 — `src/api/daemon/storage.ts` created.

---

## clipboardItems.ts retains Tauri invoke calls for native clipboard operations — not a bug

**Pattern:** After clipboardSlice.ts migrates to the daemon HTTP client, `clipboardItems.ts` still contains invoke calls for operations that require native OS clipboard integration (`restore_clipboard_entry`, `copy_file_to_clipboard`, `download_file_entry`, `open_file_location`, `clear_clipboard_items`). These are on the explicit Tauri allowlist (D005) and are NOT a migration gap.

**Lesson:** The grep audit in T03 (`rg invokeWithTrace for get_storage_stats, clear_cache, get_encryption_session_status`) was scoped to the storage/settings/encryption areas being migrated. Clipboard invokes in `clipboardItems.ts` were already handled in T02 when clipboardSlice.ts was migrated. The module retains invoke calls because: (a) native clipboard operations cannot go through daemon HTTP, and (b) `clipboardItems.ts` is now a types-and-native-utility module with zero function calls in the migrated business layer. The grep pattern `get_clipboard_` (with underscore) would not match `get_clipboard_entries` (no underscore) anyway.

**Audit approach:** To verify zero clipboard business invokes remain, scope grep to `src/store/slices/ src/api/daemon/ src/hooks/ src/components/` — not `clipboardItems.ts`. The allowlist in `src/api/storage.ts` documents the one remaining Tauri invoke (`open_data_directory`) with a bilingual comment.

**Seen in:** M003-fbgash / S06 / T02, T03 — `src/api/clipboardItems.ts`, `src/api/storage.ts`.

---

## Auth-first bootstrap: session refresh is a hard prerequisite before WebSocket connect

**Pattern:** The frontend bootstrap must call `daemonClient.refreshSession()` and wait for it to resolve before calling `daemonWs.connect()`. The sequence is:

```typescript
// ✅ Correct bootstrap order
await daemonClient.initialize(config);  // sets up HTTP client
await daemonClient.refreshSession();    // exchanges bearer → JWT session
await daemonWs.connect(wsUrl);         // now authenticated

// ❌ Wrong — race condition, causes invalid_session_token churn
await daemonClient.initialize(config);
await daemonWs.connect(wsUrl);         // daemon sees raw bearer, rejects
```

**Lesson:** `daemonClient.initialize()` only sets up the HTTP client with the bearer token. It does NOT exchange the bearer for a JWT session token. The session token is what the daemon's WebSocket handler validates. Without `refreshSession()` in between, the WS connection receives the unauthenticated bearer and the daemon logs `WS JWT validation failed`. The `invalid_session_token` console flood on app startup is the symptom of this ordering bug.

**Seen in:** M003-fbgash / S07 / T02 — `src/lib/daemon-ws-bootstrap.ts`. Root cause of the known issue documented in M003 S02.

---

## validatePayload() with TypeScript asserts guards bootstrap events before client init

**Pattern:** When the frontend listens for a one-shot Tauri event (e.g., `daemon://connection-info`) that configures a singleton, validate the payload shape before using it to initialize clients:

```typescript
function validatePayload(payload: unknown): asserts payload is DaemonConnectionPayload {
  if (!payload || typeof payload !== 'object') {
    throw new Error('Bootstrap: invalid payload — not an object');
  }
  const p = payload as Record<string, unknown>;
  if (typeof p.baseUrl !== 'string' || !p.baseUrl.startsWith('http')) {
    throw new Error('Bootstrap: invalid baseUrl in payload');
  }
  if (typeof p.token !== 'string' || !p.token.startsWith('Bearer ')) {
    throw new Error('Bootstrap: invalid token in payload');
  }
  // ...
}
```

**Lesson:** `daemon://connection-info` is emitted by the Rust side before the WebView is fully initialized. A malformed payload — or a payload emitted by a different Tauri event with a coincidentally similar name — could initialize `daemonClient` with bad config. `validatePayload()` with TypeScript's `asserts` keyword (or a `Result` type) catches bad shapes early with high-signal diagnostics, before any HTTP or WS calls are made. Without it, failures surface as cryptic network errors downstream.

**Seen in:** M003-fbgash / S07 / T02 — `src/lib/daemon-ws-bootstrap.ts`.

---

## vitest module isolation: module-level `let` state persists across test suites

**Pattern:** In vitest, `let` variables declared at module scope in a test file persist across test suites. If a test sets `moduleLevelVar = true` and another suite expects `moduleLevelVar === false`, the state leaks.

```typescript
// ❌ Module-level state persists across test suites
let connectionEstablished = false;

async function connectDaemonWs(payload: DaemonConnectionPayload) {
  if (connectionEstablished) return; // stale state from previous suite
  await daemonClient.initialize(payload);
  await daemonClient.refreshSession();
  await daemonWs.connect(payload.wsUrl);
  connectionEstablished = true;
}

// In test suite A:
test('first call', async () => {
  await connectDaemonWs(payload);
  expect(connectionEstablished).toBe(true);
});

// In test suite B (runs after A — connectionEstablished is still true!):
test('idempotency: second call does not reconnect', async () => {
  // connectionEstablished === true from suite A!
  await connectDaemonWs(payload);
  expect(daemonWs.connect).not.toHaveBeenCalled(); // FAILS
});
```

**Lesson:** Vitest runs each test file as a separate module, but if the test file uses a module-level `let` variable that accumulates state during test execution, subsequent tests within the same file that expect the initial state will fail. Use `beforeEach` to reset state, `vi.resetModules()` to clear between isolation boundaries, or move mutable state into a singleton whose `reset()` method is called in `beforeEach`. The `DaemonWsClient` singleton pattern (with a `reset()` method) works for state inside the class — but module-level variables outside the class also need explicit reset.

**Seen in:** M003-fbgash / S07 / T02 — `daemon-ws-bootstrap.test.ts` (2 idempotency tests fail due to `connectionEstablished` flag persisting across test suites).

---

## Cross-crate DTO conversions cannot use foreign `From` impls; enum string rules must be centralized

**Pattern:** If a crate re-exports DTO types from another crate, those DTOs are still foreign types in the current crate. Writing `impl From<ExternalA> for ExternalB` is invalid when both types are foreign and the trait (`From`) is also foreign. Re-export does not change orphan-rule ownership.

```rust
// ❌ Invalid in uc-daemon after DTOs moved to uc-daemon-contract
impl From<P2pPeerSnapshot> for PeerSnapshotDto { ... }
impl From<PairedDevice> for PairedDeviceDto { ... }
```

**Lesson:** Keep transport projection ownership in the transport crate via a local projection layer, not foreign `From` impls. For pure projections, a local trait is cleaner than spreading `*_from/*_to` helpers:

```rust
pub trait IntoApiDto<T> {
    fn into_api_dto(self) -> T;
}
```

If a string mapping for an enum appears in more than one crate, that is no longer a local helper — it is a shared business rule. Move it to the enum's owning crate with `Display` and `FromStr`, then replace duplicated helpers with `value.to_string()` and `Enum::from_str(raw)`.

**Anti-patterns found:**

- repeated `pairing_state_to_string`, `pairing_state_to_str`, and `pairing_state_from_str` helpers across `uc-app`, `uc-daemon`, and `uc-infra`
- temptation to replace invalid `From` impls with many mechanical helpers like `peer_snapshot_dto_from(...)`
- keeping the duplicated local helpers after the enum authority was identified

**Preferred handling:**

1. Put cross-crate transport mapping in a dedicated projection module owned by the boundary crate.
2. Use a local trait for pure projections (`IntoApiDto<T>`), and local mapper functions only when extra context is required.
3. Put stable enum string rules on the enum itself (`Display` / `FromStr`) in the owning crate.
4. Delete the old helper paths; do not keep dual conversion routes.

**Seen in:** 2026-04-10 — `uc-daemon` orphan-rule review on `api/types.rs`, followed by `PairingState` string-rule consolidation into `uc-core/src/network/paired_device.rs` and call-site cleanup in `uc-app`, `uc-daemon`, and `uc-infra`.
