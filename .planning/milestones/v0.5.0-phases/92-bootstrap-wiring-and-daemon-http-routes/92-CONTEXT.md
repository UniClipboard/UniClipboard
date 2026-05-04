# Phase 92: Bootstrap Wiring and Daemon HTTP Routes - Context

**Gathered:** 2026-04-11
**Status:** Ready for planning

<domain>
## Phase Boundary

Expose the already-built local search backend through daemon bootstrap, HTTP routes, and WebSocket events so search works end-to-end without UI work: unlocked clipboard capture can incrementally index new entries, `GET /search/query` can return filtered results, `POST /search/rebuild` can trigger a background rebuild, `GET /search/status` can report truthful availability, and all search routes enforce locked-session behavior. This phase does not add new search capabilities or any frontend UI.

</domain>

<decisions>
## Implementation Decisions

### Query Contract
- **D-01:** `GET /search/query` uses readable query-string parameters, not an encoded filter blob. Use explicit keys in the current daemon style for query/operator, time preset or absolute range, repeated `fileTypes` / `extensions`, and `limit` / `offset`.
- **D-02:** Successful query responses include the result rows plus `total` and `hasMore`. Phase 92 should lock the backend contract that Phase 93 needs for visible result counts and pagination awareness.
- **D-03:** Query failures keep the daemon-wide simple error envelope `{ code, message }`. Search-specific failures are distinguished by precise codes such as `invalid_query`, `session_locked`, and `index_not_ready`, not by extra hint/debug payloads.

### Status Contract
- **D-04:** `/search/status` returns product-oriented states as its primary contract, not raw `search_index_meta` fields. Clients should receive states they can render directly, such as ready, locked, and rebuilding.
- **D-05:** Unavailable states also carry a reason code so clients can distinguish locked session, first-time backfill, version-mismatch rebuild, manual rebuild, and rebuild-failed-waiting-for-retry without inventing their own inference layer.
- **D-06:** Status responses should act as the reconnect-safe truth source for current search availability. A client that misses WebSocket events should still be able to recover the current blocked/rebuilding state from `/search/status`.

### Rebuild Progress Events
- **D-07:** Search rebuild events use a dedicated search WebSocket topic/stream rather than piggybacking on `status` or unrelated topics. Keep the wire contract domain-scoped like existing clipboard and file-transfer topics.
- **D-08:** Rebuild events are not lifecycle-only. Emit start, incremental progress with `indexed` / `total`, and terminal complete / failed states so later UI can show real progress instead of a generic busy spinner.
- **D-09:** Search event payloads should follow the existing daemon event rules: stable topic/event constants in `uc_core::network::daemon_api_strings`, camelCase payload fields, and forwarding through the existing daemon WS broadcast path.

### Carried-Forward Guardrails
- **D-10:** Lock state remains authoritative: `/search/query`, `/search/rebuild`, and `/search/status` return HTTP 423 when the encryption session is locked.
- **D-11:** Search remains honestly unavailable during rebuild windows and version mismatch. There is no stale-result fallback.
- **D-12:** The first unlocked opportunity should auto-start backfill / rebuild for existing history instead of requiring a user-discovered manual action.

### the agent's Discretion
- Exact DTO field names as long as they preserve the readable-query-parameter choice and the product-state plus reason-code split above.
- Exact reconnect/polling details between `/search/status` and WebSocket subscriptions, as long as a late subscriber can recover truthful current state.
- Exact progress emission cadence (per batch vs throttled updates), as long as start, incremental counters, and terminal states remain observable.
- Exact bootstrap/runtime wiring shape, as long as daemon stays a thin HTTP/WS layer over the existing search use cases and does not re-implement search business logic.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Phase Scope And Acceptance
- `.planning/ROADMAP.md` §Phase 92 — Goal, locked-session requirement, route surface, and rebuild WebSocket expectation for this phase.
- `.planning/REQUIREMENTS.md` §SQRY-01–§SQRY-06, §REBLD-04 — Query filters, exact-match semantics, 423 lock behavior, structured `invalid_query`, and rebuild progress broadcast requirement.
- `.planning/STATE.md` — Current Phase 92 precondition to reuse existing `DaemonApiEventEmitter` patterns for search progress events.

### Prior Search Decisions
- `.planning/phases/88-core-domain-and-port-contracts/88-CONTEXT.md` — Locked `SearchResult`, `SearchError`, and `RebuildProgress` contracts that Phase 92 must expose without redefining.
- `.planning/phases/89-use-cases-and-delete-integration/89-CONTEXT.md` — Thin use-case boundaries and delete cleanup semantics already routed through `SearchIndexPort`.
- `.planning/phases/90-sqlite-schema-migration-and-tokenizer-pipeline/90-CONTEXT.md` — Blocked-on-mismatch policy, explicit rebuild UX preference, profile isolation, and `active_time_ms` search semantics.
- `.planning/phases/91-sqlite-index-adapter-and-rebuild-strategy/91-CONTEXT.md` — First-unlocked auto-backfill, blocked rebuild semantics, double-write guarantees, and no-stale-search rule.

