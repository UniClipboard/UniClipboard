# Phase 58: Extract DTO models and pairing event types from uc-tauri to uc-app and uc-core - Context

**Gathered:** 2026-03-25
**Status:** Ready for planning
**Revision:** R3 (post Codex review R2 — narrowed clipboard scope, field reconciliation noted)

<domain>
## Phase Boundary

Simplify DTO ownership between uc-tauri and uc-app by: (1) unifying truly duplicate clipboard DTOs (add serde to uc-app, delete duplicates in uc-tauri), (2) extracting pairing aggregation DTOs to uc-app, (3) investigating and cleaning up pairing event types. After this phase, uc-tauri has zero duplicate DTO definitions — only Tauri-specific command response types and wiring.

**In scope:**

- Unify `EntryProjectionDto` (uc-app) with `ClipboardEntryProjection` (uc-tauri): add serde derives + reconcile field differences (`link_domains` vs `file_transfer_ids`), then delete uc-tauri duplicate
- Unify `ClipboardStats` if uc-app version can serve as the wire type
- Extract pairing aggregation DTOs (`P2PPeerInfo`, `PairedPeer`) from uc-tauri/commands/pairing.rs to uc-app
- Investigate `P2PPairingVerificationEvent`/`P2PPairingVerificationKind` — determine if stale (host_event_emitter.rs may already have canonical wire payload) and either extract to uc-app or delete
- Clean up any residual `DaemonPairingRequestError` definition in uc-tauri (already correctly placed in uc-daemon-client)
- Update all import paths (direct delete, no re-export stubs)

**Explicitly NOT in scope (keep in uc-tauri):**

- 8 Tauri-only command response types: `ClipboardEntriesResponse`, `ClipboardEntryDetail`, `ClipboardEntryResource`, `ClipboardItemDto`, `ClipboardTextItemDto`, `ClipboardImageItemDto`, `ClipboardLinkItemDto`, `ClipboardItemResponse`, `LifecycleStatusDto` — these are pure frontend wire contracts with no uc-app counterparts
- Pairing request/response DTOs (`P2PPairingRequest`, `P2PPairingResponse`, `P2PPinVerifyRequest`) — Tauri command protocol types
- New functionality, frontend code changes, business logic changes
- specta::Type handling (Tauri commands manage specta at command level)

</domain>

<decisions>
## Implementation Decisions

### Clipboard DTO unification

- **D-03:** Only unify types that have true duplicates in uc-app. Currently confirmed duplicates: `ClipboardEntryProjection` ↔ `EntryProjectionDto`, and `ClipboardStats` (both crates). Add `#[derive(Serialize, Deserialize)]` + necessary serde annotations to uc-app types, then delete the uc-tauri duplicates. **Field reconciliation required:** `EntryProjectionDto` has `file_transfer_ids: Vec<String>` while `ClipboardEntryProjection` has `link_domains: Option<Vec<String>>` and `#[serde(skip_serializing_if)]` annotations. The unified type must preserve the existing frontend JSON contract. Claude's discretion on approach: extend uc-app type to match wire contract, or keep a thin adapter function in uc-tauri commands.

### Pairing DTO extraction

- **D-01:** `P2PPeerInfo` and `PairedPeer` → uc-app (application layer, alongside existing `P2pPeerSnapshot` and `LocalDeviceInfo`)

### Pairing event types

- **D-02:** `P2PPairingVerificationEvent` and `P2PPairingVerificationKind` — **Researcher must investigate** before planning: `host_event_emitter.rs` already has an internal `PairingVerificationPayload` that handles Tauri emit. If `events/p2p_pairing.rs` types are stale/unused externally, delete them. If actively used by other consumers, extract to uc-app. uc-core already has serde-free `PairingVerificationKind`/`PairingHostEvent` — do NOT duplicate into uc-core.

### Error types

- **D-06:** `DaemonPairingRequestError` is already correctly placed in uc-daemon-client. Only clean up any residual definition or import in uc-tauri.

### Migration strategy

- **D-05:** Direct delete + update all imports. No re-export stubs. Clean cut.

### Claude's Discretion

- Whether to extend `EntryProjectionDto` with `link_domains` + serde annotations vs. keep a thin mapping function in uc-tauri
- Exact module organization within uc-app for extracted pairing DTOs
- Whether `P2PPairingVerificationEvent` types are stale and should be deleted vs. extracted
- Order of extraction plans
- Whether uc-tauri's 8 remaining response types need any adjustments after unifying the shared types they reference

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Clipboard DTO unification

