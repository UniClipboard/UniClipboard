---
phase: quick-02
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
  - src-tauri/crates/uc-core/src/network/protocol/clipboard_payload.rs
  - src-tauri/crates/uc-core/src/network/protocol/mod.rs
  - src-tauri/crates/uc-core/src/network/mod.rs
  - src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs
  - src-tauri/crates/uc-app/src/usecases/clipboard/sync_outbound.rs
  - src-tauri/crates/uc-app/src/usecases/clipboard/list_entry_projections/list_entry_projections.rs
  - src-tauri/crates/uc-infra/src/clipboard/chunked_transfer.rs
  - src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs
  - src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs
  - .planning/STATE.md
autonomous: true
requirements: [quick-02]
must_haves:
  truths:
    - 'MIME string literals are defined as constants in uc-core and used everywhere else'
    - 'Image detection is case-insensitive in list_entry_projections'
    - 'V2 inbound avoids cloning bytes when selecting highest-priority repr'
    - 'Encoder rejects plaintext > u32::MAX bytes instead of silent truncation'
    - 'Decoder validates total_plaintext_len against chunk count and final output size'
    - 'Migration sentinel only written when all file removals succeeded'
    - 'NoopPort BlobStorePort::put does not panic on data > i64::MAX'
    - 'SyncOutbound no longer carries unused encryption field'
    - 'Tampered-content test name matches its assertion (returns_err)'
    - 'STATE.md frontmatter reflects actual project completion'
  artifacts:
    - path: 'src-tauri/crates/uc-core/src/network/protocol/clipboard_payload.rs'
      provides: 'Module-level MIME constants'
      contains: 'pub const MIME_IMAGE_PREFIX'
    - path: 'src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs'
      provides: 'Zero-copy repr selection via index + remove'
      contains: 'swap_remove'
    - path: 'src-tauri/crates/uc-infra/src/clipboard/chunked_transfer.rs'
      provides: 'u32 overflow guard and decoder validation'
      contains: 'u32::try_from'
  key_links:
    - from: 'src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs'
      to: 'uc-core MIME constants'
      via: 'use uc_core::network::protocol::clipboard_payload'
      pattern: 'MIME_IMAGE_PREFIX|MIME_TEXT_HTML|MIME_TEXT_RTF|MIME_TEXT_PLAIN'
    - from: 'src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs'
      to: 'SyncOutboundClipboardUseCase::new'
      via: 'removed encryption parameter'
      pattern: 'sync_outbound_clipboard'
---

<objective>
Apply round 2 code review findings across uc-core, uc-app, uc-infra, and uc-tauri.

Purpose: Close 10 review items spanning MIME constant extraction, case-insensitive checks, zero-copy optimization, overflow guards, decoder validation, migration safety, dead code removal, test naming, and STATE.md consistency.
Output: Clean codebase with all review findings addressed, all existing tests passing.
</objective>

<execution_context>
@/home/wuy6/.claude/get-shit-done/workflows/execute-plan.md
@/home/wuy6/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/STATE.md
@src-tauri/crates/uc-core/src/network/protocol/clipboard_payload.rs
@src-tauri/crates/uc-core/src/network/protocol/mod.rs
@src-tauri/crates/uc-core/src/network/mod.rs
@src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs
@src-tauri/crates/uc-app/src/usecases/clipboard/sync_outbound.rs
@src-tauri/crates/uc-app/src/usecases/clipboard/list_entry_projections/list_entry_projections.rs
@src-tauri/crates/uc-infra/src/clipboard/chunked_transfer.rs
@src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs
@src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs
</context>

<tasks>

<task type="auto">
  <name>Task 1: MIME constants, case-insensitive check, zero-copy inbound, dead code removal, test rename</name>
  <files>
    src-tauri/crates/uc-core/src/network/protocol/clipboard_payload.rs,
    src-tauri/crates/uc-core/src/network/protocol/mod.rs,
    src-tauri/crates/uc-core/src/network/mod.rs,
    src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs,
    src-tauri/crates/uc-app/src/usecases/clipboard/sync_outbound.rs,
    src-tauri/crates/uc-app/src/usecases/clipboard/list_entry_projections/list_entry_projections.rs,
    src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs
  </files>
  <action>
**1a. Extract MIME constants into uc-core (clipboard_payload.rs)**

Add module-level public constants ABOVE the `ClipboardTextPayloadV1` struct definition:

```rust
/// Standard MIME type constants used throughout the clipboard protocol.
pub const MIME_IMAGE_PREFIX: &str = "image/";
pub const MIME_TEXT_HTML: &str = "text/html";
pub const MIME_TEXT_RTF: &str = "text/rtf";
pub const MIME_TEXT_PLAIN: &str = "text/plain";
```

