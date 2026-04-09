# Phase 58: Extract DTO Models and Pairing Event Types from uc-tauri to uc-app and uc-core - Research

**Researched:** 2026-03-25
**Domain:** Rust crate refactoring — DTO ownership, serde derives, import path updates
**Confidence:** HIGH

## Summary

Phase 58 is a structural cleanup: consolidate duplicate DTO definitions that exist in both `uc-tauri/src/models/` and `uc-app/src/usecases/`. The goal is to eliminate conversion layers for true duplicates, move pairing aggregation DTOs into `uc-app`, and determine the fate of the `P2PPairingVerificationEvent`/`P2PPairingVerificationKind` types in `uc-tauri/src/events/p2p_pairing.rs`.

Investigation confirms three distinct work areas: (1) clipboard DTO unification — two confirmed duplicates (`ClipboardEntryProjection` / `EntryProjectionDto` and `ClipboardStats`), with a field reconciliation requirement for the former; (2) pairing DTO extraction — `P2PPeerInfo` and `PairedPeer` are uc-tauri-only aggregation helpers with no uc-app counterparts, ready for migration; (3) pairing event type investigation — `P2PPairingVerificationEvent`/`P2PPairingVerificationKind` in `uc-tauri/src/events/p2p_pairing.rs` are **stale**: zero external consumers exist. The canonical wire payload is `PairingVerificationPayload` in `host_event_emitter.rs` (module-private struct), and the semantic model is `PairingVerificationKind`/`PairingHostEvent` in `uc-core`. The p2p_pairing.rs types should be deleted.

**Primary recommendation:** Unify `EntryProjectionDto` → `ClipboardEntryProjection` using a thin mapping function in `uc-tauri/commands/clipboard.rs` for the `link_domains` derivation step. Delete uc-tauri duplicates. Extract pairing DTOs to uc-app. Delete stale p2p_pairing.rs event types.

---

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

- **D-03:** Only unify types that have true duplicates in uc-app. Currently confirmed duplicates: `ClipboardEntryProjection` <-> `EntryProjectionDto`, and `ClipboardStats` (both crates). Add `#[derive(Serialize, Deserialize)]` + necessary serde annotations to uc-app types, then delete the uc-tauri duplicates. Field reconciliation required: `EntryProjectionDto` has `file_transfer_ids: Vec<String>` while `ClipboardEntryProjection` has `link_domains: Option<Vec<String>>` and `#[serde(skip_serializing_if)]` annotations. The unified type must preserve the existing frontend JSON contract. Claude's discretion on approach: extend uc-app type to match wire contract, or keep a thin adapter function in uc-tauri commands.
- **D-01:** `P2PPeerInfo` and `PairedPeer` -> uc-app (application layer, alongside existing `P2pPeerSnapshot` and `LocalDeviceInfo`)
- **D-02:** `P2PPairingVerificationEvent` and `P2PPairingVerificationKind` — Researcher must investigate before planning: `host_event_emitter.rs` already has an internal `PairingVerificationPayload` that handles Tauri emit. If `events/p2p_pairing.rs` types are stale/unused externally, delete them. If actively used by other consumers, extract to uc-app. uc-core already has serde-free `PairingVerificationKind`/`PairingHostEvent` — do NOT duplicate into uc-core.
- **D-05:** Direct delete + update all imports. No re-export stubs. Clean cut.
- **D-06:** `DaemonPairingRequestError` is already correctly placed in uc-daemon-client. Only clean up any residual definition or import in uc-tauri.
- **8 Tauri-only command response types must stay in uc-tauri:** `ClipboardEntriesResponse`, `ClipboardEntryDetail`, `ClipboardEntryResource`, `ClipboardItemDto`, `ClipboardTextItemDto`, `ClipboardImageItemDto`, `ClipboardLinkItemDto`, `ClipboardItemResponse`, `LifecycleStatusDto`
- **Pairing request/response DTOs stay in uc-tauri:** `P2PPairingRequest`, `P2PPairingResponse`, `P2PPinVerifyRequest`
- specta::Type handling: Tauri commands manage specta at command level. Not in scope.

### Claude's Discretion

- Whether to extend `EntryProjectionDto` with `link_domains` + serde annotations vs. keep a thin mapping function in uc-tauri
- Exact module organization within uc-app for extracted pairing DTOs
- Whether `P2PPairingVerificationEvent` types are stale and should be deleted vs. extracted
- Order of extraction plans
- Whether uc-tauri's 8 remaining response types need any adjustments after unifying the shared types they reference

