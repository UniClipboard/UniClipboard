# Review Log: Phase 41 Plans

Started: 2026-03-18

## Round 1

### 41-01-PLAN — Codex Verdict: NEEDS_REVISION

| Finding | Severity | CC Decision    | Action                                                                                          |
| ------- | -------- | -------------- | ----------------------------------------------------------------------------------------------- |
| F-1     | CRITICAL | AGREE          | Forbid CoreRuntime modification; use SetupAssemblyPorts::placeholder + build_setup_orchestrator |
| F-2     | MAJOR    | PARTIAL ACCEPT | Documented get_storage_paths(&config) call path for CLI in plan                                 |
| F-3     | MAJOR    | AGREE          | Separated RuntimeState from worker ownership; RuntimeState is now snapshot-only                 |
| F-4     | MAJOR    | AGREE          | Added Task 3 with unit tests for non_gui_runtime, RuntimeState, RPC types                       |
| F-5     | MINOR    | REJECT         | Placeholder main.rs uses eprintln intentionally; Plan 02 replaces it entirely                   |

### 41-02-PLAN — Codex Verdict: NEEDS_REVISION

| Finding | Severity | CC Decision    | Action                                                                                  |
| ------- | -------- | -------------- | --------------------------------------------------------------------------------------- |
| F-1     | CRITICAL | AGREE          | Reordered: bind RPC socket before starting workers                                      |
| F-2     | CRITICAL | AGREE          | Changed to Vec<Arc<dyn DaemonWorker>>; worker ownership in DaemonApp not RuntimeState   |
| F-3     | MAJOR    | AGREE          | run_rpc_server accepts PathBuf (owned) instead of &Path                                 |
| F-4     | MAJOR    | PARTIAL ACCEPT | Stale socket check now sends ping RPC per CONTEXT.md locked decision                    |
| F-5     | MAJOR    | AGREE          | wait_for_shutdown_signal returns Result; no expect/unwrap                               |
| F-6     | MAJOR    | PARTIAL ACCEPT | Added JoinSet tracking for connection handlers with drain timeout                       |
| F-7     | MAJOR    | AGREE          | Added integration test requirement for ping RPC e2e                                     |
| F-8     | MAJOR    | REJECT         | Plan 01 Task 1 already creates LoggingHostEventEmitter; depends_on: [41-01] covers this |

### 41-03-PLAN — Codex Verdict: NEEDS_REVISION

| Finding | Severity | CC Decision    | Action                                                                                  |
| ------- | -------- | -------------- | --------------------------------------------------------------------------------------- |
| F-1     | CRITICAL | PARTIAL ACCEPT | Added note: status e2e depends on Plan 02; this plan validates compilation + exit codes |
| F-2     | CRITICAL | AGREE          | Fixed data_dir → app_data_root in resolve_socket_path()                                 |
| F-3     | MAJOR    | AGREE          | Added #[cfg(unix)] guards for all UnixStream usage with non-unix fallback               |
| F-4     | MAJOR    | AGREE          | Added CLI integration test requirements to verification section                         |
| F-5     | MAJOR    | AGREE          | main() now returns anyhow::Result<()>; no expect()                                      |
| F-6     | MINOR    | PARTIAL ACCEPT | Noted but not split in this revision; Task 1 stays bundled for new-crate bootstrap      |

### Plan diffs (Round 1 → Round 2):

**41-01-PLAN changes:**

- Task 1: SetupOrchestrator construction now mandates SetupAssemblyPorts::placeholder path; forbids CoreRuntime modification
- Task 1: Added note about CLI callers using get_storage_paths(&config)
- Task 2: RuntimeState redesigned as snapshot-only (no worker ownership); uses Arc refs
- Added Task 3: Unit tests for non_gui_runtime, RuntimeState, RPC types
- Success criteria updated with test requirements

**41-02-PLAN changes:**

