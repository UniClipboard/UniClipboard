# UniClipboard OpenAPI Normalization Spec (ADR-008)

**What:** Normalize the daemon HTTP API into a single, deterministically-generated OpenAPI document and a generated TypeScript fetch client, replacing the ~20 hand-written `src/api/daemon/*.ts` wrappers and bespoke `{ data, ts }` response structs with a contract-owned, codegen-driven surface.

**Why:** The current surface is inconsistent (mixed envelopes: `{data,ts}` vs bare objects vs bare arrays vs ad-hoc JSON), partially undocumented (many handlers lack `#[utoipa::path]`; whole domains absent from the doc), and the hand-written FE types drift from the Rust wire shapes. A single source-of-truth contract + generated client removes drift and makes wire-shape changes reviewable as a git diff.

**Locked decisions (do not relitigate):**

1. **Contract owns the wire surface.** The canonical envelope, error body, and OpenAPI metadata live in `uc-daemon-contract`; `uc-webserver` keeps only the axum-coupled machinery and the handler-bound `#[utoipa::path]` annotations.
2. **A `gen-openapi` cargo bin** writes a deterministic, committed `schema/openapi.json` at repo root. It is the input to frontend codegen.
3. **Frontend codegen via `@hey-api/openapi-ts`** consumes `schema/openapi.json` into `src/api/generated/`, bridged into the existing `daemonClient` singleton (baseUrl, `?auth=` query token, 401 single-refresh).
4. **Binary and WebSocket endpoints are in-scope for documentation but exempt from the `{ data, ts }` envelope** — they keep `application/octet-stream` / the `DaemonWsEvent` protocol respectively.
5. **User sign-off (2026-06-02) — see §0, which SUPERSEDES the recommendations in §B/§C/§G/§H/§I below.** Headline: pure-generic envelope (no bespoke wrappers), ALL breaking endpoints normalized in this cycle (incl. native `uc-daemon-client`/`uc-cli` lockstep), `ApiErrorResponse` gains optional `details`, generated artifacts committed with a CI drift-check.

---

## 0. Locked Decisions — User Sign-off (2026-06-02) — SUPERSEDES §B/§C/§G/§H/§I

The §I questions were put to the user. The answers below are FINAL and OVERRIDE any contradicting recommendation later in this document (notably §B's "hybrid", §C.3's "wrappers stay", §G/§H's "deferred"). **Implementation agents: follow this section first.**

### 0.1 Envelope = PURE GENERIC (no bespoke wrappers)
- Every enveloppable endpoint returns `ApiEnvelope<T> { data: T, ts: i64 }`. There are **no** bespoke `{data,ts}`-redeclaring wrapper structs anywhere — replace the existing ones (`ListEntriesResponse`, `GetUpgradeStatusResponse`, `GetSettingsResponse`, …) with `ApiEnvelope<...>` aliases.
- The irregular endpoints **fold their extra top-level fields INTO the payload `T`** (new DTOs):
  - `PUT /settings` → `ApiEnvelope<SettingsUpdateResultDto { success, restart_required }>` (was top-level `success`+`restartRequired`).
  - `PATCH /member/.../sync-preferences` → `ApiEnvelope<MemberSyncResultDto { success }>`.
  - `GET /search/query` → `ApiEnvelope<SearchQueryResultDto { items, total, has_more }>` (move `total`/`hasMore` from top-level into the payload).
- **`#[aliases(...)]` is still REQUIRED** — it is the utoipa-v4 mechanism that turns each `ApiEnvelope<Concrete>` into a named `$ref` component (a bare generic cannot be registered; see §B "Why"). "Pure generic" means *no wrapper structs*, NOT *no aliases*. Maintain one alias per concrete payload — including the folded payloads above and **every newly-enveloped bare endpoint** in §H.

### 0.2 Scope = ALL breaking migrations in THIS cycle (no deferral)
The §G "Deferred" block and the §H "none executed except settings" note are VOID. Every breaking endpoint in §H is normalized now, with its consumers updated in lockstep — including the native Rust clients the original handoff missed:
- Command bodies: `dispatch`, `resend`, `cancel-transfer` → enveloped; update `uc-daemon-client/src/http/clipboard.rs` + `uc-cli/src/send.rs`.
- Bare arrays: `/peers`, `/paired-devices` → `ApiEnvelope<Vec<…>>`; update `uc-daemon-client/src/http/query.rs` + JS `members.ts`/`devicesSlice.ts`.
- Bare objects: `/status`, `/lifecycle/status`, `/health`, `/presence/refresh` → enveloped; update Rust `query.rs` + JS `lifecycle.ts`/`daemon-auth.ts`/`presence.ts`.
- `restore/{entry_id}` → `ApiEnvelope<RestoreEntryResponse>`; ad-hoc errors → `ApiErrorResponse` (410 context into `details`, §0.3).
- setup-v2 (all 6: initialize / issue-invitation / redeem / state / switch-space / migration-progress) → enveloped; update `setupV2.ts` AND keep `classify*Error` matchers working (they read `message`, whose text is preserved — §0.3).
- `POST /auth/connect` → ALSO enveloped (`ApiEnvelope<SessionTokenResponse>`); update the native decoder `uc-daemon-client/src/http/mod.rs`. Bootstrap caveat: connect is L1/public and decoded as plain JSON on both sides — enveloping is safe iff server + native client change together; verify the handshake against a live daemon in P3's gate.
- STILL EXEMPT (physical, not scope): binary `blobs/{id}` + `thumbnails/{rep_id}` (octet-stream), `/ws` (protocol upgrade). Doc-only.

### 0.3 Error body = `ApiErrorResponse { code, message, details? }`
Add an optional `details: Option<serde_json::Value>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`) to `ApiErrorResponse`. Additive — existing `{code,message}` consumers are unaffected. The restore-410 `payload_unavailable` context (`entry_id`/`rep_id`/`state`) goes into `details`. `code` and the English `message` strings remain LOAD-BEARING and unchanged (setup-v2 + restore classifiers substring-match them). Ensure `serde_json` is a dependency of `uc-daemon-contract` (add it if absent). Overrides §B Rule 5 + §I.3.

### 0.4 Generated artifacts = committed + CI drift-check
Commit BOTH `schema/openapi.json` and `src/api/generated/`. Add a CI step: `npm run gen:api` then `git diff --exit-code` to fail on drift. (As in §E/§F.)

### 0.5 Defaults adopted (orchestrator, no objection)
- **Auth scheme:** register BOTH `session_query` (`ApiKey::Query("auth")`, the real browser transport) and `session_header` (`ApiKey::Header("Authorization")`, native Rust client) — §C.5 already does this; keep it.
- **Pairing DTO graveyard:** in P1, grep for usages of the 8 dead `dto/pairing.rs` DTOs; delete only those with zero references. Non-blocking.

### 0.6 Revised phase plan (replaces the scope/ordering of §G; §G's per-domain detail is still valid)
1. **P1 — Contract foundation (additive, no wire change).** `ApiEnvelope<T>` + full alias registry (incl. folded payloads + every §H endpoint), `ApiErrorResponse { code, message, details? }` moved to contract, all new/relocated DTOs (`SettingsUpdateResultDto`, `MemberSyncResultDto`, `SearchQueryResultDto`, `RestoreEntryResponse`, storage DTOs, dispatch/resend/cancel bodies, diagnostic `ToSchema`s, `SessionTokenResponse`), `openapi_meta.rs` (dual security), pairing cleanup. Gate: `cargo check` (workspace).
2. **P2 — Webserver annotate + NORMALIZE wire.** Every handler gets `#[utoipa::path]` with the alias body, explicit `operation_id` + `tag` (§D), `IntoParams` on query structs, `PUBLIC_PATHS` allowlist. Bare/ad-hoc responses rewritten to `ApiEnvelope`/`ApiErrorResponse`. This is the breaking server change. Per-domain fan-out over non-overlapping files. Gate: `cargo check`.
3. **P3 — Native Rust consumer lockstep.** Update `uc-daemon-client/src/http/{clipboard,query,mod}.rs` + `uc-cli/src/send.rs` to decode enveloped shapes (unwrap `data`). Gate: `cargo check` (workspace) + live connect/dispatch smoke check if a daemon is available.
4. **P4 — gen-openapi bin + schema.** `src/bin/gen-openapi.rs`, `gen:openapi` script, commit `schema/openapi.json`. Gate: bin runs + inspect each alias rendered a real `$ref` (esp. `Vec<…>` + folded payloads), then `cargo check`.
5. **P5 — Frontend codegen + bridge.** `@hey-api/openapi-ts@0.97.3` devDep, `openapi-ts.config.ts`, generate + commit `src/api/generated/`, `generated-bridge.ts` + `callSdk`/`installGeneratedClientBridge`. Gate: `gen:api` + `tsc`.
6. **P6 — Frontend consumer migration (ALL breaking JS consumers).** Update every JS consumer in §H to read the enveloped shape; route the settings domain through the generated SDK as the exemplar. Preserve public wrapper signatures + `toSettingsPatchRequest`; keep `classify*Error` matchers. Gate: `tsc`/`typecheck` + the full vitest suite (settings + lifecycle + setup-v2 + clipboard).

Validation is serial at each gate; fan-out only within a phase across non-overlapping files.

---

## A. Endpoint Inventory