### Deferred Ideas (OUT OF SCOPE)

- "修复 setup 配对确认提示缺失" — UI bug, unrelated to DTO extraction. Belongs in a separate UI fix phase.
  </user_constraints>

---

## Pairing Event Types Investigation (D-02 Resolution)

**Finding: `P2PPairingVerificationEvent`/`P2PPairingVerificationKind` are stale — DELETE them.**

Evidence:

1. `events/p2p_pairing.rs` defines `P2PPairingVerificationEvent` and `P2PPairingVerificationKind` with serde derives and constructor helpers.
2. `events/mod.rs` re-exports them: `pub use p2p_pairing::{P2PPairingVerificationEvent, P2PPairingVerificationKind};`
3. **Zero external call sites** found across the entire `src-tauri` tree. The grep search for `P2PPairingVerification` returns only: the definition file, the re-export in `events/mod.rs`, and a comment + `kind_to_str` function comment in `host_event_emitter.rs` — none of these are active consumers constructing or emitting these types.
4. `host_event_emitter.rs` has its own module-private `PairingVerificationPayload` struct (lines 162–181) that handles the Tauri wire payload. It uses `PairingVerificationKind` from `uc-core/ports/host_event_emitter.rs` (serde-free), not the `P2PPairingVerificationKind` from `events/p2p_pairing.rs`.
5. The `uc-core` canonical types (`PairingVerificationKind`, `PairingHostEvent`) at `uc-core/src/ports/host_event_emitter.rs` (lines 150–188) already provide the serde-free semantic model the whole system uses.

**Conclusion:** `P2PPairingVerificationEvent` and `P2PPairingVerificationKind` in `uc-tauri/events/p2p_pairing.rs` have been superseded and are dead code. Safe to delete the entire file plus remove the `pub mod p2p_pairing` and `pub use` lines from `events/mod.rs`.

**Why not extract to uc-app?** They have zero consumers. Extracting dead code just moves the problem.

---

## Standard Stack

### Core Technologies

| Component          | Version                          | Notes                                          |
| ------------------ | -------------------------------- | ---------------------------------------------- |
| serde              | 1.x with `features = ["derive"]` | Already in both uc-app and uc-tauri Cargo.toml |
| serde_json         | 1.x                              | Already in both crates                         |
| Rust module system | stable                           | `pub use` for re-exports within uc-app         |

### Dependency Context

- `uc-app` already has `serde = { version = "1", features = ["derive"] }` — no new dependencies needed
- `uc-tauri` already depends on `uc-app` — the type flow direction is correct
- `uc-core` has serde-free types intentionally — do NOT add serde to uc-core types (confirmed locked)

**Installation:** No new dependencies needed. All required crates are already present.

---

## Architecture Patterns

### Pattern 1: Serde-Augmenting a Domain DTO

The recommended approach for clipboard DTO unification is **Option B: keep a thin mapping function** in uc-tauri rather than adding `link_domains` to `EntryProjectionDto`. Rationale:

- `link_domains` is derived from `link_urls` at the command layer (`extract_domain()` call in `commands/clipboard.rs` lines 73–76). It is a view-layer concern, not a domain concern.
- `file_transfer_ids` in `EntryProjectionDto` is an internal field not needed in the wire response. The frontend wire contract (uc-tauri `ClipboardEntryProjection`) does NOT include `file_transfer_ids`.
- Extending `EntryProjectionDto` with `link_domains` and making `file_transfer_ids` serde-skip would mix frontend wire concerns into the app layer.
- Cleaner split: `EntryProjectionDto` (uc-app) = domain projection DTO with serde, `ClipboardEntryProjection` (uc-tauri) is deleted and command layer does a direct field mapping.

**What the thin mapping function looks like** (already exists in `commands/clipboard.rs` lines 70–96): the existing mapping code just maps fields + derives `link_domains`. After unification, this same inline mapping can remain — the change is: the source type is now `EntryProjectionDto` with serde derives and the target type is the same `EntryProjectionDto` (no more separate `ClipboardEntryProjection`), wrapped in the existing `ClipboardEntriesResponse::Ready { entries: projections }`.

Wait — actually the cleanest approach is:

- Add serde + `#[serde(skip_serializing_if)]` derives to `EntryProjectionDto` in uc-app
- Add `link_domains: Option<Vec<String>>` to `EntryProjectionDto` (computed during projection, not during use case execution — OR computed in the use case execute)
- Delete `ClipboardEntryProjection` from uc-tauri models
- `ClipboardEntriesResponse::Ready { entries: Vec<EntryProjectionDto> }` (update the variant)

However: `file_transfer_ids: Vec<String>` in `EntryProjectionDto` would then serialize into the JSON response unless skipped. **Resolution**: add `#[serde(skip_serializing_if = "Vec::is_empty")]` or `#[serde(skip)]` to that field. Either approach is valid — the frontend currently does not receive `file_transfer_ids` so it must stay absent.

**Recommended final approach:**

1. Add `#[derive(Serialize, Deserialize)]` to `EntryProjectionDto`
2. Add `#[serde(skip_serializing_if)]` annotations to optional fields (matching current `ClipboardEntryProjection` contract)
3. Add `#[serde(skip)]` to `file_transfer_ids` (internal field, not in wire contract)
4. Add `link_domains: Option<Vec<String>>` field to `EntryProjectionDto` with `#[serde(skip_serializing_if = "Option::is_none")]`
5. Populate `link_domains` in the use case execute (derive from `link_urls` using `extract_domain`) or in the command layer thin mapper
6. Delete `ClipboardEntryProjection` from `uc-tauri/src/models/mod.rs`
7. Update `ClipboardEntriesResponse` to hold `Vec<EntryProjectionDto>` instead of `Vec<ClipboardEntryProjection>`
8. Update `commands/clipboard.rs` to use `EntryProjectionDto` directly (no more struct-to-struct mapping)

**Where to compute `link_domains`:** The `link_urls` field is already populated by the use case. The `extract_domain` call is a pure URL utility. It can stay in the command layer as a post-processing step — OR be moved into the use case `execute()`. Given D-05 (clean cut, no stubs), keeping it in the command layer is less invasive and avoids modifying the already-tested use case logic.

### Pattern 2: Extracting Pairing DTOs to uc-app

`P2PPeerInfo` and `PairedPeer` are command-level response types used by `get_p2p_peers` and `get_paired_peers`/`list_paired_devices`. They aggregate daemon API types into frontend wire shapes.

Pattern reference: `P2pPeerSnapshot` at `uc-app/src/usecases/pairing/get_p2p_peers_snapshot.rs` (no serde, domain DTO). The pairing DTOs being extracted DO have serde (they are wire types), which is fine — `uc-app` has serde.

**Target location:** `uc-app/src/usecases/pairing/dto.rs` (new file), exported via `uc-app/src/usecases/pairing/mod.rs` with `pub use dto::{P2PPeerInfo, PairedPeer}`.

### Anti-Patterns to Avoid

- **Do not add serde-free fields to uc-core types** — `PairingVerificationKind`/`PairingHostEvent` in uc-core must remain serde-free. Only uc-app and above carry serde.
- **Do not add re-export stubs** — D-05 mandates direct delete + import update. No `pub use old_path::Type` stubs.
- **Do not mix `file_transfer_ids` into the wire contract** — this is an internal field; use `#[serde(skip)]` to prevent it from appearing in JSON.
- **Do not extract dead code** — `P2PPairingVerificationEvent`/`P2PPairingVerificationKind` are unused; delete, don't move.

---

## Don't Hand-Roll

| Problem                          | Don't Build          | Use Instead                                                                  |
| -------------------------------- | -------------------- | ---------------------------------------------------------------------------- |
| Field case translation for serde | Custom serializer    | `#[serde(rename_all = "camelCase")]` or per-field `#[serde(rename = "...")]` |
| Optional field omission          | Manual Option check  | `#[serde(skip_serializing_if = "Option::is_none")]`                          |
| Empty vec omission               | Manual len check     | `#[serde(skip_serializing_if = "Vec::is_empty")]`                            |
| Hiding internal fields from JSON | Separate DTO wrapper | `#[serde(skip)]` on the specific field                                       |

---

## Complete File Change Map

### Files to Modify