- DaemonApp: workers now Vec<Arc<dyn DaemonWorker>>
- Startup order: RPC bind before worker start
- run_rpc_server: accepts PathBuf not &Path
- Stale socket check: sends actual ping RPC
- Signal handler: returns Result, no expect()
- JoinSet for connection handler tracking
- Fixed data_dir → app_data_root in main.rs
- Added integration test to verification/success criteria

**41-03-PLAN changes:**

- Fixed data_dir → app_data_root in resolve_socket_path()
- main() returns anyhow::Result<()>
- Added #[cfg(unix)] guard requirement for all Unix socket code
- Added status e2e dependency note on Plan 02
- Added CLI integration tests to verification
- Updated success criteria with platform guard + correct field name

## Round 2

### 41-01-PLAN — Codex Verdict: NEEDS_REVISION

| Finding | Severity | CC Decision | Action                                                                                              |
| ------- | -------- | ----------- | --------------------------------------------------------------------------------------------------- |
| F-1     | CRITICAL | AGREE       | Added full 7-param build_setup_orchestrator call with NoopSessionReadyEmitter + NoopWatcherControl  |
| F-2     | MAJOR    | AGREE       | RuntimeState now pure snapshot: Vec<WorkerStatus> only, no DaemonWorker refs. DaemonApp updates it. |
| F-3     | MAJOR    | AGREE       | build_non_gui_runtime test deferred to integration; removed from acceptance criteria                |
| F-4     | MINOR    | AGREE       | Added PartialEq derive to all RPC types                                                             |

### 41-02-PLAN — Codex Verdict: NEEDS_REVISION

| Finding | Severity | CC Decision    | Action                                                                                                                         |
| ------- | -------- | -------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| F-1     | CRITICAL | AGREE          | Split bind from spawn: DaemonApp::run() does check+bind, passes listener to accept loop                                        |
| F-2     | MAJOR    | AGREE          | Updated interfaces section: RuntimeState now snapshot-only, no workers()/workers_mut()                                         |
| F-3     | MAJOR    | PARTIAL ACCEPT | Narrowed objective: skeleton doesn't need CoreRuntime, deferred to real worker phase. Updated key_links + acceptance criteria. |

### 41-03-PLAN — Codex Verdict: NEEDS_REVISION

| Finding | Severity | CC Decision | Action                                                                                                             |
| ------- | -------- | ----------- | ------------------------------------------------------------------------------------------------------------------ |
| F-1     | MAJOR    | AGREE       | Moved platform dispatch into status::run(); resolve_socket_path() now in status module behind cfg(unix)            |
| F-2     | MAJOR    | REJECT      | execution_context absolute paths are GSD framework standard; resolved locally by executor, not a portability issue |
| F-3     | MINOR    | AGREE       | Fixed ls path to use cwd-relative target/debug/                                                                    |

### Plan diffs (Round 2 → Round 3):

**41-01-PLAN changes:**

- Task 1: Full 7-param build_setup_orchestrator call with NoopSessionReadyEmitter + NoopWatcherControl
- Task 2: RuntimeState is now Vec<WorkerStatus> pure snapshot + update_worker_statuses() method
- Task 2: All RPC types derive PartialEq for test assertions
- Task 3: Removed build_non_gui_runtime test (deferred to integration); adjusted acceptance criteria

**41-02-PLAN changes:**

- DaemonApp::run() does check_or_remove_stale_socket + UnixListener::bind BEFORE spawn
- run_rpc_server renamed to run_rpc_accept_loop, accepts already-bound UnixListener
- Interfaces section updated to match snapshot-only RuntimeState
- Objective narrowed: CoreRuntime integration deferred to real worker phase
- key_links and acceptance criteria updated to remove misleading build_non_gui_runtime ref

**41-03-PLAN changes:**

- status::run() now handles platform dispatch internally (resolve_socket_path moved there)
- main.rs no longer calls resolve_socket_path() directly
- Fixed ls path in acceptance criteria

## Round 3

### 41-01-PLAN — Codex Verdict: NEEDS_REVISION (2 findings)