Keep `ClipboardTextPayloadV1::MIME_TEXT_PLAIN` as-is for backward compatibility — it can reference the module constant: `pub const MIME_TEXT_PLAIN: &str = super::MIME_TEXT_PLAIN;` — actually, since both are `"text/plain"`, just leave the struct const unchanged; the module-level constant is the canonical one for new code.

**Re-export from protocol/mod.rs:** Add to the existing `pub use clipboard_payload::` line:

```rust
pub use clipboard_payload::{ClipboardTextPayloadV1, MIME_IMAGE_PREFIX, MIME_TEXT_HTML, MIME_TEXT_RTF, MIME_TEXT_PLAIN};
```

Also re-export from `network/mod.rs` by extending the `pub use protocol::` line to include the MIME constants:

```rust
pub use protocol::{
    ClipboardMessage, DeviceAnnounceMessage, HeartbeatMessage, PairingBusy, PairingCancel,
    PairingChallenge, PairingChallengeResponse, PairingConfirm, PairingKeyslotOffer,
    PairingMessage, PairingReject, PairingRequest, PairingResponse, ProtocolMessage,
    MIME_IMAGE_PREFIX, MIME_TEXT_HTML, MIME_TEXT_RTF, MIME_TEXT_PLAIN,
};
```

**1b. Use MIME constants in sync_inbound.rs**

Add to existing imports at top of file:

```rust
use uc_core::network::protocol::{MIME_IMAGE_PREFIX, MIME_TEXT_HTML, MIME_TEXT_RTF, MIME_TEXT_PLAIN};
```

In `select_highest_priority_repr_index` (the new function — see 1d below), update the `priority` inner fn:

```rust
fn priority(mime: Option<&str>) -> u8 {
    match mime {
        Some(m) if m.to_ascii_lowercase().starts_with(MIME_IMAGE_PREFIX) => 4,
        Some(m) if m.eq_ignore_ascii_case(MIME_TEXT_HTML) => 3,
        Some(m) if m.eq_ignore_ascii_case(MIME_TEXT_RTF) => 2,
        Some(m) if m.eq_ignore_ascii_case(MIME_TEXT_PLAIN) => 1,
        _ => 0,
    }
}
```

Also update `first_text_representation_len` (line ~578-583) to use the constant:

```rust
.is_some_and(|mime| mime.as_str().eq_ignore_ascii_case(MIME_TEXT_PLAIN))
```

**1c. Case-insensitive is_image check in list_entry_projections.rs**

At line 156-160, change:

```rust
let is_image = representation
    .mime_type
    .as_ref()
    .map(|mt| mt.as_str().to_ascii_lowercase().starts_with("image/"))
    .unwrap_or(false);
```

If MIME constants are importable here (uc-app depends on uc-core), use `MIME_IMAGE_PREFIX` instead of `"image/"`.

**1d. Zero-copy inbound: replace `select_highest_priority_repr` with index-based approach**

Replace the current `select_highest_priority_repr` function (lines 544-559) with:

```rust
/// Returns the index of the highest-priority WireRepresentation, or None if empty.
fn select_highest_priority_repr_index(representations: &[WireRepresentation]) -> Option<usize> {
    fn priority(mime: Option<&str>) -> u8 {
        match mime {
            Some(m) if m.to_ascii_lowercase().starts_with(MIME_IMAGE_PREFIX) => 4,
            Some(m) if m.eq_ignore_ascii_case(MIME_TEXT_HTML) => 3,
            Some(m) if m.eq_ignore_ascii_case(MIME_TEXT_RTF) => 2,
            Some(m) if m.eq_ignore_ascii_case(MIME_TEXT_PLAIN) => 1,
            _ => 0,
        }
    }
    representations
        .iter()
        .enumerate()
        .max_by_key(|(_, r)| priority(r.mime.as_deref()))
        .map(|(i, _)| i)
}
```

Then update the call site (lines 461-483). Replace:

```rust
let selected = match select_highest_priority_repr(&v2_payload.representations) {
    Some(r) => r,
    None => { ... }
};
```

With:

```rust
let selected_idx = match select_highest_priority_repr_index(&v2_payload.representations) {
    Some(i) => i,
    None => {
        warn!(message_id = %message.id, "V2 inbound: no representations — dropping");
        self.rollback_recent_id(&message.id).await;
        return Ok(InboundApplyOutcome::Skipped);
    }
};

// Take ownership via swap_remove to avoid cloning potentially large bytes.
let selected = v2_payload.representations.swap_remove(selected_idx);
```

Then update the snapshot construction below to use owned values (no `.clone()`):