| File                                                                             | Change                                                                                                                                                                                                                                                                               |
| -------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `uc-app/src/usecases/clipboard/list_entry_projections/list_entry_projections.rs` | Add `#[derive(Serialize, Deserialize)]` to `EntryProjectionDto`. Add `#[serde(skip)]` to `file_transfer_ids`. Add `link_domains: Option<Vec<String>>` with `#[serde(skip_serializing_if = "Option::is_none")]`. Add `#[serde(skip_serializing_if)]` to all existing `Option` fields. |
| `uc-app/src/usecases/clipboard/mod.rs`                                           | Add `#[derive(Serialize, Deserialize)]` to `ClipboardStats`.                                                                                                                                                                                                                         |
| `uc-tauri/src/models/mod.rs`                                                     | Delete `ClipboardEntryProjection` struct and `ClipboardStats` struct. Update `ClipboardEntriesResponse::Ready` to hold `Vec<uc_app::usecases::clipboard::EntryProjectionDto>`. Keep the 8 Tauri-only types.                                                                          |
| `uc-tauri/src/commands/clipboard.rs`                                             | Update imports. Replace `ClipboardEntryProjection` construction with direct `EntryProjectionDto` use. Keep `link_domains` derivation inline in the mapping step (or populate in EntryProjectionDto directly). Update `ClipboardStats` import to uc-app.                              |
| `uc-tauri/src/events/p2p_pairing.rs`                                             | Delete entire file.                                                                                                                                                                                                                                                                  |
| `uc-tauri/src/events/mod.rs`                                                     | Remove `pub mod p2p_pairing;` and `pub use p2p_pairing::...;` lines.                                                                                                                                                                                                                 |
| `uc-tauri/src/commands/pairing.rs`                                               | Delete `P2PPeerInfo` and `PairedPeer` struct definitions. Add import from `uc_app::usecases::pairing::{P2PPeerInfo, PairedPeer}`. `DaemonPairingRequestError` is already imported from `uc_daemon_client` — no change needed (D-06: already correct).                                |
| `uc-tauri/tests/daemon_command_shell.rs`                                         | Update `use uc_tauri::commands::pairing::PairedPeer` → `use uc_app::usecases::pairing::PairedPeer` (or adjust path after extraction).                                                                                                                                                |
| `uc-tauri/tests/models_serialization_test.rs`                                    | Update imports if `ClipboardEntryProjection` ref changes. The test itself constructs `ClipboardEntryProjection` by name — must update to use `EntryProjectionDto` from uc-app.                                                                                                       |

### New Files to Create

| File                                 | Content                                                             |
| ------------------------------------ | ------------------------------------------------------------------- |
| `uc-app/src/usecases/pairing/dto.rs` | Move `P2PPeerInfo` and `PairedPeer` structs here with serde derives |

### D-06 Verification — DaemonPairingRequestError

Current state in `uc-tauri/src/commands/pairing.rs`:

```rust
use uc_daemon_client::{
    http::{DaemonPairingClient, DaemonPairingRequestError, DaemonQueryClient},
    DaemonConnectionState,
};
```

This is already importing from `uc_daemon_client`. There is no residual definition of `DaemonPairingRequestError` in uc-tauri. D-06 is already satisfied — no action needed for this item.

---

## Common Pitfalls

### Pitfall 1: `file_transfer_ids` Leaks Into Wire JSON

**What goes wrong:** After adding serde to `EntryProjectionDto`, `file_transfer_ids: Vec<String>` will serialize into the JSON response sent to the frontend. The frontend does not know this field and currently does not receive it. If it suddenly appears, it could cause unexpected TypeScript deserialization behavior.
**Why it happens:** Adding `#[derive(Serialize)]` to a struct serializes ALL fields by default.
**How to avoid:** Add `#[serde(skip)]` to `file_transfer_ids` before shipping.
**Warning signs:** A test assertion like `assert!(value.get("file_transfer_ids").is_none())` failing.

### Pitfall 2: `ClipboardStats` Import Path Change Breaks Callers

**What goes wrong:** `commands/clipboard.rs` imports `ClipboardStats` from `crate::models`. After deletion from uc-tauri models, if the import is not updated to `uc_app::usecases::clipboard::ClipboardStats`, compilation fails.
**How to avoid:** Grep for all `ClipboardStats` usages in uc-tauri before deleting.

### Pitfall 3: Test File Uses Deleted Type by Value