| Finding | Severity          | CC Decision | Action                                                        |
| ------- | ----------------- | ----------- | ------------------------------------------------------------- |
| F-1     | MAJOR [RE-RAISED] | AGREE       | Removed eprintln from placeholder main.rs; now exits silently |
| F-2     | MINOR             | AGREE       | Added main.rs to files_modified metadata                      |

### 41-02-PLAN — Codex Verdict: NEEDS_REVISION (3 findings)

| Finding | Severity | CC Decision | Action                                                                                                |
| ------- | -------- | ----------- | ----------------------------------------------------------------------------------------------------- |
| F-1     | CRITICAL | AGREE       | Updated key_links, acceptance criteria to use run_rpc_accept_loop; moved UnixListener::bind to app.rs |
| F-2     | MAJOR    | AGREE       | Fixed RuntimeState::new call to use Vec<WorkerStatus> not &workers                                    |
| F-3     | MAJOR    | AGREE       | Clarified JoinSet lives in accept loop; DaemonApp awaits accept loop JoinHandle                       |

### 41-03-PLAN — Codex Verdict: NEEDS_REVISION (1 finding)

| Finding | Severity | CC Decision | Action                                                                    |
| ------- | -------- | ----------- | ------------------------------------------------------------------------- |
| F-1     | MAJOR    | AGREE       | Removed uc-daemon from verification section; Plan 03 only verifies uc-cli |

### Plan diffs (Round 3 → Round 4):

**41-01**: Removed eprintln from placeholder main.rs; added main.rs to files_modified
**41-02**: Fixed stale run_rpc_server→run_rpc_accept_loop refs; RuntimeState::new uses Vec<WorkerStatus>; JoinSet ownership clarified in accept loop
**41-03**: Removed uc-daemon from verification (not in depends_on)

## Round 4

### 41-01-PLAN — Codex Verdict: NEEDS_REVISION (3 findings)

| Finding | Severity | CC Decision | Action                                                               |
| ------- | -------- | ----------- | -------------------------------------------------------------------- |
| F-1     | MAJOR    | AGREE       | Fixed health_check wording to "async start/stop + sync health_check" |
| F-2     | MAJOR    | AGREE       | Added full workspace cargo check to verification                     |
| F-3     | MINOR    | AGREE       | Added main.rs to Task 2 files list                                   |

### 41-02-PLAN — Codex Verdict: NEEDS_REVISION (4 findings)

| Finding | Severity | CC Decision | Action                                                             |
| ------- | -------- | ----------- | ------------------------------------------------------------------ |
| F-1     | MAJOR    | AGREE       | Replaced all remaining run_rpc_server → run_rpc_accept_loop        |
| F-2     | MAJOR    | AGREE       | Added explicit Arc::clone + async move pattern for worker spawn    |
| F-3     | MAJOR    | AGREE       | Added tokio::select! over shutdown signal + accept loop JoinHandle |
| F-4     | MINOR    | AGREE       | Socket removal now logs non-NotFound errors instead of .ok()       |

### 41-03-PLAN — Codex Verdict: NEEDS_REVISION (2 findings)

| Finding | Severity | CC Decision | Action                                                       |
| ------- | -------- | ----------- | ------------------------------------------------------------ |
| F-1     | MAJOR    | AGREE       | Added Task 3: CLI smoke tests (cli_smoke.rs)                 |
| F-2     | MINOR    | AGREE       | print_result returns Result instead of calling process::exit |

### Plan diffs (Round 4 → Round 5):

**41-01**: Fixed health_check wording; added full workspace check; Task 2 files includes main.rs
**41-02**: All run_rpc_server→run_rpc_accept_loop; Arc::clone spawn pattern; select! for accept loop crash; socket removal logging
**41-03**: Added Task 3 smoke tests; print_result returns Result

## Round 5

### 41-01-PLAN — Codex Verdict: NEEDS_REVISION (2 findings)

| Finding | Severity | CC Decision | Action                                                                    |
| ------- | -------- | ----------- | ------------------------------------------------------------------------- |
| F-1     | CRITICAL | AGREE       | LoggingHostEventEmitter now logs only event_type string, no payload Debug |
| F-2     | MAJOR    | AGREE       | Removed pipe from Task 1 verify; uses echo exit:$?                        |