Legend — **auth:** `L1` = public/no-auth, `L2+` = authed (auth_extractor + rate_limit), `dev` = `#[cfg(debug_assertions)]` only. **envelope:** `data-ts` = `{data, ts}`, `bare` = un-enveloped object, `bare-array` = top-level JSON array, `204` = No Content, `binary` = octet-stream, `other` = non-standard (extra siblings / protocol upgrade / ad-hoc). **utoipa today:** ✓ has `#[utoipa::path]` and is in `ApiDoc`; ⚠ has the attr but is missing from `ApiDoc.paths()`; ✗ none.

| Method | Path | Tag | operationId | Auth | Current envelope | DTO location | utoipa today | Breaking if normalized |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| GET | `/clipboard/entries` | clipboard | listClipboardEntries | L2+ | data-ts | contract (`dto/clipboard.rs`) | ✓ | No |
| GET | `/clipboard/entries/{id}` | clipboard | getClipboardEntry | L2+ | data-ts | contract (`dto/clipboard.rs`) | ✓ | No |
| DELETE | `/clipboard/entries/{id}` | clipboard | deleteClipboardEntry | L2+ | 204 | none | ✓ | No |
| POST | `/clipboard/entries/{id}/favorite` | clipboard | toggleClipboardEntryFavorite | L2+ | data-ts | contract (`dto/clipboard.rs`) | ✓ | No |
| GET | `/clipboard/stats` | clipboard | getClipboardStats | L2+ | data-ts | contract (`dto/clipboard.rs`) | ✓ | No |
| GET | `/clipboard/entries/{id}/resource` | clipboard | getClipboardEntryResource | L2+ | data-ts | contract (`dto/clipboard.rs`) | ✓ | No |
| POST | `/clipboard/entries/clear` | clipboard | clearClipboardHistory | L2+ | data-ts | contract (`dto/clipboard.rs`) | ✓ | No |
| POST | `/clipboard/restore/{entry_id}` | clipboard | restoreClipboardEntry | L2+ | other (ad-hoc `{success:true}`; ad-hoc errors incl. 410) | none-needs-creating | ✗ | No (success body ignored by consumers; 410 context must be preserved) |
| POST | `/clipboard/dispatch` | clipboard | dispatchClipboardText | L2+ | bare | contract (`dto/clipboard_command.rs`, no ToSchema) | ✗ | **Yes** (Rust `DaemonClipboardClient` strict-decodes) |
| POST | `/clipboard/resend` | clipboard | resendClipboardEntry | L2+ | bare | contract (`dto/clipboard_command.rs`, no ToSchema) | ✗ | **Yes** (Rust client strict-decodes) |
| POST | `/clipboard/cancel-transfer/{transfer_id}` | clipboard | cancelClipboardTransfer | L2+ | bare | contract (`dto/clipboard_command.rs`, no ToSchema) | ✗ | **Yes** (Rust client strict-decodes; JS discards) |
| GET | `/clipboard/blobs/{blob_id}` | clipboard | getClipboardBlob | L2+ | binary | none (raw bytes) | ✗ | **Yes if enveloped** (kept binary; doc-only) |
| GET | `/clipboard/thumbnails/{rep_id}` | clipboard | getClipboardThumbnail | L2+ | binary | none (raw bytes) | ✗ | **Yes if enveloped** (kept binary; doc-only) |
| GET | `/search/query` | search | searchQuery | L2+ | other (`{data,total,hasMore,ts}`) | contract (`dto/search.rs`) | ✗ (schemas registered) | **Yes** (FE reads `total` top-level) |
| GET | `/search/status` | search | getSearchStatus | L2+ | data-ts | contract (`dto/search.rs`) | ✗ (schemas registered) | No |
| POST | `/search/rebuild` | search | rebuildSearchIndex | L2+ | data-ts (HTTP 202) | contract (`dto/search.rs`) | ✗ (schemas registered) | No |
| GET | `/storage/stats` | storage | getStorageStats | L2+ | data-ts | webserver-inline (no ToSchema) | ✗ | No |
| POST | `/storage/clear-cache` | storage | clearStorageCache | L2+ | data-ts (success); error `{error:{code,message},ts}` | webserver-inline (no ToSchema) | ✗ | No (success ignored; only error-shape change) |
| GET | `/device/me` | device | getLocalDeviceInfo | L2+ | data-ts | contract (`dto/device.rs`) | ✓ | No |
| GET | `/peers` | system | listPeers | L2+ | bare-array | contract (`types.rs`, no ToSchema) | ✗ | **Yes** (Rust daemon-client; no JS HTTP consumer) |
| GET | `/paired-devices` | system | listPairedDevices | L2+ | bare-array | contract (`types.rs`, no ToSchema) | ✗ | **Yes** (JS `members.ts` + Rust daemon-client) |
| POST | `/presence/refresh` | system | refreshPresence | L2+ | bare | contract (`types.rs`, no ToSchema) | ✗ | **Yes** (JS `presence.ts` reads counters top-level) |
| GET | `/member/{device_id}/sync-preferences` | member | getMemberSyncPreferences | L2+ | data-ts | contract (`dto/member.rs`) | ✓ | No |
| PATCH | `/member/{device_id}/sync-preferences` | member | updateMemberSyncPreferences | L2+ | other (`{success,data,ts}`) | contract (`dto/member.rs`) | ✓ | No (FE returns `res.data` only; `success` droppable) |
| POST | `/pairing/unpair` | pairing | unpairDevice | L2+ | 204 | contract (`dto/pairing.rs`) | ✓ | No (keep 204) |
| GET | `/encryption/state` | encryption | getEncryptionState | L2+ | data-ts | contract (`dto/encryption.rs`) | ✓ (body untyped) | No |
| POST | `/encryption/unlock` | encryption | unlockEncryptionSession | L2+ | data-ts | none-needs-creating | ✓ (body untyped) | No (FE discards body) |
| POST | `/encryption/lock` | encryption | lockEncryptionSession | L2+ | data-ts | none-needs-creating | ✓ (body untyped) | No (FE discards body) |
| GET | `/encryption/keychain-access` | encryption | verifyKeychainAccess | L2+ | data-ts | contract (`dto/encryption.rs`) | ✓ (body untyped) | No |
| GET | `/settings` | settings | getSettings | L2+ | data-ts | contract (`dto/settings.rs`) | ✓ | No |
| PUT | `/settings` | settings | updateSettings | L2+ | other (`{success,data,ts,restartRequired}`) | contract (`dto/settings.rs`) | ✓ (no request_body declared) | **Yes if siblings moved into data** (FE reads `success`/`restartRequired` top-level) |
| GET | `/lifecycle/status` | lifecycle | getLifecycleStatus | L2+ | bare (`{state}`) | contract (`types.rs`, no ToSchema) | ✗ | **Yes** (FE reads `dto.state` top-level) |
| POST | `/lifecycle/retry` | lifecycle | retryLifecycle | L2+ | 204 | none-needs-creating | ✗ | No (keep 204) |
| POST | `/lifecycle/ready` | lifecycle | signalLifecycleReady | L2+ | 204 | none-needs-creating | ✗ | No (keep 204; FE discards) |
| GET | `/upgrade/status` | upgrade | getUpgradeStatus | L2+ | data-ts | contract (`dto/upgrade.rs`) | ✓ | No |
| POST | `/upgrade/ack` | upgrade | acknowledgeUpgrade | L2+ | data-ts | contract (`dto/upgrade.rs`) | ✓ | No |
| GET | `/health` | system | getHealth | **L1** | bare (`{status,...}`) | contract (`types.rs`, no ToSchema) | ✗ | **Yes** (FE `daemon-auth.ts` reads `.status`) |
| GET | `/status` | system | getStatus | L2+ | bare | contract (`types.rs`, no ToSchema) | ✗ | **Yes** (Rust daemon-client strict-decodes) |
| GET | `/ws` | system | websocketUpgrade | L2+ (self-enforced) | other (101 upgrade; `DaemonWsEvent` frames) | contract (`dto/ws.rs`, `types.rs`) | ⚠ (orphaned attr, absent from `ApiDoc`) | **Yes if enveloped** (kept as protocol; doc-only) |
| POST | `/auth/connect` | system | authConnect | **L1** | other (flat `{sessionToken,...}`; ad-hoc errors) | webserver-inline (no ToSchema) | ✗ | **Yes** (native Rust decoder strict-decodes flat shape) |
| POST | `/v2/setup/initialize` | setup-v2 | setupV2Initialize | L2+ | bare | contract (`dto/v2/setup.rs`) | ✓ | **Yes** (FE reads fields top-level) |
| POST | `/v2/setup/issue-invitation` | setup-v2 | setupV2IssueInvitation | L2+ | bare | contract (`dto/v2/setup.rs`) | ✓ | **Yes** (FE reads fields top-level) |
| POST | `/v2/setup/redeem` | setup-v2 | setupV2Redeem | L2+ | bare | contract (`dto/v2/setup.rs`) | ✓ | **Yes** (FE reads fields top-level) |
| POST | `/v2/setup/cancel` | setup-v2 | setupV2Cancel | L2+ | 204 | none-needs-creating | ✓ | No (keep 204) |
| POST | `/v2/setup/reset` | setup-v2 | setupV2Reset | L2+ | 204 | none-needs-creating | ✓ | No (keep 204) |
| GET | `/v2/setup/state` | setup-v2 | setupV2GetState | L2+ | bare | contract (`dto/v2/setup.rs`) | ✓ | **Yes** (FE reads fields top-level) |
| POST | `/v2/setup/switch-space` | setup-v2 | setupV2SwitchSpace | L2+ | bare | contract (`dto/v2/setup.rs`) | ⚠ (attr present, missing from `ApiDoc.paths()`) | **Yes** (FE reads fields top-level) |
| GET | `/v2/setup/migration-progress` | setup-v2 | setupV2GetMigrationProgress | L2+ | bare | contract (`dto/v2/setup.rs`) | ⚠ (attr present, missing from `ApiDoc.paths()`) | **Yes** (FE reads fields top-level) |
| POST | `/auth/dev-token` | dev | authDevToken | **dev** | other (flat) | webserver-inline (`DevTokenResponse` ToSchema) | ✓ (`ApiDocDev` only) | No (no production consumer) |

