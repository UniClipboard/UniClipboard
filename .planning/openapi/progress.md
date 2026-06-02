# OpenAPI Normalization — Phase Progress (ADR-008)

Tracks phase status + cross-phase carry-overs. Authoritative spec: `normalization-spec.md` (§0 = locked decisions, supersedes all else).

## Status
- **P1 Contract foundation** — ✅ DONE + adversarially verified (`cargo check -p uc-daemon-contract -p uc-webserver` clean). 2026-06-02.
- **P2 Webserver annotate + normalize wire** — ✅ DONE + verified (cargo check clean; permanent `api_doc_has_no_dangling_refs` test green; 227 $refs / 0 dangling / 122 schemas). 2026-06-02.
- **P3 Native Rust consumer lockstep** — ✅ DONE + verified (7 decode sites → `ApiEnvelope.data`; 3 contract search wrappers deleted; cargo check + uc-cli/uc-daemon-client tests green; ref-integrity guard still passes). ⚠️ Live connect/dispatch smoke = recommended manual follow-up (no daemon in CI env). 2026-06-02.
- **P4 gen-openapi bin + schema** — ✅ DONE + verified (gen-openapi bin = hard $ref-integrity gate; `schema/openapi.json` 194 KB, byte-deterministic, 0 dangling, 48 ops; `gen:openapi` npm script added). 2026-06-02.
- **P5 FE codegen + bridge** — ✅ DONE + verified (gen:client idempotent; tsc 0 new errors; bridge injects `?auth` query + `callSdk` 401-retry; `src/api/generated/` excluded from prettier+eslint; CI drift-checks added to pr-check.yml). 2026-06-02.
- **P6 FE consumer migration** — ✅ DONE + verified (all §H JS consumers read `.data`; settings via generated SDK `callSdk`; `UpdateSettingsResponse` deleted + Rust smoke tests migrated; tsc 0 new errors; vitest 99 green; cargo clean; `classify*Error` + restore-410 ride through `DaemonApiError.details`). 2026-06-02.

## Remaining follow-ups (non-blocking)
- **Live smoke (from P3):** no daemon in this env — manually run `uniclip start` + a connect/dispatch/health/peers round-trip against a P2+ daemon to confirm the enveloped decode paths end-to-end (esp. the L1 `/auth/connect` bootstrap + unauthenticated `/health` probe).
- **client.ts error-message polish:** `src/api/daemon/client.ts` `handleResponse` still extracts the human message via `if (body.error)`, but the normalized `ApiErrorResponse` is `{ code, message, details? }` (no `.error`). Classifiers are unaffected (they read `DaemonApiError.details.message`) and status-based `DaemonErrorCode` mapping is unaffected, but the top-level `DaemonApiError.message` falls back to `"<status> on <endpoint>"` instead of surfacing `body.message`. Cosmetic UX polish for a later pass.
- **Spec §G/§H stale rows:** a couple of §H/§G prose rows still say "PUT /settings keep success/restartRequired top-level (non-breaking)" — superseded by §0.1 (folded into `data`). §0 is authoritative; left as-is (historical rationale).

## P1 result
- **New files (contract):** `dto/envelope.rs` (`ApiEnvelope<T>` + 38 `#[aliases]`, bare generic NOT registered), `dto/error.rs` (`ApiErrorResponse{code,message,details:Option<Value>}` + `new()`/`with_details()`), `dto/auth.rs` (`ConnectRequest`, `SessionTokenResponse`), `dto/storage.rs` (`StorageStatsDto`,`ClearCacheRequest`,`ClearCacheResponse`), `openapi_meta.rs` (metadata + `SecurityAddon` registering BOTH `session_query` + `session_header`).
- **Edited (contract):** `dto/{clipboard_command,encryption,settings,member,search}.rs` (+ToSchema +folded DTOs), `api/types.rs` (+ToSchema on Health/Status/Worker/Lifecycle/PeerSnapshot/SpaceMember/PresenceRefresh/DaemonWsEvent), `dto/mod.rs`+`api/mod.rs` (wiring), `Cargo.toml` (+chrono).
- **Edited (webserver, no wire change):** `dto/error.rs` (re-export contract `ApiErrorResponse`; one ctor site set `details:None`), `storage.rs` (import contract storage DTOs via `as StorageStatsResponse` alias; json output byte-identical).
- **Folded payloads:** `SettingsUpdateResultDto{success,restart_required}`, `MemberSyncResultDto{success}`, `SearchQueryResultDto{items,total,has_more}` (items field renamed from `data`→`items`).
- **Pairing graveyard:** NOTHING deleted — all 9 `dto/pairing.rs` DTOs are LIVE (used by `uc-daemon-client/src/http/pairing.rs`, `ws.rs`). §C.7's "dead" assumption was wrong.

