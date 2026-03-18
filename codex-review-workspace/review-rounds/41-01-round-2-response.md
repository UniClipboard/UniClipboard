# Codex Review Round 2 — 41-01-PLAN

**Model:** gpt-5.4 via Codex v0.115.0
**Date:** 2026-03-18
**Session:** 019d0119-3ea6-7d83-8e9d-c821f44ef1c0

---

## Raw Findings

FINDING-1: CRITICAL
Task 1 gives a `build_setup_orchestrator(...)` call signature that does not match the real codebase. The actual `uc-bootstrap::build_setup_orchestrator` requires 7 parameters: `deps, ports, lifecycle_status, emitter_cell, clipboard_integration_mode, session_ready_emitter, watcher_control`; the plan only specifies the first 5, with no indication of where `session_ready_emitter` and `watcher_control` come from. An executor following the plan verbatim will fail at `cargo check -p uc-bootstrap`.
SUGGESTION: Update Task 1 to match the real function signature. Explicitly require constructing and passing `Arc<LoggingSessionReadyEmitter>` (or `NoopSessionReadyEmitter`) and a usable `Arc<dyn WatcherControlPort>`. If `build_non_gui_runtime()` needs to self-create watcher control, include the corresponding channel/adapter creation steps in the plan.

FINDING-2: MAJOR
Task 2's `RuntimeState` still holds `Vec<Arc<dyn DaemonWorker>>`, which contradicts the plan's repeated claim of "snapshot-only / does NOT own workers". `Arc` is shared ownership of a live worker, not a snapshot; `worker_statuses()` dynamically calls `health_check()`, meaning `RuntimeState` is still directly coupled to worker entities. The Round 1 accepted fix ("remove worker ownership") has not actually been implemented in the current revision.
SUGGESTION: Change `RuntimeState` to hold only snapshot data, e.g. `start_time + Arc<RwLock<Vec<WorkerStatus>>>` or plain `Vec<WorkerStatus>`. Have `DaemonApp` write snapshots on worker lifecycle changes. Remove all `DaemonWorker` references from `RuntimeState`.

FINDING-3: MAJOR
Task 3's test requirements are self-contradictory. The plan requires `test_build_non_gui_runtime_succeeds` but also says "if AppDeps fixture is too complex, add TODO and skip". Yet the acceptance criteria and success criteria require `cargo test -p uc-bootstrap -p uc-daemon` to pass and treat these unit tests as completion conditions. Without a clear, reproducible test fixture for `AppDeps + AppPaths`, executors face a deadlock: skip the test but fail acceptance, or attempt the test without a viable fixture path.
SUGGESTION: Choose one approach and lock it into the plan: either provide a reusable test fixture path (e.g. a test-specific wiring helper to generate `AppDeps`), or remove `build_non_gui_runtime_succeeds` from this plan's required unit tests (test only `LoggingHostEventEmitter` + compilation verification) and defer runtime construction testing to a later integration test.

FINDING-4: MINOR
Task 3 requires serde roundtrip tests for `RpcRequest` and `StatusResponse` with "assert equality", but Task 2's type definitions only derive `Debug, Serialize, Deserialize` — not `PartialEq`/`Eq`. The roundtrip tests as described cannot be written without additional derives or switching to field-level assertions.
SUGGESTION: Add `PartialEq`/`Eq` derive requirements for `RpcRequest`, `RpcResponse`, `RpcError`, `StatusResponse`, `WorkerStatus`, and `WorkerHealth` in the plan, or change the test requirements to use field-level / JSON structure assertions instead.

---

## VERDICT: NEEDS_REVISION — the issues above must be addressed