**Inventory totals:** 49 distinct operations. The per-domain analyses double-listed `/peers`, `/paired-devices`, `/presence/refresh` (device + system) and `/clipboard/restore` (clipboard + system); these collapse to one operation each. `authDevToken` is dev-only and lives in `ApiDocDev`, not the production `ApiDoc`.

**Cross-cutting facts that hold for the whole surface:**

- **No `/api/v1` prefix.** `build_router` (`server.rs:180`) merges `router_l1` + `router_l2_plus` + swagger + connect + ws at root; all paths resolve verbatim.
- **Auth transport mismatch (the headline cross-cutting bug).** `SecurityAddon` (`openapi.rs:56-59`) declares the L2 scheme as `ApiKey::Header("Authorization")` with value `Session <token>`. The browser/GUI `DaemonClient` actually sends `?auth=Session <token>` as a **query param** (`client.ts:217-218`, and `blobUrl()` for binary). The native Rust `uc-daemon-client` does send the header. The documented scheme does not match the browser transport. See §B and §I.
- **DaemonClient does NO envelope unwrapping.** `handleResponse` (`client.ts:233-259`) `JSON.parse`s the body and returns it verbatim as `T`; 204/205/empty → `undefined`. Whether each consumer reads `.data` is decided entirely by the hand-written wrapper, which is why bare-vs-enveloped is a per-endpoint breaking question.

---

## B. Response Envelope & Error Design

### Decision

> **⚠️ SUPERSEDED BY §0.1/§0.3 (user sign-off):** the envelope is **pure generic** — NO bespoke wrappers (irregular endpoints fold their extra fields into the payload `T`), and `ApiErrorResponse` gains an optional `details` field. The `#[aliases(...)]` mechanism described below STILL applies (it is required by utoipa v4). Read the rest of §B for the rationale + the alias/error code, but apply §0 where they differ.

Adopt a single generic `ApiEnvelope<T> { data: T, ts: i64 }` defined once in `uc-daemon-contract`, surfaced into OpenAPI **exclusively via `#[aliases(...)]`** — one named alias per concrete payload (e.g. `SettingsEnvelope = ApiEnvelope<SettingsDto>`). The canonical error body is `ApiErrorResponse { code, message, details? }` (per §0.3), relocated into the contract crate, **never** wrapped in `{ data, ts }`.

### Why (utoipa v4 reality — verified against `utoipa 4.2.3` / `utoipa-gen 4.3.1` in `src-tauri/Cargo.lock`)

- In utoipa v4 a generic `ToSchema` becomes a **named** component only through a derive-level `#[aliases(Name = ApiEnvelope<Concrete>)]` entry. The bare generic must **never** be put in `components(schemas(...))` — utoipa errors: "You should never register generic type itself in `components(...)` ... it will not render the type correctly and will cause an error in generated OpenAPI spec."
- If a path references `ApiEnvelope<T>` **without** a matching alias, utoipa **inlines an anonymous schema** at the operation site (no `$ref`). `@hey-api/openapi-ts` then emits an unstable, non-reusable inline TS type per operation — the "ugly inline TS" outcome. With an alias, the body is a `$ref: '#/components/schemas/SettingsEnvelope'`, and hey-api emits one reusable interface.
- A workspace-wide grep found **zero** `#[aliases(...)]` usage and **zero** generic response schemas in `src-tauri/crates`. Today every documented response is a bespoke non-generic wrapper (`GetUpgradeStatusResponse`, `GetSettingsResponse`, `ListEntriesResponse`, …) that re-declares `{ data, ts }`. These render clean schemas (proving option B works) but cost ~20 hand-maintained structs and invite drift (e.g. `QuickPanelSettingsDto` unregistered, `ToggleFavoriteResultDto` missing `rename_all`). The hybrid keeps the clean-schema win while collapsing the boilerplate to one alias line per payload.
- Pinned versions: `uc-daemon-contract/Cargo.toml` declares `utoipa = { version = "4", features = ["uuid","chrono","url"] }`; `Cargo.lock` resolves `utoipa 4.2.3`, `utoipa-gen 4.3.1`, `utoipa-swagger-ui 7.1.0`. This is v4 behavior — **not** the v5/`utoipa-config` build.rs aliasing; do not assume `utoipa-config`.

### Rules

1. **Strict `{ data, ts }` body** → wrap in `ApiEnvelope<T>` and add a `#[aliases(...)]` entry. Reference the **alias name** in `#[utoipa::path(... body = SettingsEnvelope)]`; register the **alias** (not the bare generic) in `components(schemas(...))`.
2. **Extra top-level siblings** (`PUT /settings` → `success`+`restartRequired`; `PATCH /member/.../sync-preferences` → `success`; `GET /search/query` → `total`+`hasMore`) → per §0.1, **fold these siblings INTO the payload `T`** via new DTOs (`SettingsUpdateResultDto` / `MemberSyncResultDto` / `SearchQueryResultDto`) and wrap in `ApiEnvelope`. No bespoke wrappers.
3. **204 No Content** (`deleteClipboardEntry`, `retryLifecycle`, `signalLifecycleReady`, `unpairDevice`, `setupV2Cancel`, `setupV2Reset`) → no envelope; document `(status = 204, description = "No Content")` with no body.
4. **Binary** (`getClipboardBlob`, `getClipboardThumbnail`) → `application/octet-stream`; **`/ws`** → its own `DaemonWsEvent` protocol. Both exempt from `{ data, ts }`.
5. **Errors** → `ApiErrorResponse { code, message, details? }` (§0.3), unwrapped, `code`/`message` **never renamed** (setup-v2 `classifySwitchSpaceError`/`classifyRedeemError` and the clipboard restore-410 path substring-match `code`/`message`). The axum `ApiError` carrier + `IntoResponse` + `log_facade_failure` stay in `uc-webserver`; only the body DTO moves to contract.

### Rust definitions

Canonical success envelope (`uc-daemon-contract/src/api/dto/envelope.rs`):

```/Users/mark/.superset/worktrees/c099fb29-e458-4a78-98f3-beeb1fb4964a/adr-008/src-tauri/crates/uc-daemon-contract/src/api/dto/envelope.rs
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

// Payload DTOs that get wrapped (imported from their own modules):
use crate::api::dto::settings::SettingsDto;
use crate::api::dto::clipboard::{
    ClipboardStatsDto, ClearHistoryResultDto, EntryDetailDto, EntryProjectionResponseDto,
    EntryResourceDto, ToggleFavoriteResultDto,
};
use crate::api::dto::device::LocalDeviceInfoDto;
use crate::api::dto::encryption::{EncryptionStateResponse, KeychainAccessResponse};
use crate::api::dto::upgrade::{AckUpgradePayload, UpgradeStatusDto};
use crate::api::dto::member::MemberSyncPreferencesDto;
use crate::api::dto::search::{SearchStatusData, SearchRebuildAcceptedData};

/// Canonical success envelope: `{ "data": T, "ts": <unix millis i64> }`.
///
/// `ts` is `chrono::Utc::now().timestamp_millis()` (set in the webserver handler;
/// the contract carries only the type, not the clock). camelCase is a no-op here
/// (single-word fields) but declared for forward-compat.
///
/// IMPORTANT (utoipa v4): every concrete `ApiEnvelope<X>` we want as a named
/// OpenAPI component is declared below via `#[aliases(...)]`. Add a new alias
/// line whenever a new payload type needs enveloping. NEVER register the bare
/// `ApiEnvelope` in `components(schemas(...))`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[aliases(
    // clipboard
    ListEntriesEnvelope = ApiEnvelope<Vec<EntryProjectionResponseDto>>,
    EntryDetailEnvelope = ApiEnvelope<EntryDetailDto>,
    EntryResourceEnvelope = ApiEnvelope<EntryResourceDto>,
    ClipboardStatsEnvelope = ApiEnvelope<ClipboardStatsDto>,
    ClearHistoryEnvelope = ApiEnvelope<ClearHistoryResultDto>,
    ToggleFavoriteEnvelope = ApiEnvelope<ToggleFavoriteResultDto>,
    // settings (GET + PUT both enveloped per §0.1)
    SettingsEnvelope = ApiEnvelope<SettingsDto>,
    SettingsUpdateResultEnvelope = ApiEnvelope<SettingsUpdateResultDto>,
    // device / member
    LocalDeviceInfoEnvelope = ApiEnvelope<LocalDeviceInfoDto>,
    MemberSyncPreferencesEnvelope = ApiEnvelope<MemberSyncPreferencesDto>,
    // encryption
    EncryptionStateEnvelope = ApiEnvelope<EncryptionStateResponse>,
    KeychainAccessEnvelope = ApiEnvelope<KeychainAccessResponse>,
    // upgrade
    UpgradeStatusEnvelope = ApiEnvelope<UpgradeStatusDto>,
    AckUpgradeEnvelope = ApiEnvelope<AckUpgradePayload>,
    // search (status + rebuild + query all enveloped per §0.1)
    SearchStatusEnvelope = ApiEnvelope<SearchStatusData>,
    SearchRebuildEnvelope = ApiEnvelope<SearchRebuildAcceptedData>,
    SearchQueryEnvelope = ApiEnvelope<SearchQueryResultDto>,
    // NOTE: this alias list is illustrative — P1 must ADD an alias for every
    // newly-enveloped bare endpoint in §H (health/status/peers/paired-devices/
    // presence/lifecycle/dispatch/resend/cancel-transfer/restore/setup-v2/connect).
)]
pub struct ApiEnvelope<T> {
    pub data: T,
    /// Server time when the response was built (unix epoch milliseconds).
    pub ts: i64,
}