### 41-03-PLAN — Codex Verdict: NEEDS_REVISION (2 findings)

| Finding | Severity | CC Decision | Action                                                            |
| ------- | -------- | ----------- | ----------------------------------------------------------------- |
| F-1     | CRITICAL | AGREE       | Changed JSON verification to --json devices; status JSON deferred |
| F-2     | MAJOR    | AGREE       | Changed test filter to --test cli_smoke (not -- cli_smoke)        |

### Plan diffs (Round 5 → Round 6):

**41-01**: LoggingHostEventEmitter logs only event_type; Task 1 verify uses echo exit:$? without pipe
**41-03**: must_haves.truths + verification now use `--json devices`; Task 3 verify uses `--test cli_smoke`

## Round 6 (VERIFICATION)

### 41-01-PLAN — Codex Verdict: NEEDS_REVISION (1 finding)

| Finding | Severity | CC Decision | Action               |
| ------- | -------- | ----------- | -------------------- | ---------------------------------------- |
| F-1     | MAJOR    | AGREE       | Task 3 verify pipe ` | tail -10`replaced with`; echo "exit:$?"` |

### 41-03-PLAN — Codex Verdict: APPROVED

Both Round 5 fixes verified present. No new findings raised.

### Plan diffs (Round 6):

**41-01**: Task 3 verify command fixed: removed `| tail -10`, now uses `; echo "exit:$?"`

## 41-04-PLAN Round 1 — 2026-03-18

### Codex Verdict: NEEDS_REVISION

| Finding | Severity | CC Decision     | User Override | Action                                                                                                                                            |
| ------- | -------- | --------------- | ------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| F-1     | CRITICAL | PARTIALLY AGREE | —             | Changed fallback from `std::env::temp_dir()` to `/tmp` on Unix; added length guard with NUL terminator                                            |
| F-2     | CRITICAL | AGREE           | —             | Added length check after joining XDG_RUNTIME_DIR; fallback to `/tmp` if over limit                                                                |
| F-3     | MAJOR    | DISAGREE        | REJECT        | Kept uc-daemon dependency. Reason: creating a new crate for one 5-line function is over-engineering; uc-cli already needs to know daemon protocol |
| F-4     | MAJOR    | PARTIALLY AGREE | —             | Tests now check byte length + NUL terminator; added boundary and too-long XDG_RUNTIME_DIR test cases                                              |
| F-5     | MAJOR    | DISAGREE        | REJECT        | Out of scope. This plan is gap closure for SUN_LEN path fix only; SIGTERM handling already exists in daemon code                                  |
| F-6     | MINOR    | DISAGREE        | REJECT        | Absolute paths are required by GSD toolchain automation                                                                                           |

### Plan changes:

- Task 1 Unix fallback: `/tmp` instead of `std::env::temp_dir()`
- Task 1: Added length guard (path.as_os_str().len() + 1 < 104) with XDG fallback
- Task 1 tests: Updated to check byte length + NUL, added `test_xdg_runtime_dir_too_long` and `test_socket_path_boundary`

## 41-04-PLAN Round 2 — 2026-03-18

### Codex Verdict: NEEDS_REVISION

| Finding | Severity             | CC Decision | User Override | Action                                                                                                                      |
| ------- | -------------------- | ----------- | ------------- | --------------------------------------------------------------------------------------------------------------------------- |
| F-1     | CRITICAL [RE-RAISED] | AGREE       | —             | Fixed off-by-one: `len() + 1 < 104` → `len() <= 103`; use OsStrExt::as_bytes().len() on Unix                                |
| F-2     | MAJOR                | AGREE       | —             | Extract pure `resolve_daemon_socket_path_from(base: Option<&Path>)` for testability; tests use it directly, no env mutation |

### Plan changes:

- Length guard corrected to `path.as_os_str().len() <= 103`
- Added `resolve_daemon_socket_path_from()` pure function for testability
- All tests rewritten to use `_from()` variant — no global env var mutation, parallel-safe

