# 41-02 Round 5 (FINAL) — Codex Review Response

## Findings

FINDING-1: MAJOR [41-02-PLAN.md, Task 2 Step 2 vs Step 5] Plan contains self-contradictory `run_rpc_accept_loop` spawn sequence. Step 2 already requires `tokio::spawn(run_rpc_accept_loop(listener, ...))` immediately after bind, but Step 5 shows `let rpc_handle = tokio::spawn(run_rpc_accept_loop(listener, ...));` again. The same `listener` cannot be moved into two separate spawns; implementors will hit "use of moved value" or ambiguity about which spawn is canonical. SUGGESTION: Create `rpc_handle` once at bind time, reuse in the subsequent `select!` and shutdown drain.

FINDING-2: MAJOR [41-02-PLAN.md, Task 2 Step 4] Worker task lifecycle is not closed. Step 4 creates `worker_tasks: JoinSet<_>` and spawns all `worker.start(...)`, but subsequent steps never await, drain, or timeout-reclaim these tasks. Worker startup failures/panics are unobservable. Shutdown only calls `worker.stop()` without ensuring the corresponding task has exited, inconsistent with Phase 41 CONTEXT.md's "reuse TaskRegistry / cooperative shutdown pattern" requirement. SUGGESTION: Either include `worker_tasks` in the `select!`/shutdown drain (join+timeout like the RPC loop), or reuse the existing `TaskRegistry` pattern instead of leaving unmanaged background tasks.

## Verdict

VERDICT: NEEDS_REVISION