impl<T> ApiEnvelope<T> {
    pub fn now(data: T) -> Self {
        Self { data, ts: chrono::Utc::now().timestamp_millis() }
    }
    pub fn with_ts(data: T, ts: i64) -> Self {
        Self { data, ts }
    }
}
```

Canonical error body, **moved** from `uc-webserver/src/api/dto/error.rs` into the contract crate:

```/Users/mark/.superset/worktrees/c099fb29-e458-4a78-98f3-beeb1fb4964a/adr-008/src-tauri/crates/uc-daemon-contract/src/api/dto/error.rs
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Canonical daemon HTTP error body.
///
/// Wire shape: `{ "code": "<machine_code>", "message": "<human text>" }`.
/// `code` is a stable snake_case token (e.g. `not_found`, `bad_request`,
/// `runtime_unavailable`, `conflict`, `internal_error`, `payload_unavailable`).
/// `message` is human-readable English; setup-v2 error classifiers and the CLI
/// 410 handler substring-match it, so the strings are LOAD-BEARING — do not
/// silently reword.
///
/// `Deserialize` is added (the original webserver struct was Serialize-only) so
/// the Rust `uc-daemon-client` and tests can decode error bodies.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ApiErrorResponse {
    pub code: String,
    pub message: String,
    /// Optional structured context (per §0.3). E.g. restore-410 `payload_unavailable`
    /// carries `{ entry_id, rep_id, state }`. Omitted from the wire when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}
```

The axum `ApiError` carrier keeps its `IntoResponse`, importing the contract body:

```/Users/mark/.superset/worktrees/c099fb29-e458-4a78-98f3-beeb1fb4964a/adr-008/src-tauri/crates/uc-webserver/src/api/dto/error.rs
pub use uc_daemon_contract::api::dto::error::ApiErrorResponse;
// ApiError + constructors + IntoResponse + log_facade_failure remain here.
//
// impl IntoResponse for ApiError {
//     fn into_response(self) -> Response {
//         let body = ApiErrorResponse { code: self.code, message: self.message };
//         (self.status, axum::Json(body)).into_response()
//     }
// }
```

### utoipa-generic findings (verification record)

- **Generic-alias rule (v4):** named component only via derive-level `#[aliases(...)]`, one entry per concrete `T`; bare generic out of `components(...)`.
- **Inline-anonymous risk:** `body = ApiEnvelope<T>` without an alias → anonymous inline schema → unstable, non-reusable hey-api TS type. The alias is the lever that controls the public TS type name; pick alias names deliberately.
- **`Vec<...>` aliases** (e.g. `ApiEnvelope<Vec<EntryProjectionResponseDto>>`) are supported but are the kind of thing to **verify in the emitted JSON** — array-of-`$ref` inside a named envelope.
- **Implementer caveat:** `ApiEnvelope<T>` is the simplest generic shape (one type param, one wrapped field) and is well within v4's reliable path, but the `gen-openapi` bin must be run and `schema/openapi.json` eyeballed to confirm each alias produced a real `$ref` component (not an inline) before wiring hey-api.

### Trade-offs

- **Pro:** single source of truth for `{ data, ts }`; clean reusable named schemas; one alias line per new enveloped endpoint; contract-owned without a webserver→contract dependency inversion.
- **Con:** the `#[aliases(...)]` list is a manual registry — forgetting an alias silently degrades to inline TS, so it needs a PR review checklist + the "inspect exported openapi.json" verification step; v4 generic-alias support is the crate's roughest corner (one-time validation cost).
- **Pure generic (no aliases): rejected** — errors or inlines anonymous schemas.
- **Pure named wrappers (status quo): rejected as the default** — clean schemas but ~20 hand-maintained near-identical structs with drift; the hybrid is a strict superset that keeps named wrappers only for the genuinely-irregular endpoints (PUT settings, PATCH member, paginated search query).
- **Error type:** keeping `ApiErrorResponse { code, message }` unchanged (relocated + `Deserialize` added) is zero-risk and preserves the load-bearing `message`/`code` contract; wrapping errors in `{data,ts}` or adding/renaming fields would break setup-v2 and restore error classification, so it is explicitly avoided.

---

## C. Contract Layout & contract↔webserver Split

This section defines the concrete module tree: what is **added/moved** into `uc-daemon-contract`, and what **stays** in `uc-webserver` (contract must not depend on webserver, and `#[utoipa::path]` must sit on the handler fns).

### C.0 Important correction to the per-domain analysis

The analysis repeatedly states clipboard *history* DTOs are `webserver-inline` (`uc-webserver/src/api/dto/clipboard.rs`). **That file does not exist.** The clipboard history DTOs already live at `uc-daemon-contract/src/api/dto/clipboard.rs` and already derive `ToSchema`. `uc-webserver/src/api/dto/mod.rs` only re-exports them:

```/Users/mark/.superset/worktrees/c099fb29-e458-4a78-98f3-beeb1fb4964a/adr-008/src-tauri/crates/uc-webserver/src/api/dto/mod.rs
pub mod error;
pub mod search;
pub use uc_daemon_contract::api::dto::{
    clipboard, device, encryption, member, pairing, settings, setup, ws,
};
```

The only genuinely **webserver-local DTOs** are `dto/error.rs` (the `ApiError`/`ApiErrorResponse` pair) and `dto/search.rs` (a pass-through re-export). So the contract-side work is narrower than implied: add `ToSchema` to a handful of command/diagnostic structs, add the envelope + a couple of new request/response DTOs, and relocate the *error response* type + OpenAPI metadata.

### C.1 Target module tree in `uc-daemon-contract/src/api/`

```
src-tauri/crates/uc-daemon-contract/src/api/
├── mod.rs                 # add: pub mod openapi_meta;
├── auth.rs                # EXISTS (DaemonConnectionInfo) — unchanged
├── types.rs               # EXISTS — ADD #[derive(ToSchema)] to structs in §C.4
├── openapi_meta.rs        # NEW — info/servers/tags + SecurityAddon (§C.5)
└── dto/
    ├── mod.rs             # add: pub mod envelope; pub mod error; pub mod auth; pub mod storage;
    ├── envelope.rs        # NEW — generic ApiEnvelope<T> + aliases (§B)
    ├── error.rs           # NEW (MOVED from webserver) — ApiErrorResponse (§B)
    ├── auth.rs            # NEW — ConnectRequest + SessionTokenResponse (§C.6)
    ├── clipboard.rs       # EXISTS — history DTOs (no change; already ToSchema)
    ├── clipboard_command.rs  # EXISTS — ADD ToSchema + new RestoreEntryResponse (§C.4)
    ├── device.rs          # EXISTS — /device/me DTOs (no change)
    ├── encryption.rs      # EXISTS — ADD EncryptionActionResponse (§C.4)
    ├── member.rs          # EXISTS — no change
    ├── pairing.rs         # EXISTS — UnpairDeviceRequest only is live (§C.7 cleanup)
    ├── search.rs          # EXISTS — already ToSchema; SearchQueryParams stays webserver-side
    ├── settings.rs        # EXISTS — no MOVE; only schema-registration gaps to fix in webserver
    ├── storage.rs         # NEW — storage DTOs MOVED out of webserver storage.rs (§C.4)
    ├── upgrade.rs         # EXISTS — already ToSchema, already {data,ts}
    ├── ws.rs              # EXISTS — WsSubscribeRequest/WsErrorResponse (already ToSchema)
    └── v2/setup.rs        # EXISTS — already ToSchema (bare bodies; §C.8)
```

`uc-daemon-contract/Cargo.toml` already has `utoipa = { version = "4", ... }` and `serde_with = "3.18.0"`, and depends on `uc-core` (not `uc-application`/`uc-webserver`), so all of the above is dependency-safe.

### C.2 Error type relocation

`ApiErrorResponse` (the only **moved** piece) splits out of `uc-webserver/src/api/dto/error.rs:65` into `uc-daemon-contract/src/api/dto/error.rs` with `Deserialize + Clone` added (so the generated TS client and native Rust client both decode it). The axum-coupled `ApiError` struct, constructors, `IntoResponse` impls, and `log_facade_failure` **stay** in webserver and import `ApiErrorResponse` from contract via re-export. See §B for code.

### C.3 Canonical response envelope