**What goes wrong:** `uc-tauri/tests/models_serialization_test.rs` directly constructs `ClipboardEntryProjection` by listing all fields (struct literal). After deleting `ClipboardEntryProjection`, the test must use `EntryProjectionDto` from uc-app — and the field set is different (`file_transfer_ids` present, `link_domains` absent in current `EntryProjectionDto`, both need reconciling).
**How to avoid:** Update test file in the same plan as the model deletion.

### Pitfall 4: `ClipboardEntriesResponse::Ready` Loses Concrete Type

**What goes wrong:** `ClipboardEntriesResponse` in uc-tauri models holds `Vec<ClipboardEntryProjection>`. After deletion, it must hold `Vec<EntryProjectionDto>` (from uc-app). The serde annotation `#[serde(tag = "status", rename_all = "snake_case")]` on the enum must be preserved. The test `clipboard_entries_response_ready_serializes_correctly` must still pass.
**How to avoid:** After changing the variant inner type, run the serialization tests.

### Pitfall 5: `events/mod.rs` Re-exports Orphaned Test Code

**What goes wrong:** `events/p2p_pairing.rs` contains tests (`#[cfg(test)] mod tests`) that reference the deleted types. If only the file is deleted but the module declaration remains, the compiler errors. If the `pub mod p2p_pairing` line is removed but the file still exists, it compiles but the dead file remains.
**How to avoid:** Delete the file AND remove the `pub mod` + `pub use` lines from `events/mod.rs` in the same plan step.

### Pitfall 6: `daemon_command_shell.rs` Import Path

**What goes wrong:** `tests/daemon_command_shell.rs` line 15: `use uc_tauri::commands::pairing::PairedPeer;`. After extracting `PairedPeer` to uc-app, this import path becomes invalid.
**How to avoid:** Update the import to `use uc_app::usecases::pairing::PairedPeer;` in the same plan that moves the type.

---

## Code Examples

### Adding serde to EntryProjectionDto (approach)

```rust
// Source: codebase investigation + serde docs
// In uc-app/src/usecases/clipboard/list_entry_projections/list_entry_projections.rs

use serde::{Deserialize, Serialize};  // already available via uc-app Cargo.toml

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntryProjectionDto {
    pub id: String,
    pub preview: String,
    pub has_detail: bool,
    pub size_bytes: i64,
    pub captured_at: i64,
    pub content_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail_url: Option<String>,
    pub is_encrypted: bool,
    pub is_favorited: bool,
    pub updated_at: i64,
    pub active_time: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_transfer_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_transfer_reason: Option<String>,
    #[serde(skip)]  // internal field — not part of frontend wire contract
    pub file_transfer_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_urls: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_domains: Option<Vec<String>>,  // NEW: derived from link_urls at command layer
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_sizes: Option<Vec<i64>>,
}
```

### Pairing DTO target location

```rust
// Source: codebase investigation
// New file: uc-app/src/usecases/pairing/dto.rs

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct P2PPeerInfo {
    pub peer_id: String,
    pub device_name: Option<String>,
    pub addresses: Vec<String>,
    pub is_paired: bool,
    pub connected: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedPeer {
    pub peer_id: String,
    pub device_name: String,
    pub shared_secret: Vec<u8>,
    pub paired_at: String,
    pub last_seen: Option<String>,
    pub last_known_addresses: Vec<String>,
    pub connected: bool,
}
```

### Updated `uc-app/src/usecases/pairing/mod.rs` export addition

```rust
// Add after existing pub use lines:
pub mod dto;
pub use dto::{P2PPeerInfo, PairedPeer};
```

### Deleting stale event types

```rust
// events/mod.rs — BEFORE
pub mod p2p_pairing;
pub use p2p_pairing::{P2PPairingVerificationEvent, P2PPairingVerificationKind};

// events/mod.rs — AFTER (remove those two lines)
// File p2p_pairing.rs is deleted entirely.
```

---

## Validation Architecture

### Test Framework

| Property           | Value                                            |
| ------------------ | ------------------------------------------------ |
| Framework          | Rust `cargo test` (built-in) + Vitest (frontend) |
| Config file        | `src-tauri/` (run from here per CLAUDE.md)       |
| Quick run command  | `cd src-tauri && cargo test -p uc-tauri`         |
| Full suite command | `cd src-tauri && cargo test`                     |

### Key Test Files Affected