```rust
let mime: Option<MimeType> = selected.mime.as_deref().map(|s| MimeType(s.to_string()));
let snapshot = SystemClipboardSnapshot {
    ts_ms: v2_payload.ts_ms,
    representations: vec![ObservedClipboardRepresentation {
        id: RepresentationId::new(),
        format_id: FormatId::from(selected.format_id.as_str()),
        mime,
        bytes: selected.bytes,  // <-- owned, no .clone()
    }],
};
```

Update any test that called `select_highest_priority_repr` directly — if any test references the old function name, update to use `select_highest_priority_repr_index`. The existing tests in the `tests` module that use `build_v2_message` and call `execute_with_outcome` should continue to work since the public API is unchanged.

**1e. Rename test function**

In sync_inbound.rs, line 1515: rename `v2_message_with_tampered_content_returns_skipped` to `v2_message_with_tampered_content_returns_err`.

**1f. Remove unused encryption field from SyncOutbound**

In `sync_outbound.rs`:

- Remove lines 24-25: `#[allow(dead_code)]` and `encryption: Arc<dyn EncryptionPort>,`
- Remove `encryption` parameter from `new()` constructor (line 37)
- Remove `encryption,` from struct init (line 47)
- Remove `EncryptionPort` from the use statement if it's no longer used in this file

In `sync_outbound.rs` tests `build_usecase` function (~line 555-625):

- Remove `Arc::new(TestEncryption { encrypt_calls: encrypt_calls.clone() })` from the `SyncOutboundClipboardUseCase::new(...)` call (line 608-609)
- Check if `TestEncryption` struct is still needed elsewhere — if only used for this parameter, remove it entirely

In `runtime.rs` line 743: Remove `self.runtime.deps.encryption.clone(),` from the `sync_outbound_clipboard()` factory method.
</action>
<verify>
<automated>cd /home/wuy6/myprojects/UniClipboard/src-tauri && cargo test -p uc-app -p uc-core -p uc-tauri --lib 2>&1 | tail -20</automated>
</verify>
<done> - MIME constants defined in uc-core and used in sync_inbound.rs and list_entry_projections.rs - is_image check is case-insensitive - sync_inbound uses swap_remove for zero-copy repr selection - Test renamed to v2_message_with_tampered_content_returns_err - SyncOutbound no longer has unused encryption field; all call sites updated - All uc-app, uc-core, and uc-tauri tests pass
</done>
</task>

<task type="auto">
  <name>Task 2: Chunked transfer overflow guards, decoder validation, migration safety, NoopPort fix, STATE.md update</name>
  <files>
    src-tauri/crates/uc-infra/src/clipboard/chunked_transfer.rs,
    src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs,
    src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs,
    .planning/STATE.md
  </files>
  <action>
**2a. u32 overflow guard in ChunkedEncoder (chunked_transfer.rs)**

At line 128, replace:

```rust
let total_plaintext_len = plaintext.len() as u32;
```

With:

```rust
let total_plaintext_len = u32::try_from(plaintext.len())
    .map_err(|_| ChunkedTransferError::EncryptFailed(
        format!("plaintext length {} exceeds u32::MAX", plaintext.len())
    ))?;
```

Line 132 `as u32` cast is then safe because we already validated `plaintext.len() <= u32::MAX`. Keep the existing cast.

**2b. Decoder validation in ChunkedDecoder (chunked_transfer.rs)**

After reading the header (after line ~208 where `total_plaintext_len` is computed), add validation:

```rust
// Validate header consistency: plaintext cannot exceed what total_chunks can hold.
if total_chunks > 0 && total_plaintext_len == 0 {
    return Err(ChunkedTransferError::InvalidCiphertextLen {
        chunk_index: 0,
        ciphertext_len: 0,
    });
}
if total_plaintext_len > total_chunks as usize * CHUNK_SIZE {
    return Err(ChunkedTransferError::InvalidCiphertextLen {
        chunk_index: 0,
        ciphertext_len: total_plaintext_len,
    });
}
```

Note: Reusing `InvalidCiphertextLen` is acceptable but consider adding a new variant for clearer semantics. If adding a new variant:

```rust
/// Header declares a total_plaintext_len inconsistent with chunk count.
#[error("header validation failed: {reason}")]
InvalidHeader { reason: String },
```

And map it in the `From<ChunkedTransferError> for TransferCryptoError` impl:

```rust
ChunkedTransferError::InvalidHeader { reason } => {
    TransferCryptoError::InvalidFormat(reason)
}
```

If using the new variant, update the two validations above to use `InvalidHeader` instead of `InvalidCiphertextLen`.