`ApiEnvelope<T>` + the `#[aliases(...)]` registry live in the new `dto/envelope.rs` (§B). **Decision (per §0.1):** the existing bespoke wrapper structs (`ListEntriesResponse`, `GetUpgradeStatusResponse`, `GetSettingsResponse`, …) are **replaced** by `ApiEnvelope<...>` aliases in this cycle (not kept). Every documented response uses an alias; no `{data,ts}`-redeclaring wrapper structs remain.

### C.4 DTOs to ADD / get `ToSchema` (contract crate)

| DTO | Action | Target file |
| --- | --- | --- |
| `DispatchTextRequest`, `DispatchOutcomeResponse`, `PerTargetOutcomeDto` | **add `ToSchema`** | `dto/clipboard_command.rs` |
| `ResendRequest`, `ResendResponse` | **add `ToSchema`** | `dto/clipboard_command.rs` |
| `CancelTransferRequest`, `CancelTransferResponse` | **add `ToSchema`** | `dto/clipboard_command.rs` |
| `InboundNoticeEvent` (WS payload) | **add `ToSchema`** (optional, WS doc) | `dto/clipboard_command.rs` |
| `RestoreEntryResponse { success: bool }` | **CREATE** (success body for restore; wrap in `ApiEnvelope`) | `dto/clipboard_command.rs` |
| `EncryptionActionResponse { success: bool }` | **CREATE** (shared body for unlock + lock) | `dto/encryption.rs` |
| `StorageStatsDto`, `ClearCacheRequest`, `ClearCacheResponse` | **MOVE** from webserver `storage.rs` inline + add `ToSchema` | `dto/storage.rs` (NEW) |
| `LifecycleStatusResponse` | **add `ToSchema`** in place | `api/types.rs` |
| `HealthResponse`, `StatusResponse`, `WorkerStatusDto` | **add `ToSchema`** in place | `api/types.rs` |
| `PeerSnapshotDto`, `SpaceMemberDto`, `PresenceRefreshResponse` | **add `ToSchema`** in place | `api/types.rs` |
| `DaemonWsEvent` | **add `ToSchema`** in place (minimal WS doc) | `api/types.rs` |

Notes:
- `RestoreQuery { plain: bool }` (`routes.rs`), `SearchQueryParams` (`search.rs`), `PaginationParams` (`clipboard.rs`) are **query-param structs** — they stay in webserver and get `#[derive(IntoParams)]` (documented as params, not body schemas).
- `ClearCacheErrorResponse` (storage inline) is **dropped** — normalize its 400 path onto `ApiErrorResponse`.
- The `*PatchDto` / `KeyboardShortcutsPatchDto` / `QuickPanelSettingsDto` settings gaps are **registration** fixes in `openapi.rs components(schemas(...))`, not relocations.
- `restore` 410 `payload_unavailable` carries load-bearing `entry_id`/`rep_id`/`state`. If migrated to `ApiError`, preserve those fields (see §I open question).

### C.5 OpenAPI metadata + SecurityAddon (`api/openapi_meta.rs`, NEW in contract)

Cross-cutting metadata that has no handler-path dependency moves to contract as `utoipa::openapi`-typed helpers. **This fixes the auth scheme mismatch** by registering BOTH schemes:

```/Users/mark/.superset/worktrees/c099fb29-e458-4a78-98f3-beeb1fb4964a/adr-008/src-tauri/crates/uc-daemon-contract/src/api/openapi_meta.rs
use utoipa::openapi::security::{ApiKey, ApiKeyValue, SecurityScheme, SecurityRequirement};
use utoipa::openapi::OpenApi;

/// info + servers + tags + dual security scheme. Applied by the webserver's
/// `#[derive(OpenApi)] ApiDoc` via `modifiers(&ContractMeta)`.
pub fn apply_metadata(doc: &mut OpenApi) {
    let comps = doc.components.get_or_insert_with(Default::default);

    // FIX documented-vs-real auth transport mismatch: register BOTH.
    // Primary (browser/GUI): ?auth=Session <token> query param.
    comps.add_security_scheme(
        "session_query",
        SecurityScheme::ApiKey(ApiKey::Query(ApiKeyValue::new("auth"))),
    );
    // Native Rust client: Authorization: Session <token> header.
    comps.add_security_scheme(
        "session_header",
        SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::new("Authorization"))),
    );

    // Skip L1/public + dev operations (they must not inherit session security).
    const PUBLIC_PATHS: &[&str] = &["/health", "/auth/connect"];
    for (path, item) in doc.paths.paths.iter_mut() {
        if PUBLIC_PATHS.contains(&path.as_str()) { continue; }
        for op in item.operations.values_mut() {
            let reqs = op.security.get_or_insert_with(Default::default);
            reqs.push(SecurityRequirement::new("session_query", std::iter::empty::<String>()));
            reqs.push(SecurityRequirement::new("session_header", std::iter::empty::<String>()));
        }
    }
    // info(title/version/description) + servers(base_url placeholder) set here too.
}

pub fn tags() -> Vec<(&'static str, &'static str)> { /* clipboard, device, member, … */ }
```

The webserver wraps it in a `Modify` impl on the `#[derive(OpenApi)] ApiDoc` (which stays in webserver because `paths(...)` references handler fns):

```/Users/mark/.superset/worktrees/c099fb29-e458-4a78-98f3-beeb1fb4964a/adr-008/src-tauri/crates/uc-webserver/src/api/openapi.rs
struct ContractMeta;
impl utoipa::Modify for ContractMeta {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        uc_daemon_contract::api::openapi_meta::apply_metadata(openapi);
    }
}
// #[openapi(modifiers(&ContractMeta), paths(...), components(...), tags(...))]
```

### C.6 Auth-connect dedup (`dto/auth.rs`, NEW in contract)

`ConnectRequest`/`ConnectResponse` exist as **three** inline copies (webserver `security/connect.rs`, webserver `dev.rs` `DevTokenResponse`, `uc-daemon-client/src/http/mod.rs`). Introduce one shared contract DTO `ConnectRequest` + `SessionTokenResponse { session_token, expires_in_secs, refresh_at_secs }` (camelCase, ToSchema). `/auth/connect` keeps its **bare flat** shape (the native Rust decoder strict-decodes it; do NOT envelope it). `DevTokenResponse` can become a thin alias or stay dev-local.

### C.7 Pairing DTO graveyard cleanup

`dto/pairing.rs` defines 9 DTOs but only `UnpairDeviceRequest` is reachable from a mounted route. The other 8 (`InitiatePairingRequest`, `VerifyPairingRequest`, `PairingSessionCommandRequest`, `SetPairingDiscoverabilityRequest`, `SetPairingParticipantRequest`, `AckedPairingCommandResponse`, `InitiatePairingResponse`, `PairingApiErrorResponse`, `PairingSessionSummaryDto`) are deletion candidates after a workspace-wide usage grep. Not blocking; flag it.

### C.8 setup-v2 (bare bodies — decision point)

All 8 `dto/v2/setup.rs` DTOs already derive `ToSchema` and are returned bare (`Json(dto)`). FE `setupV2.ts` reads fields top-level AND its error classifiers branch on raw English `message` text. **Recommendation:** keep setup-v2 bare for this work (only the two missing `paths()`/`schemas()` registrations for `switch_space` + `query_migration_progress` and their DTOs). Enveloping setup-v2 is a coordinated FE break, deferred.

### C.9 What STAYS in `uc-webserver`

- **All `#[utoipa::path]` annotations** (on handler fns, reference webserver-local param structs).
- **`struct ApiDoc`** (`#[derive(OpenApi)]` with the `paths(...)` handler list) — pulls metadata from contract via `modifiers(&ContractMeta)`.
- **`struct ApiDocDev` + `DevTokenResponse` + `dev_token_handler`** under `#[cfg(debug_assertions)]`, served at `/api-docs/openapi-dev.json`. Dev-only types never enter contract.
- **`ApiError` + `IntoResponse` + `log_facade_failure`** (axum-coupled).
- **Query-param structs** with `#[derive(IntoParams)]`: `PaginationParams`, `SearchQueryParams`, `RestoreQuery`, `DevTokenQuery`.
- **`dto/search.rs` + `dto/mod.rs` re-export façade**.

---

## D. Tags & operationIds

### Tag taxonomy

**Flat, single-level** (utoipa v4 = one tag per operation; `@hey-api` groups SDK files by tag string). Eleven production tags + `setup-v2` + dev-only `dev`.

| Tag | Description | Doc membership |
| --- | --- | --- |
| `clipboard` | Entry CRUD, stats, resources, binary blobs/thumbnails, history actions (clear/restore), delivery (dispatch/resend/cancel) | `ApiDoc` |
| `search` | Query, index status, index rebuild | `ApiDoc` |
| `storage` | Storage stats + cache maintenance | `ApiDoc` |
| `device` | Local device identity (`/device/me`) | `ApiDoc` |
| `member` | Per-space-member sync preferences | `ApiDoc` |
| `pairing` | Space-member unpair lifecycle | `ApiDoc` |
| `encryption` | Encryption state + session lock/unlock | `ApiDoc` |
| `settings` | Persisted settings read/update (no OS side effects) | `ApiDoc` |
| `lifecycle` | Daemon lifecycle state, retry, ready-signal | `ApiDoc` |
| `upgrade` | Version upgrade detection + acknowledgement | `ApiDoc` |
| `system` | Diagnostics & topology: health, status, peer/member snapshots, presence refresh, websocket | `ApiDoc` (`getHealth` is L1/public) |
| `setup-v2` | Stateless v2 space-setup & invitation flow | `ApiDoc` |
| `dev` | Dev-only auth bypass token — debug builds only | `ApiDocDev` |