## P2 result
- **48 operations / 45 path templates** registered in `openapi.rs` `paths()`; **122 component schemas**; §D 12-tag set; dual security (`session_query`+`session_header`) + `PUBLIC_PATHS=[/health, /auth/connect]`, sourced from contract `openapi_meta` via a `ContractMeta` Modify.
- All §H breaking endpoints normalized to `ApiEnvelope`/`ApiErrorResponse`. `restore` handler lives in `routes.rs` (not clipboard.rs) — rewired there: `ApiEnvelope<RestoreEntryResponse>`, 410 context → `details`, `code`/`message` preserved; 5 restore unit tests updated and passing.
- Folded payloads wired: settings PUT → `SettingsUpdateResultEnvelope`, member PATCH → `MemberSyncResultEnvelope`, search query → `SearchQueryEnvelope`.
- **P2-fix (ref integrity):** utoipa v4 stringifies fully-qualified `body =` paths into dotted schema names → 69 FQ refs rewritten to bare names across 10 files; 9 `*PatchDto` + `ConnectRequest` registered in components; `KeyboardShortcutsPatchDto` got `ToSchema` + `#[schema(value_type = HashMap<String, ShortcutKeyDto>)]` (utoipa Option-valued-map limitation). Added a permanent `#[cfg(test)] api_doc_has_no_dangling_refs` guard (walks every `$ref`; regression net for P4/P5).
- **Surviving bespoke wrappers (NOT registered in the doc; deletion deferred):** `SearchQueryResponse`/`SearchStatusResponse`/`SearchRebuildAcceptedResponse` (still decoded by `uc-daemon-client/src/http/search.rs` → delete in **P3**); `UpdateSettingsResponse` (still built by settings smoke tests + an in-module test → delete in **P6**).

## Carry-overs INTO P2 (webserver wire normalization)
1. Rewire EVERY handler to build `ApiEnvelope::now(payload)` / alias bodies; flip bare/ad-hoc → envelope for ALL §H breaking endpoints. `/auth/connect` → `SessionTokenEnvelope`. Binary (`blob.rs`) + `/ws` stay un-enveloped.
2. DELETE the legacy bespoke `{data,ts}` wrapper structs after rewiring: `SearchQueryResponse`/`SearchStatusResponse`/`SearchRebuildAcceptedResponse` (search.rs), `GetMemberSyncPreferencesResponse`/`UpdateMemberSyncPreferencesResponse` (member.rs), `GetSettingsResponse`/`UpdateSettingsResponse` (settings.rs), `ListEntriesResponse`, `GetUpgradeStatusResponse`, + any sibling wrappers. P1 added NO new wrappers; these pre-existing ones must go so "no `{data,ts}` wrappers remain" (§0.1) holds.
3. `SearchQueryResultDto` renamed items field `data`→`items`; the query handler must build it that way and fold `total`/`hasMore` INTO the payload.
4. `storage.rs` still emits `json!({"data":..,"ts":..})` with `StorageStatsDto as StorageStatsResponse`; rewire to `ApiEnvelope`/`StorageStatsEnvelope`/`ClearCacheEnvelope`, drop the alias import.
5. restore 410: put `entry_id`/`rep_id`/`state` into `ApiErrorResponse.details` (`with_details`). Preserve `code` + exact English `message` strings (setup-v2 + restore classifiers depend on them).
6. `openapi.rs` assembly = SINGLE owner: register all ops in `paths()`, all alias names + DTOs in `components(schemas())`, wire `openapi_meta` SecurityAddon (dual) + tags + a `PUBLIC_PATHS` allowlist for L1 endpoints. Register the 2 missing setup-v2 ops (`switch-space`, `migration-progress`).
7. Use §D operation_ids + tags for every op; add `#[derive(IntoParams)]` to query structs (`RestoreQuery`, `SearchQueryParams`, `PaginationParams`).

## Carry-overs INTO P3 (native Rust clients)
- `uc-daemon-client/src/http/{clipboard,query,mod}.rs` + `uc-cli/src/send.rs`: unwrap `ApiEnvelope` (`.data`) for dispatch/resend/cancel/restore/peers/paired-devices/status/health/lifecycle/connect.
- **ENCRYPTION DESERIALIZE:** `EncryptionStateResponse`/`KeychainAccessResponse`/`EncryptionActionResponse` derive only `Serialize`. If any native client decodes enveloped encryption responses, add `Deserialize` (P1 left them Serialize-only).
- Wire mismatch is RUNTIME-only (cargo check won't catch it). After P2+P3, run a live smoke: connect handshake + one dispatch.

## Out-of-scope working-tree changes — DO NOT TOUCH / COMMIT (handoff §8)
- `src/api/file_transfer.ts` (new), `src/api/__tests__/file_transfer.test.ts` (new), `src/api/tauri-command/file_transfer.ts` (deleted), `src/components/clipboard/ClipboardPreview.tsx` (modified). Unrelated file_transfer migration. P5/P6 must avoid these. (`src/api/clipboardItems.ts` is NOT currently modified despite an earlier reviewer note.)