| Test File                                     | What It Tests                                                         | Change Required                                                                                                                      |
| --------------------------------------------- | --------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| `uc-tauri/tests/models_serialization_test.rs` | `ClipboardEntryProjection` struct literal construction + serde output | Update struct type to `EntryProjectionDto`, add `link_domains: None`, `file_transfer_ids` (now skipped by serde but still in struct) |
| `uc-tauri/tests/daemon_command_shell.rs`      | `PairedPeer` import + IPC roundtrip                                   | Update import path to uc-app                                                                                                         |
| `uc-tauri/src/models/mod.rs` inline tests     | `ClipboardStats` serde, `ClipboardEntryProjection` serde              | These tests will move/be deleted with the types                                                                                      |
| `uc-app/.../list_entry_projections.rs` tests  | `EntryProjectionDto` construction in tests (many)                     | Each test constructs `EntryProjectionDto` by field list — add `link_domains: None` to each                                           |

### Phase Gate

After all plans complete, run `cd src-tauri && cargo test` — all tests green before marking phase complete.

### Wave 0 Gaps

None — existing test infrastructure covers all phase requirements. The changes are refactors of existing types, not new features.

---

## Open Questions

1. **`link_domains` population location**
   - What we know: Currently derived in `commands/clipboard.rs` lines 73–76 during the `EntryProjectionDto` → `ClipboardEntryProjection` mapping. After deletion of `ClipboardEntryProjection`, this derivation step must still happen somewhere.
   - What's unclear: Should `link_domains` be populated in the use case `execute()` (requires `extract_domain` to be available from uc-core link_utils, which it is), or remain in the command layer?
   - Recommendation: Keep it in the command layer as a post-processing step populating the new `link_domains` field directly on the `EntryProjectionDto` returned from the use case. This avoids modifying the already-tested use case execute path. Implementation: after calling `uc.execute()`, iterate and set `dto.link_domains = dto.link_urls.as_ref().map(|urls| urls.iter().filter_map(|u| extract_domain(u)).collect())`.

2. **`ClipboardEntriesResponse` inner type update**
   - What we know: `ClipboardEntriesResponse::Ready { entries: Vec<ClipboardEntryProjection> }` — after deletion, `entries` must be `Vec<EntryProjectionDto>`.
   - What's unclear: `ClipboardEntriesResponse` stays in uc-tauri (it is a Tauri-specific envelope type). It just needs its inner `Vec<>` type updated to point to the uc-app type.
   - Recommendation: Update in-place in `uc-tauri/src/models/mod.rs`, same plan as the deletion.

---

## Sources

### Primary (HIGH confidence)

- Direct codebase reads — all canonical reference files from CONTEXT.md
  - `uc-tauri/src/models/mod.rs` — full struct definitions verified
  - `uc-app/src/usecases/clipboard/list_entry_projections/list_entry_projections.rs` — `EntryProjectionDto` definition verified
  - `uc-app/src/usecases/clipboard/mod.rs` — `ClipboardStats` definition verified
  - `uc-tauri/src/commands/clipboard.rs` — existing mapping logic verified
  - `uc-tauri/src/commands/pairing.rs` — `P2PPeerInfo`, `PairedPeer` definitions + `DaemonPairingRequestError` import verified
  - `uc-tauri/src/events/p2p_pairing.rs` — stale types verified
  - `uc-tauri/src/events/mod.rs` — re-exports verified
  - `uc-tauri/src/adapters/host_event_emitter.rs` — `PairingVerificationPayload` (module-private canonical wire type) verified
  - `uc-core/src/ports/host_event_emitter.rs` — `PairingVerificationKind` serde-free canonical type verified
  - `uc-app/src/usecases/pairing/mod.rs` — existing exports verified (no `P2PPeerInfo`/`PairedPeer`)
  - `uc-tauri/tests/daemon_command_shell.rs` — `PairedPeer` import path verified
  - `uc-tauri/tests/models_serialization_test.rs` — affected test patterns verified
- Grep search: `P2PPairingVerification` across all of `src-tauri` — zero external consumers confirmed
- `uc-app/Cargo.toml` — serde dependency confirmed present

---

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — no new dependencies, existing serde setup confirmed
- Architecture: HIGH — confirmed by direct codebase inspection
- Pitfalls: HIGH — confirmed by reading actual test files and type definitions
- D-02 resolution: HIGH — confirmed by exhaustive grep showing zero external consumers of the p2p_pairing.rs types

**Research date:** 2026-03-25
**Valid until:** Stable (pure Rust refactoring, no external API dependencies)