Consolidation: binary blob/thumbnail fold into `clipboard`; `/ws` folds into `system`; the double-listed `/peers`/`/paired-devices`/`/presence/refresh` get the single tag `system`; `/auth/connect` is folded into `system` (avoids a near-empty `auth` client file) but is L1 and excluded from session security.

### operationId convention

`operationId = <verb><Noun>[<Qualifier>]`, **camelCase, verb-first**, set **explicitly** via `operation_id` in each `#[utoipa::path]` (do NOT rely on utoipa's fn-name derivation → snake_case). Fixed verb vocabulary: `list`/`get`/`create`/`update`/`delete` + domain verbs (`clear`, `toggle`, `dispatch`, `resend`, `cancel`, `restore`, `rebuild`, `refresh`, `unlock`, `lock`, `redeem`, `switch`, `retry`, `signal`, `ack`, `verify`, `connect`, `query`, `initialize`, `issue`, `reset`). No redundant domain prefix unless disambiguating (`getStorageStats` vs `getClipboardStats`; `searchQuery` because `query` alone is ambiguous). Globally unique across `ApiDoc` + `ApiDocDev` (dev endpoint = `authDevToken`, never collides with `authConnect`).

The full method→path→tag→operationId mapping is the operationId column of the §A inventory table (frozen contract once landed — renaming any later is a breaking change to `src/api/generated/`).

`Δ` deviations from the inventory's `operation_id_suggestion`: `getClipboardEntry` (was getClipboardEntryDetail), `getClipboardBlob`/`getClipboardThumbnail` (were getBlob/getThumbnail), `listPeers`/`listPairedDevices`/`refreshPresence` (de-duplicated single routes), `setupV2GetMigrationProgress` (was setupV2QueryMigrationProgress — aligned to `get` for a GET), `authDevToken` (was devToken — namespaced).

L1/public skip: `getHealth` and `authConnect` must not carry the session scheme (handled by the `PUBLIC_PATHS` allowlist in §C.5). `authConnect` uses `Authorization: Bearer <token>` (the local daemon secret), a different scheme; document it as Bearer.

---

## E. gen-openapi Binary & Schema Export

### Determinism is free in this stack

No sorting / re-serialization pass is needed for clean git diffs — two layers guarantee byte-stable output:

1. **utoipa map ordering.** `utoipa 4.2.3` is built **without** `preserve_path_order`, so `Paths.paths` is a `BTreeMap<String, PathItem>` (emitted sorted by path) and `Components.{schemas,responses,security_schemes}` are `BTreeMap` (always alphabetically sorted). Reordering the `components(schemas(...))` list does NOT churn the JSON.
2. **serde_json struct ordering.** `serde_json 1.0.149` is built **without** `preserve_order` (no `indexmap` in the lockfile), so struct fields serialize in fixed definition order.

**Conclusion:** `ApiDoc::openapi().to_pretty_json()` (= `serde_json::to_string_pretty`, 2-space indent) is already deterministic. **Do NOT add `serde_json`'s `preserve_order` feature** anywhere (it makes key order follow insertion order = less stable), and do not add a custom `BTreeMap` round-trip.

### Scope

Generate exactly `ApiDoc::openapi()` (production L2+ surface). **Do NOT merge `ApiDocDev`** (the dev `/auth/dev-token` doc) — it is `#[cfg(debug_assertions)]`-gated, so including it would make the committed artifact build-profile-dependent.

### Location & commit policy

`schema/` sits at **repo root** (sibling of `src-tauri/`, `src/`, `package.json`). It does not exist yet — the bin creates it via `create_dir_all`. Output: `schema/openapi.json`. **Commit it** (no `.gitignore` excludes it; it is the reviewable contract diff and the hey-api input).

### Bin code

A file under `src/bin/` is an **implicit** binary target named `gen-openapi` — no `[[bin]]` entry required. It links the crate's `lib` (`uc_webserver`, which re-exports `pub mod api`; `ApiDoc` is `pub`); `utoipa` and `serde_json` are already deps. No dependency changes.

```/Users/mark/.superset/worktrees/c099fb29-e458-4a78-98f3-beeb1fb4964a/adr-008/src-tauri/crates/uc-webserver/src/bin/gen-openapi.rs
//! Generates the canonical OpenAPI document at repo-root `schema/openapi.json`.
//!
//! Output is deterministic: utoipa stores paths/components in `BTreeMap`s and
//! `serde_json` is built without `preserve_order`, so re-running on any machine
//! yields a byte-identical file -> clean git diffs.
//!
//! Run: `cargo run -p uc-webserver --bin gen-openapi`

use std::path::PathBuf;
use std::{fs, io};

use utoipa::OpenApi;
use uc_webserver::api::openapi::ApiDoc;

fn main() -> io::Result<()> {
    // Production L2+ surface only. ApiDocDev is debug-gated and excluded so the
    // committed artifact is build-profile-independent.
    let doc = ApiDoc::openapi();

    let mut json = doc
        .to_pretty_json()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    json.push('\n'); // POSIX-clean trailing newline

    // CARGO_MANIFEST_DIR = .../src-tauri/crates/uc-webserver
    // repo root = manifest_dir/../../.. ; schema dir = repo_root/schema
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .ancestors()
        .nth(3)
        .expect("manifest dir has 3 ancestors up to repo root");
    let schema_dir = repo_root.join("schema");
    let out_path = schema_dir.join("openapi.json");

    fs::create_dir_all(&schema_dir)?;
    fs::write(&out_path, json)?;
    eprintln!("wrote {}", out_path.display());
    Ok(())
}
```

### Invocation & scripts

Run from inside `src-tauri/` (repo pins `rust-toolchain` there): `cargo run -p uc-webserver --bin gen-openapi`. `package.json` gets `"gen:openapi"` as the upstream half of the locked `gen:api` pipeline (`gen:openapi` → `gen:client`). See §F for the exact script trio.

### CI guard

```bash
cd src-tauri && cargo run -p uc-webserver --bin gen-openapi
git diff --exit-code schema/openapi.json
```

Deterministic output ⇒ any diff means a handler/DTO annotation changed without regenerating — a real review signal.

---

## F. Frontend @hey-api/openapi-ts Bridge

### Verified toolchain

- Package **`@hey-api/openapi-ts`**, verified latest stable **`0.97.3`** (Hey API is pre-1.0; its own guidance is to pin exactly). **Pin exactly** (no caret) in `devDependencies`.
- **No separate runtime dependency.** The fetch runtime is emitted into `src/api/generated/` (a `core/` + `client/client.gen.ts` tree) when the `@hey-api/client-fetch` plugin is enabled; `@hey-api/client-fetch` is a plugin name in the config, not a `dependencies` entry.
- Verified from the generated `client.gen.ts`: request interceptors run as `request = await fn(request, opts)` (`Request` in / `Request` out); response interceptors as `response = await fn(response, request, opts)` — so `response.status === 401` is observable. This is load-bearing for the auth bridge.

### Config (`openapi-ts.config.ts`, repo root)

```/Users/mark/.superset/worktrees/c099fb29-e458-4a78-98f3-beeb1fb4964a/adr-008/openapi-ts.config.ts
import { defineConfig } from '@hey-api/openapi-ts'

export default defineConfig({
  // Local spec produced by the gen-openapi cargo bin (offline, reproducible).
  input: './schema/openapi.json',
  output: {
    path: 'src/api/generated',
    format: 'prettier',
    lint: false,
  },
  plugins: [
    '@hey-api/client-fetch',  // fetch runtime emitted into output/core + output/client
    '@hey-api/typescript',
    '@hey-api/sdk',
  ],
})
```

`devDependencies` entry:

```/Users/mark/.superset/worktrees/c099fb29-e458-4a78-98f3-beeb1fb4964a/adr-008/package.json
"@hey-api/openapi-ts": "0.97.3"
```

### Auth bridge (the critical decision)

Auth is overridden via a **request interceptor**, NOT the SDK's built-in `auth()` callback. The daemon authenticates via the **query param** `?auth=Session <token>` (`client.ts:217-218`, `blobUrl()` 166-179). The generated `setAuthParams` *can* place an apiKey into the query, but it sources the token from a static callback and cannot drive the async refresh + single-retry, which must be coordinated centrally in `daemonClient`.

```/Users/mark/.superset/worktrees/c099fb29-e458-4a78-98f3-beeb1fb4964a/adr-008/src/api/daemon/generated-bridge.ts
import { client as generatedClient } from '@/api/generated/client.gen'
import { daemonClient } from './client'

/** Wire the @hey-api generated fetch client to the daemon session lifecycle.
 *  Call once after daemonClient.initialize(config). */
export function installGeneratedClientBridge(baseUrl: string): void {
  // (a) baseUrl injection.
  generatedClient.setConfig({ baseUrl })

  // (b) Inject auth as ?auth=Session <token> QUERY param (NOT a header).
  //     Request.url is read-only -> rebuild the Request with the rewritten URL.
  generatedClient.interceptors.request.use((request: Request) => {
    const token = daemonClient.currentSession?.token
    if (!token) return request
    const url = new URL(request.url)
    url.searchParams.set('auth', `Session ${token}`)
    return new Request(url, request)
  })
}
```

The 401 single-refresh stays centralized in `daemonClient` via a small `callSdk` helper that mirrors the existing `request<T>()` control flow (pre-emptive refresh + one-shot 401 retry); the request interceptor re-reads the freshly refreshed token on the retry automatically:

```/Users/mark/.superset/worktrees/c099fb29-e458-4a78-98f3-beeb1fb4964a/adr-008/src/api/daemon/client.ts
// Add inside class DaemonClient (mirrors request<T>()):
async callSdk<T>(
  fn: () => Promise<{ data: T; response: Response }>
): Promise<T> {
  if (!this.config) {
    throw new DaemonApiError(DaemonErrorCode.INTERNAL_ERROR, 'DaemonClient not initialized')
  }
  if (isSessionExpired(this.session)) {
    await this.refreshSession()
  }
  try {
    const { data } = await fn()      // throwOnError SDK: rejects on non-2xx
    return data
  } catch (err) {
    if (this.isUnauthorized(err)) {  // err.response?.status === 401
      this.session = null
      await this.refreshSession()
      const { data } = await fn()    // single retry
      return data
    }
    throw err
  }
}
```

SDK fns are always called with `{ throwOnError: true }`. The Rust-side `SecurityAddon → ApiKey::Query("auth")` change (§C.5) makes the spec match reality; the bridge does not depend on which scheme the spec emits.

### Scripts

```/Users/mark/.superset/worktrees/c099fb29-e458-4a78-98f3-beeb1fb4964a/adr-008/package.json
"gen:openapi": "cd src-tauri && cargo run -p uc-webserver --bin gen-openapi",
"gen:client": "openapi-ts",
"gen:api": "npm run gen:openapi && npm run gen:client"
```

### P6 settings migration sketch

Keep `getSettings(): Promise<Settings>` and `updateSettings(partial): Promise<{success, restartRequired}>` byte-for-byte. Internally: delete the hand-written `Settings`/`*Dto` interfaces and re-export the generated equivalents (so the `src/api/daemon/index.ts` barrel keeps resolving); route through generated `getSettingsHandler`/`updateSettingsHandler`; unwrap `{data,ts}` inside the wrapper (GET returns `data`; PUT reads top-level `success`/`restartRequired`). **Keep `toSettingsPatchRequest` exactly as-is** (its `autoStart`-exclusion and "omit undefined = no-change" semantics are domain rules the generated types do not encode). Existing settings `__tests__` should pass unmodified (they target the public functions, not transport). Other domains follow the same wrapper-over-SDK pattern in later PRs (P6 scoped to settings only).

---

## G. P1–P6 Execution Plan

Each phase ends at a **single validation gate**: `cargo check` (run from inside `src-tauri/`) for any Rust-touching phase, and `tsc` / `npm run typecheck` for any frontend-touching phase. Fan-out parallelism is per-domain where the work is independent and the cargo/tsc gate is the join point.

### P1 — Contract foundation (envelope + error + metadata, no wire change)

**Touches (contract):** `dto/envelope.rs` (NEW, `ApiEnvelope<T>` + aliases), `dto/error.rs` (NEW, moved `ApiErrorResponse`), `openapi_meta.rs` (NEW, metadata + dual SecurityAddon), `dto/auth.rs` (NEW, connect dedup), `dto/storage.rs` (NEW), `dto/mod.rs` + `api/mod.rs` (module wiring), `api/types.rs` (add `ToSchema` to diagnostic structs), `dto/clipboard_command.rs` + `dto/encryption.rs` (add `ToSchema` + new DTOs). **Touches (webserver):** `dto/error.rs` (re-export contract `ApiErrorResponse`, keep `ApiError`/`IntoResponse`), `api/openapi.rs` (`ContractMeta` `Modify`), inline storage DTOs removed (now imported from contract).
**Fan-out:** the per-domain `ToSchema` additions in `types.rs`/`clipboard_command.rs`/`encryption.rs` are independent and parallelizable.
**No wire change** — this phase only changes Rust types and the doc, not runtime JSON.
**Gate:** `cargo check` (whole workspace, since contract is depended on widely) — the join point for all fan-out.

### P2 — Annotate the documented-but-incomplete + missing-utoipa endpoints (no wire change for already-`{data,ts}` domains)

**Touches (webserver):** add/complete `#[utoipa::path]` on handlers that lack it or have untyped bodies — search (`searchQuery`/`getSearchStatus`/`rebuildSearchIndex`), storage (`getStorageStats`/`clearStorageCache`), lifecycle (3), encryption (type the 4 bodies via aliases), upgrade (swap bespoke wrappers to alias bodies if desired), `system` diagnostics (`getHealth`/`getStatus`/`listPeers`/`listPairedDevices`/`refreshPresence`/`websocketUpgrade`), `authConnect`. Register the two missing setup-v2 ops (`setupV2SwitchSpace`, `setupV2GetMigrationProgress`) + their DTOs in `ApiDoc.paths()`/`components()`. Fix settings schema-registration gaps (`QuickPanelSettingsDto`, `*PatchDto`, request_body on PUT). Add `#[derive(IntoParams)]` to query-param structs. Set explicit `operation_id` + `tag` per §D on every operation. Add the `PUBLIC_PATHS` allowlist so L1 ops skip session security.
**Fan-out:** per-domain annotation work is independent; each domain is one parallel unit.
**Wire change:** none for already-`{data,ts}` domains (encryption/upgrade stay identical on the wire; only the doc gains typed bodies). Bare/breaking domains are NOT migrated here — only documented to reflect *current* shape.
**Gate:** `cargo check`.

### P3 — gen-openapi bin + commit the schema

**Touches:** `src-tauri/crates/uc-webserver/src/bin/gen-openapi.rs` (NEW), `package.json` (`gen:openapi` script), `schema/openapi.json` (NEW, generated + committed).
**Fan-out:** none (single artifact).
**Gate:** `cargo run -p uc-webserver --bin gen-openapi` succeeds AND the inspect step confirms each `ApiEnvelope<...>` alias rendered a real `$ref` component (not an inline), especially the `Vec<...>` aliases. Then `cargo check` (bin compiles).

### P4 — Frontend codegen bridge (generated client, not yet consumed)

**Touches:** `openapi-ts.config.ts` (NEW), `package.json` (`@hey-api/openapi-ts@0.97.3` devDep + `gen:client`/`gen:api` scripts), `src/api/generated/` (NEW, generated tree, committed), `src/api/daemon/generated-bridge.ts` (NEW), `src/api/daemon/client.ts` (`callSdk` helper + `installGeneratedClientBridge` call in `initialize`).
**Fan-out:** none (generation is one step; bridge is one file).
**Gate:** `npm run gen:api` succeeds AND `tsc`/`npm run typecheck` passes (generated types compile, bridge typechecks). No runtime behavior change yet — nothing routes through the SDK.

### P5 — Settings domain wire normalization (Rust side, the first deliberate non-breaking confirmation)

**Touches (webserver):** confirm `GET /settings` uses `SettingsEnvelope` alias body; confirm `PUT /settings` keeps `UpdateSettingsResponse` (bespoke wrapper with `success`+`restartRequired` top-level — NOT collapsed). Regenerate `schema/openapi.json` + `src/api/generated/`.
**Fan-out:** none.
**Wire change:** none (settings is already `{data,ts}`; this phase verifies the generated client matches and pins the schema diff).
**Gate:** `cargo check` + `cargo run --bin gen-openapi` + `npm run gen:api` clean (no schema drift), then `tsc`.

### P6 — Route settings FE consumers through the generated SDK (scoped wire-consumption migration)

**Touches (frontend):** `src/api/daemon/settings.ts` (route `getSettings`/`updateSettings` through `getSettingsHandler`/`updateSettingsHandler` via `daemonClient.callSdk`; delete hand-written interfaces, re-export generated types; keep `toSettingsPatchRequest` verbatim; keep public signatures). `src/api/daemon/index.ts` barrel re-exports unchanged.
**Fan-out:** none (single domain, per locked decision 5).
**Gate:** `tsc`/`npm run typecheck` + existing settings `__tests__` pass unmodified (they target the public functions, not transport).

> **⚠️ VOID (superseded by §0.2/§0.6):** the earlier "deferred" scoping no longer applies. ALL breaking migrations above are executed in this cycle per the revised phase plan in §0.6 (P2 server wire + P3 native Rust lockstep + P6 all JS consumers). The §G phase paragraphs P1–P6 retain useful per-domain detail, but their *scope/ordering* is governed by §0.6.

---

## H. Breaking Changes & Frontend Impact

Every endpoint whose envelope/shape changes **if** normalized to `{data,ts}`, with the exact consumers that must change in lockstep. Endpoints already on `{data,ts}` (clipboard history, settings GET, encryption, upgrade, device/me, member GET, search status/rebuild, storage) are **non-breaking** and omitted.

| Endpoint | Change if normalized | Frontend / Rust files affected |
| --- | --- | --- |
| `POST /clipboard/dispatch` | bare → `ApiEnvelope<DispatchOutcomeResponse>` | `src-tauri/crates/uc-daemon-client/src/http/clipboard.rs` (`dispatch_text`, strict `response.json::<DispatchOutcomeResponse>()`); CLI `src-tauri/crates/uc-cli/src/send.rs`; no JS consumer |
| `POST /clipboard/resend` | bare → `ApiEnvelope<ResendResponse>` | `src-tauri/crates/uc-daemon-client/src/http/clipboard.rs` (`resend_entry`); GUI Tauri path `src/api/tauri-command/clipboard_delivery.ts`, `useResendAction.ts` |
| `POST /clipboard/cancel-transfer/{transfer_id}` | bare → `ApiEnvelope<CancelTransferResponse>` | `src-tauri/crates/uc-daemon-client/src/http/clipboard.rs` (`cancel_transfer`, strict decode); JS `src/api/file_transfer.ts` (body discarded → tolerant) |
| `GET /clipboard/blobs/{blob_id}` | stays binary; would break if enveloped | `src/api/daemon/client.ts` `blobUrl()`; `src/api/clipboardItems.ts` (`fetchResourceText`, `resolveResourceImageUrl`). Tests: `src/api/__tests__/clipboardItems.test.ts`, `src/api/daemon/__tests__/client.test.ts` |
| `GET /clipboard/thumbnails/{rep_id}` | stays binary; would break if enveloped | Same as blobs; `src/components/clipboard/__tests__/ClipboardItem.test.tsx`, `src/quick-panel/__tests__/useClipboardPreview.test.tsx` |
| `POST /clipboard/restore/{entry_id}` | ad-hoc `{success:true}` → `ApiEnvelope<RestoreEntryResponse>`; ad-hoc errors → `ApiErrorResponse` | `src/api/daemon/clipboard.ts` (`restoreClipboardEntry`, ignores body); `src/api/clipboardItems.ts:356`; Rust `uc-daemon-client/src/http/clipboard.rs:45`. **410 `payload_unavailable` context (`entry_id`/`rep_id`/`state`) load-bearing for `DaemonErrorCode.PAYLOAD_UNAVAILABLE` UX — must preserve** |
| `GET /search/query` | `{data,total,hasMore,ts}` → strict `{data,ts}` (move `total`/`hasMore` into `data`) | `src/components/clipboard/ClipboardContent.tsx:285` (`response.total`); `src/quick-panel/hooks/useHistorySearch.ts:184` (`response.total`) |
| `GET /peers` | bare-array → `ApiEnvelope<Vec<PeerSnapshotDto>>` | Rust `uc-daemon-client/src/http/query.rs:39` (`get_peers`); **no JS HTTP consumer** ("peers" is a WS topic) |
| `GET /paired-devices` | bare-array → `ApiEnvelope<Vec<SpaceMemberDto>>` | `src/api/daemon/members.ts:68` (`getPairedPeers`, returns array raw), `src/store/slices/devicesSlice.ts`, `src/pages/DevicesPage.tsx`; Rust `uc-daemon-client/src/http/query.rs:43` |
| `POST /presence/refresh` | bare → `ApiEnvelope<PresenceRefreshResponse>` | `src/api/daemon/presence.ts:29` (reads counters top-level); `src/pages/DevicesPage.tsx:110` |
| `PUT /settings` | breaks ONLY if `success`/`restartRequired` moved into `data` | `src/api/daemon/settings.ts:272` (reads both top-level). **Decision: keep them top-level (bespoke wrapper); non-breaking** |
| `GET /lifecycle/status` | bare `{state}` → `ApiEnvelope<LifecycleStatusResponse>` | `src/api/daemon/lifecycle.ts` (`getLifecycleStatus` reads `dto.state`), `src/hooks/useLifecycleStatus.ts`, `src/api/types.ts:8`; tests `src/api/__tests__/lifecycle.test.ts`, `src/__tests__/api/daemon/lifecycle.test.ts` |
| `GET /health` | bare `{status,...}` → `ApiEnvelope<HealthResponse>` | `src/lib/daemon-auth.ts:101` (reads `health.status`); test `src/__tests__/lib/daemon-client.test.ts` |
| `GET /status` | bare → `ApiEnvelope<StatusResponse>` | Rust `uc-daemon-client/src/http/query.rs:47` (`get_status`); no JS consumer |
| `POST /auth/connect` | flat `{sessionToken,...}` → would break native decoder if enveloped | Rust `uc-daemon-client/src/http/mod.rs:91-102` (strict flat decode). **Decision: keep bare flat; do not envelope** |
| `GET /ws` | stays protocol upgrade + `DaemonWsEvent`; would break if enveloped | `src/lib/daemon-ws.ts:285-318` (reads `raw.topic`/`raw.type`/`raw.payload`); `src/hooks/useDaemonEvents.ts`, `src/lib/daemon-ws-bootstrap.ts`. **Exempt** |
| `POST /v2/setup/initialize` | bare → enveloped | `src/api/daemon/setupV2.ts:345` (reads fields top-level); `classifyInitializeError` (raw `message` text) |
| `POST /v2/setup/issue-invitation` | bare → enveloped | `src/api/daemon/setupV2.ts:358` |
| `POST /v2/setup/redeem` | bare → enveloped | `src/api/daemon/setupV2.ts:381`; `classifyRedeemError` depends on raw English `message` (`expired`, `declined`) |
| `GET /v2/setup/state` | bare → enveloped | `src/api/daemon/setupV2.ts:408` |
| `POST /v2/setup/switch-space` | bare → enveloped | `src/api/daemon/setupV2.ts:430`; `classifySwitchSpaceError` depends on raw English `message` (`first-time setup`, `still in flight`, `locked`, `declined`, `did not recognise`, `corrupted ciphertext`, `key material`) |
| `GET /v2/setup/migration-progress` | bare → enveloped | `src/api/daemon/setupV2.ts:447`; `MigrationPhase` TS union hardcodes snake_case enum values |

**Note (per §0.2):** ALL of the above breaking migrations ARE executed in this cycle (the prior "none except settings" scoping is void). This table is the authoritative lockstep work-list for P2 (server wire), P3 (native Rust: `uc-daemon-client`/`uc-cli`), and P6 (JS consumers). `/auth/connect` is ALSO enveloped (§0.2). Binary (`blobs`/`thumbnails`) and `/ws` remain exempt. Error-body normalization must preserve `ApiErrorResponse` `code` + the exact English `message` strings (setup-v2 + restore-410 classifiers); the restore-410 context moves into the new optional `details` field (§0.3).

---

## I. Open Questions for User Sign-off

> **RESOLVED 2026-06-02 — see §0 for the binding answers.** Q1 (envelope)→**pure generic, no bespoke wrappers**; Q2 (auth scheme)→**register BOTH** query+header; Q3 (restore-410)→**add optional `details`** to `ApiErrorResponse` (option b); Q4 (scope)→**ALL breaking migrations this cycle**; Q5 (commit)→**commit both + CI drift-check**; Q6 (pairing graveyard)→**grep + delete dead DTOs in P1**. The recommendations below are the pre-decision analysis, retained for rationale only, and are SUPERSEDED where they differ from §0.

1. **(Lead) Envelope representation — generic vs named.** Recommendation: **hybrid** — one generic `ApiEnvelope<T>` defined once in the contract, surfaced to OpenAPI exclusively via `#[aliases(...)]` (one named alias per payload), keeping bespoke wrappers only for the irregular endpoints (PUT settings, PATCH member, paginated search query). This is forced by utoipa v4 (a bare generic cannot be registered; an un-aliased generic inlines anonymous schemas → ugly non-reusable hey-api TS). **Sign-off needed**: accept the hybrid, and confirm the alias names (they become the public TS type names — e.g. `SettingsEnvelope` vs reusing the FE's existing `SettingsGetResponse` naming to minimize P6 churn).

2. **Auth security scheme: header vs query (vs both).** The doc currently declares `ApiKey::Header("Authorization")` but the browser sends `?auth=Session <token>` as a query param. Recommendation: register **both** (`session_query` primary + `session_header` for the native Rust client) and batch-apply as alternatives. Confirm — this is doc-wide and blocks a faithful hey-api bridge (the FE override is via interceptor regardless, so the doc scheme is cosmetic for the FE, but the spec should match reality).

3. **restore 410 `payload_unavailable` context** (`entry_id`/`rep_id`/`state`). `ApiErrorResponse` only has `{code,message}`. Choose: (a) encode context into `message`, (b) add optional `details: Option<serde_json::Value>` to `ApiErrorResponse` (additive, non-breaking, but touches every error consumer schema-wide), or (c) keep restore's 410 as a documented per-endpoint error variant. Recommendation: **(c) or (a)** to avoid widening the canonical error type.

4. **Scope of breaking migrations.** This spec scopes P1–P6 to: contract foundation, full documentation, the gen-openapi bin, the hey-api bridge, and the (non-breaking) settings consumption migration. The genuinely breaking endpoints (dispatch/resend/cancel-transfer with lockstep `uc-daemon-client`, bare-array `/peers`/`/paired-devices`, bare lifecycle/status, bare setup-v2, `/health`, `/auth/connect`) are deferred to future coordinated phases. Confirm this scoping, OR specify which breaking domains to fold into this cycle (each requires a lockstep Rust-client + FE change).

5. **Commit the generated trees?** Recommendation: **commit both** `schema/openapi.json` and `src/api/generated/` so CI/typecheck don't require running cargo + codegen, paired with a CI drift-check (`gen:api` then `git diff --exit-code`). Confirm, vs gitignoring and regenerating in CI.

6. **Pairing DTO graveyard** (`dto/pairing.rs`): delete the 8 dead DTOs now (after a usage grep), or defer? Not blocking the normalization but flagged.