### Architecture And Runtime Contracts
- `docs/architecture/local-encrypted-search.md` §查询执行, §增量更新规则, §全量重建, §API 形态 — Daemon responsibility boundary, query execution order, rebuild expectations, and V1 API guidance.
- `src-tauri/crates/uc-core/src/search/error.rs` — Typed search error variants the daemon must map into HTTP/search status behavior.
- `src-tauri/crates/uc-core/src/search/result.rs` — `SearchResult` and `RebuildProgress` shapes that constrain Phase 92 HTTP/WS contracts.
- `src-tauri/crates/uc-core/src/network/daemon_api_strings.rs` — Single source of truth for daemon HTTP route and WebSocket string constants.

### Existing Daemon Patterns To Reuse
- `src-tauri/crates/uc-daemon/src/api/routes.rs` — Protected L2+ router composition pattern for new search routes.
- `src-tauri/crates/uc-daemon/src/api/clipboard.rs` — Current daemon REST DTO / pagination / error style for list-like endpoints.
- `src-tauri/crates/uc-daemon/src/api/dto/error.rs` — Existing daemon `{ code, message }` error envelope.
- `src-tauri/crates/uc-daemon/src/api/event_emitter.rs` — Current WebSocket emission adapter pattern and camelCase payload conventions.
- `src-tauri/crates/uc-daemon/src/api/lifecycle.rs` — Existing unlock/deferred-start entry points that Phase 92 may reuse for first-unlocked auto backfill.

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `src-tauri/crates/uc-infra/src/search/sqlite_index.rs` — Finished search adapter already owns query execution, blocked-state handling, `get_index_meta()`, and rebuild persistence; Phase 92 should consume it directly rather than duplicating search rules in daemon code.
- `src-tauri/crates/uc-infra/src/search/pipeline.rs` — Ready-to-use builder for `SearchDocument` and `SearchPosting` from authoritative clipboard fields, usable for capture-time indexing and rebuild input.
- `src-tauri/crates/uc-infra/src/search/text_extractor.rs` — Existing input contract for searchable fields and preview derivation, helpful for keeping capture-time indexing and rebuild input aligned.
- `src-tauri/crates/uc-daemon/src/api/clipboard.rs` and `src-tauri/crates/uc-daemon/src/api/conversion.rs` — Existing REST handler and transport-mapping pattern for query params, pagination, and DTO projection.
- `src-tauri/crates/uc-daemon/src/api/event_emitter.rs` and `src-tauri/crates/uc-daemon/src/api/ws.rs` — Existing WebSocket broadcast + subscription path that new search events should extend rather than bypass.

### Established Patterns
- Protected daemon routes are added by merging subrouters into `router_l2_plus` and rely on the shared auth and rate-limit middleware chain.
- Daemon transport errors use `ApiError` / `ApiErrorResponse` with stable `code` + `message`, not custom nested payloads per feature.
- WebSocket contracts are domain-scoped via `daemon_api_strings` constants and camelCase payload serialization.
- Unlock/deferred-start flow already runs through `/lifecycle/ready`, `SetupCompletionEmitter`, and `DaemonApiState` notify/gate plumbing; first-unlocked backfill should piggyback on those runtime boundaries instead of inventing a parallel unlock path.
- `AppDeps` currently has no explicit search bundle and `CoreUseCases` currently exposes no search accessor surface; Phase 92 needs one clear ownership path instead of ad-hoc construction inside handlers and workers.

### Integration Points
- `src-tauri/crates/uc-app/src/deps.rs` — Add a single owned dependency path for search index, key derivation, and pipeline usage.
- `src-tauri/crates/uc-app/src/usecases/mod.rs` — Add the daemon-facing accessor surface for search query / rebuild / index use cases.
- `src-tauri/crates/uc-daemon/src/api/routes.rs` plus a new search API module — Add `/search/query`, `/search/status`, and `/search/rebuild` to the protected daemon API.
- `src-tauri/crates/uc-daemon/src/workers/clipboard_watcher.rs` and existing unlock lifecycle entry points — Hook incremental indexing and first-unlocked auto backfill into already-owned daemon flows.
- `src-tauri/crates/uc-daemon/src/api/event_emitter.rs` and `src-tauri/crates/uc-daemon/src/api/ws.rs` — Extend topic registration and event forwarding for search rebuild progress.

</code_context>

<specifics>
## Specific Ideas

- The query route should stay human-readable and easy to inspect in logs or manual requests; avoid hiding filters inside an encoded blob.
- Clients should be able to show result counts and truthful unavailable reasons without guessing from raw index metadata.
- Rebuild progress should be visible as real counts on a dedicated search stream, not only as start/end toggles.

</specifics>

<deferred>
## Deferred Ideas

- Rich hint/debug fields on search error responses — not chosen for Phase 92; revisit only if a later client genuinely needs more than `code + message`.
- Exposing raw `search_index_meta` fields as the primary public contract — deferred unless a future debug/admin surface needs it.

### Reviewed Todos (not folded)
- `修复 setup 配对确认提示缺失` — surfaced by the todo matcher, but kept out of Phase 92 because this phase is about daemon search wiring, not setup/UI messaging.

</deferred>

---

*Phase: 92-bootstrap-wiring-and-daemon-http-routes*
*Context gathered: 2026-04-11*