After the chunk loop (after line 255 `plaintext.extend_from_slice`), before `Ok(plaintext)`, add:

```rust
if plaintext.len() != total_plaintext_len {
    return Err(ChunkedTransferError::InvalidHeader {
        reason: format!(
            "decoded {} bytes but header declared {}",
            plaintext.len(),
            total_plaintext_len
        ),
    });
}
```

**2c. Migration error handling in wiring.rs (lines 611-647)**

Replace the `entries.flatten()` loop with explicit error tracking:

```rust
Ok(entries) => {
    let mut purged = 0u64;
    let mut errors = 0u64;
    for entry_result in entries {
        let entry = match entry_result {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(error = %e, "Failed to read directory entry during V2 migration");
                errors += 1;
                continue;
            }
        };
        if entry.path().is_file() {
            if let Err(e) = std::fs::remove_file(entry.path()) {
                tracing::warn!(
                    path = %entry.path().display(),
                    error = %e,
                    "Failed to purge old blob file"
                );
                errors += 1;
            } else {
                purged += 1;
            }
        }
    }
    if purged > 0 {
        tracing::info!(
            count = purged,
            "Purged old blob files (V2 format migration)"
        );
    }

    // Only mark migration complete when ALL files were handled successfully.
    if errors == 0 {
        if let Err(e) = std::fs::File::create(&sentinel) {
            tracing::warn!(error = %e, "Failed to create V2 migration sentinel");
        }
    } else {
        tracing::warn!(
            errors = errors,
            "Skipping V2 migration sentinel: {} errors during cleanup, will retry next startup",
            errors
        );
    }
}
```

**2d. NoopPort BlobStorePort::put overflow (runtime.rs line 1491)**

Change:

```rust
Some(_data.len() as i64)
```

To:

```rust
i64::try_from(_data.len()).ok()
```

**2e. STATE.md frontmatter and backward-compat consistency**

Update `.planning/STATE.md` frontmatter:

- `status: unknown` -> `status: completed`
- `progress.total_phases: 1` -> `progress.total_phases: 4`
- `progress.completed_phases: 1` -> `progress.completed_phases: 4`
- `progress.total_plans: 2` -> `progress.total_plans: 8`
- `progress.completed_plans: 2` -> `progress.completed_plans: 8`
- `progress.percent: 100` (keep as is)

On line 49, change:

```
- Kept for_blob (v1) unchanged alongside new for_blob_v2 for backward compatibility
```

To:

```
- Replaced for_blob (v1) with for_blob_v2 (breaking change -- V1 blobs are incompatible with V2 binary format)
```

  </action>
  <verify>
    <automated>cd /home/wuy6/myprojects/UniClipboard/src-tauri && cargo test -p uc-infra -p uc-tauri --lib 2>&1 | tail -20</automated>
  </verify>
  <done>
    - ChunkedEncoder rejects plaintext > u32::MAX with descriptive error
    - ChunkedDecoder validates total_plaintext_len against chunk count, and verifies final output length
    - Migration only writes sentinel when error_count == 0; errors are counted and logged
    - NoopPort uses i64::try_from instead of as-cast
    - STATE.md frontmatter matches actual project state; backward-compat line corrected
    - All uc-infra and uc-tauri tests pass
  </done>
</task>

</tasks>

<verification>
Run the full workspace test suite to confirm no regressions:

```bash
cd /home/wuy6/myprojects/UniClipboard/src-tauri && cargo test --workspace --lib
```

Verify no remaining hardcoded MIME literals in uc-app:

```bash
grep -rn '"image/"' src-tauri/crates/uc-app/src/ | grep -v 'test'
grep -rn '"text/html"' src-tauri/crates/uc-app/src/ | grep -v 'test'
grep -rn '"text/rtf"' src-tauri/crates/uc-app/src/ | grep -v 'test'
```

Verify no `.bytes.clone()` in sync_inbound repr selection:

```bash
grep -n 'selected\.bytes\.clone' src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs
```

Verify SyncOutbound no longer has encryption field:

```bash
grep -n 'encryption' src-tauri/crates/uc-app/src/usecases/clipboard/sync_outbound.rs | grep -v test | grep -v '//'
```

</verification>

<success_criteria>

- All 10 review findings addressed
- `cargo test --workspace --lib` passes with zero failures
- No hardcoded MIME string literals remain in uc-app production code (tests are ok)
- Zero-copy repr selection confirmed (no `.bytes.clone()` at selection site)
- STATE.md frontmatter accurately reflects 4 phases completed
  </success_criteria>

<output>
After completion, create `.planning/quick/2-verify-and-fix-code-review-findings-roun/2-SUMMARY.md`
</output>