- `src-tauri/crates/uc-tauri/src/models/mod.rs` — Wire DTOs; only `ClipboardEntryProjection` and (partially) `ClipboardStats` are true duplicates
- `src-tauri/crates/uc-app/src/usecases/clipboard/list_entry_projections/list_entry_projections.rs` — `EntryProjectionDto` definition (needs serde + field reconciliation)
- `src-tauri/crates/uc-app/src/usecases/clipboard/mod.rs` — `ClipboardStats` definition
- `src-tauri/crates/uc-tauri/src/commands/clipboard.rs` — Primary consumer, currently maps from `EntryProjectionDto` → `ClipboardEntryProjection` (including `link_domains` derivation)
- `src-tauri/crates/uc-tauri/src/commands/lifecycle.rs` — Uses `LifecycleStatusDto`
- `src-tauri/crates/uc-tauri/tests/models_serialization_test.rs` — Serialization contract tests (must pass after changes)
- `src-tauri/crates/uc-tauri/tests/clipboard_commands_stats_favorites_test.rs` — Clipboard command tests
- `src-tauri/crates/uc-tauri/tests/lifecycle_command_contract_test.rs` — Lifecycle command tests

### Pairing DTO extraction

- `src-tauri/crates/uc-tauri/src/commands/pairing.rs` — P2PPeerInfo, PairedPeer definitions + mapping functions + DaemonPairingRequestError usage
- `src-tauri/crates/uc-app/src/usecases/pairing/mod.rs` — Existing pairing exports (P2pPeerSnapshot, LocalDeviceInfo)
- `src-tauri/crates/uc-app/src/usecases/pairing/get_p2p_peers_snapshot.rs` — P2pPeerSnapshot pattern reference
- `src-tauri/crates/uc-tauri/tests/daemon_command_shell.rs` — Imports PairedPeer

### Pairing event types (investigation needed)

- `src-tauri/crates/uc-tauri/src/events/p2p_pairing.rs` — P2PPairingVerificationEvent/Kind — may be stale
- `src-tauri/crates/uc-tauri/src/events/mod.rs` — Event module exports
- `src-tauri/crates/uc-tauri/src/adapters/host_event_emitter.rs` — Has internal `PairingVerificationPayload`, may already be the canonical wire type
- `src-tauri/crates/uc-core/src/ports/host_event_emitter.rs` — Serde-free `PairingVerificationKind`/`PairingHostEvent` (DO NOT duplicate)

### Error types

- `src-tauri/crates/uc-daemon-client/src/http/pairing.rs` — DaemonPairingRequestError (canonical location)
- `src-tauri/crates/uc-daemon-client/src/lib.rs` — Re-exports DaemonPairingRequestError

</canonical_refs>

<code_context>

## Existing Code Insights

### Key Field Differences (must reconcile)

| Field               | EntryProjectionDto (uc-app) | ClipboardEntryProjection (uc-tauri)                |
| ------------------- | --------------------------- | -------------------------------------------------- |
| `file_transfer_ids` | `Vec<String>`               | Not present                                        |
| `link_domains`      | Not present                 | `Option<Vec<String>>`                              |
| serde annotations   | None                        | `#[serde(skip_serializing_if)]` on optional fields |
| link_urls           | `Option<Vec<String>>`       | `Option<Vec<String>>`                              |

The uc-tauri `commands/clipboard.rs` derives `link_domains` from `link_urls` during the mapping step.

### Reusable Assets

- `EntryProjectionDto` in uc-app: Core projection, needs serde + field extension
- `ClipboardStats` in uc-app: Already defined with `total_items` and `total_size`
- `P2pPeerSnapshot` in uc-app: Pattern reference for pairing DTOs

### Established Patterns

- uc-app pairing module exports types via `pub use` in `mod.rs`
- uc-core and uc-app both depend on serde — adding derives is zero new dependencies
- Tauri commands map from uc-app types — after unification, mapping layer eliminated for duplicates

### Integration Points

- `uc-tauri/src/commands/clipboard.rs` — Must switch unified types, keep mapping for non-duplicate fields
- `uc-tauri/src/commands/pairing.rs` — Pairing DTO extraction changes imports
- `uc-tauri/src/adapters/host_event_emitter.rs` — Event type investigation
- `uc-tauri/tests/*` — Multiple test files reference types that may change

</code_context>

<specifics>
## Specific Ideas

- **DTO unification over extraction:** User explicitly chose "simplify" — reduce conversion layers rather than just moving types between crates. Only unify where true duplicates exist; Tauri-only response types stay put.
- **Researcher-gated decision:** D-02 (pairing event types) requires codebase investigation before planning — researcher determines if types are stale.

</specifics>

<deferred>
## Deferred Ideas

### Reviewed Todos (not folded)

- "修复 setup 配对确认提示缺失" — UI bug, unrelated to DTO extraction. Belongs in a separate UI fix phase.

</deferred>

---

_Phase: 58-extract-dto-models-and-pairing-event-types-from-uc-tauri-to-uc-app-and-uc-core_
_Context gathered: 2026-03-25_
_Revised: 2026-03-25 (post Codex review R2 — narrowed scope, field reconciliation documented)_
