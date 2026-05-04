# Phase 91: SQLite Index Adapter and Rebuild Strategy - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-04-11
**Phase:** 91-SQLite Index Adapter and Rebuild Strategy
**Areas discussed:** Manual rebuild availability, rebuild-window delete consistency, initial backfill behavior

---

## Manual Rebuild Availability

| Option | Description | Selected |
|--------|-------------|----------|
| Rebuild-period blocked | Stop search for the whole rebuild window so users never see stale pre-rebuild results | ✓ |
| Keep serving old results | Continue returning the current index until cutover finishes | |
| the agent decides | Leave the trade-off to planning/implementation | |

**User's choice:** Rebuild-period blocked
**Notes:** Truthful blocked state is preferred over continued access to possibly stale results, even for manual same-version rebuilds.

---

## Rebuild-Window Delete Consistency

| Option | Description | Selected |
|--------|-------------|----------|
| Mirror deletes immediately | Apply delete to both active and temp index data during rebuild | ✓ |
| Allow temporary lag | Let deleted entries disappear only after a later rebuild/self-heal pass | |
| the agent decides | Leave the trade-off to planning/implementation | |

**User's choice:** Mirror deletes immediately
**Notes:** Rebuild cutover must not resurrect content that the user deleted while rebuild was running.

---

## Initial Backfill Behavior

| Option | Description | Selected |
|--------|-------------|----------|
| Auto-backfill after unlock | Trigger a rebuild automatically when old clipboard history exists but the search index is not ready yet | ✓ |
| Manual rebuild only | Require the user to explicitly trigger the first full rebuild | |
| the agent decides | Leave the trade-off to planning/implementation | |

**User's choice:** Auto-backfill after unlock
**Notes:** First-time search should aim to cover existing history without the user needing to discover a manual rebuild action.

---

## the agent's Discretion

- Exact temp-table naming and transaction structure
- Exact `UPSERT` / replace clauses used for active and temp writes
- Exact rebuild progress emission cadence
- Exact integration-test harness design

## Deferred Ideas

- None

## Reviewed Todos

- `修复 setup 配对确认提示缺失` — reviewed during discuss-phase and not folded because it is outside Phase 91 scope.