## 41-04-PLAN Round 3 — 2026-03-18

### Codex Verdict: NEEDS_REVISION

| Finding | Severity | CC Decision | User Override | Action                                                                                                                                 |
| ------- | -------- | ----------- | ------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| F-1     | MAJOR    | AGREE       | —             | Unified all length checks to use `socket_path_byte_len()` helper (OsStrExt on Unix); updated length guard, tests, and success criteria |

### Plan changes:

- Added `socket_path_byte_len(path)` helper using `OsStrExt::as_bytes().len()` on Unix
- All length checks (guard + tests + success criteria) now use consistent `socket_path_byte_len() <= 103`

## 41-04-PLAN Round 4 — 2026-03-18

### Codex Verdict: NEEDS_REVISION

| Finding | Severity | CC Decision | User Override | Action                                                                                                                |
| ------- | -------- | ----------- | ------------- | --------------------------------------------------------------------------------------------------------------------- |
| F-1     | MAJOR    | AGREE       | —             | `resolve_daemon_socket_path_from` now treats `Some("")` same as `None`; public fn filters empty/whitespace XDG values |
| F-2     | MINOR    | AGREE       | —             | Test paths now use concrete constructions: `"a".repeat(90)` for too-long, precise `"x" * 76` for boundary             |

### Plan changes:

- Added empty-path guard in `resolve_daemon_socket_path_from`
- Added `test_xdg_runtime_dir_empty` test case
- Replaced placeholder test paths with concrete `"a".repeat(N)` constructions
- Boundary test now uses precise byte math: base = `"/" + "x" * 76` for 103-byte total

## 41-04-PLAN Round 5 — 2026-03-18

### Codex Verdict: NEEDS_REVISION

| Finding | Severity | CC Decision | User Override | Action                                                                                              |
| ------- | -------- | ----------- | ------------- | --------------------------------------------------------------------------------------------------- |
| F-1     | MAJOR    | AGREE       | —             | Fixed: socket name is 24 bytes not 26; boundary test now computes from `SOCKET_NAME.len()` constant |
| F-2     | MINOR    | AGREE       | —             | Added `test_xdg_runtime_dir_whitespace_only` test case                                              |

### Plan changes:

- Boundary test uses `const SOCKET_NAME` (24 bytes) for correct math: max_base_len = 103 - 1 - 24 = 78
- Added whitespace-only XDG test via `_from(Some(Path::new("   ")))`

## 41-04-PLAN Round 6 — 2026-03-18

### Codex Verdict: NEEDS_REVISION

| Finding | Severity | CC Decision | User Override | Action                                                                                                                                                      |
| ------- | -------- | ----------- | ------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| F-1     | MAJOR    | AGREE       | —             | Separated string-level sanitization from path-level resolution: `sanitize_xdg_runtime_dir(Option<&str>)` + `resolve_daemon_socket_path_from(Option<&Path>)` |

### Plan changes:

- Extracted `sanitize_xdg_runtime_dir()` for env var string cleaning (None/empty/whitespace → None)
- `resolve_daemon_socket_path_from()` now only handles path logic (no string trim)
- Tests split: 4 sanitize tests + 1 resolve_from_none test replace old mixed tests

## 41-04-PLAN Round 7 — 2026-03-18

### Codex Verdict: NEEDS_REVISION

| Finding | Severity | CC Decision | User Override | Action                                                                                               |
| ------- | -------- | ----------- | ------------- | ---------------------------------------------------------------------------------------------------- |
| F-1     | MAJOR    | AGREE       | —             | All Unix helpers and tests gated with `#[cfg(unix)]`; non-Unix path uses `std::env::temp_dir()` only |

### Plan changes:

- `sanitize_xdg_runtime_dir` and `resolve_daemon_socket_path_from` gated with `#[cfg(unix)]`
- Public `resolve_daemon_socket_path()` has separate `#[cfg(unix)]` and `#[cfg(not(unix))]` implementations
- Tests split: 1 cross-platform test + 9 Unix-only tests with `#[cfg(unix)]` guard
