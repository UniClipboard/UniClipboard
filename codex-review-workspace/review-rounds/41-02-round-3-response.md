# Codex Review Round 3 - Plan 41-02

**Model:** gpt-5.4
**Tokens used:** 29,370

## Findings

FINDING-1: CRITICAL [41-02-PLAN.md:46] [41-02-PLAN.md:203] [41-02-PLAN.md:234] This round's body correctly splits RPC startup into `run_rpc_accept_loop(listener, ...)` and moves `UnixListener::bind()` to `DaemonApp::run()`, but `key_links`, Task 2 import, and Task 1 `acceptance_criteria` still reference the old `run_rpc_server` and require `server.rs` to contain `UnixListener::bind`. This pulls the executor and acceptance criteria back to the pre-fix design from round 2. SUGGESTION: Unify the entire plan to `run_rpc_accept_loop`, remove all residual `run_rpc_server` references; move the `UnixListener::bind` acceptance point from `server.rs` to `app.rs`, and explicitly require "stale-check/bind before starting workers".

FINDING-2: MAJOR [41-02-PLAN.md:91] [41-02-PLAN.md:287] The interfaces section correctly declares `RuntimeState::new(initial_statuses: Vec<WorkerStatus>)` as pure snapshot construction, but Task 2 still writes `RuntimeState::new(&workers)` claiming "snapshot from Arc refs". This conflicts with the snapshot-only fix accepted in round 2 and re-ties `RuntimeState` initialization to worker trait objects. SUGGESTION: Change to first mapping `workers` into `Vec<WorkerStatus>` (e.g., from `name()` and `health_check()`), then calling `RuntimeState::new(initial_statuses)`.

FINDING-3: MAJOR [41-02-PLAN.md:296] [41-02-PLAN.md:300] [41-02-PLAN.md:328] The shutdown phase `JoinSet` ownership is still self-contradictory: Task 2 step 4 says the app-level `JoinSet` tracks worker spawns, step 8 says it drains connection handlers; the later code block places the connection handler `JoinSet` inside the RPC server. Under the current description, `DaemonApp::run()` can neither access the connection task set nor does it `await` the accept-loop's `JoinHandle`, so "graceful shutdown with in-flight RPC completion" remains incompletely defined. SUGGESTION: Define a single ownership scheme: either keep the connection `JoinSet` entirely inside `run_rpc_accept_loop()` and have `DaemonApp` `await` the server task's `JoinHandle` after cancel; or have the RPC layer return an awaitable/closeable handle for `DaemonApp` to unify teardown. Do not declare the same drain responsibility in both the app and server layers.

## Verdict

VERDICT: NEEDS_REVISION
