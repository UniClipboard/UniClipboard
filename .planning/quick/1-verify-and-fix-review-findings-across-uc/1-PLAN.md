---
phase: quick
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
  - src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs
  - src-tauri/crates/uc-app/src/usecases/clipboard/sync_outbound.rs
  - src-tauri/crates/uc-infra/src/clipboard/chunked_transfer.rs
  - src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs
  - src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs
autonomous: true
requirements: []
must_haves:
  truths:
    - 'Decrypt/deserialize failures in V2 inbound propagate as Err, not silent Ok(Skipped)'
    - 'MIME priority check for image/* is case-insensitive'
    - 'Outbound sync does not clone byte buffers when building WireRepresentation'
    - 'ChunkedDecoder rejects ciphertext_len outside valid range before allocating'
    - 'V2 migration sentinel is only written when all blob removals succeeded'
    - 'NoopPort BlobStorePort::put test double returns Some(data.len())'
  artifacts:
    - path: 'src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs'
      provides: 'Error propagation for decrypt/deserialize + case-insensitive MIME check'
    - path: 'src-tauri/crates/uc-app/src/usecases/clipboard/sync_outbound.rs'
      provides: 'Zero-copy byte transfer via into_iter'
    - path: 'src-tauri/crates/uc-infra/src/clipboard/chunked_transfer.rs'
      provides: 'Bounds check on ciphertext_len from wire'
    - path: 'src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs'
      provides: 'Conditional sentinel creation'
    - path: 'src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs'
      provides: 'Correct test double return value'
  key_links:
    - from: 'sync_inbound.rs decrypt/deserialize'
      to: 'upstream error handlers'
      via: 'Err propagation instead of Ok(Skipped)'
      pattern: "return Err\\(anyhow"
    - from: 'chunked_transfer.rs decode'
      to: 'memory allocation'
      via: 'bounds validation before vec allocation'
      pattern: 'InvalidCiphertextLen|ciphertext_len'
---

<objective>
Fix 6 verified review findings across uc-app, uc-infra, and uc-tauri crates.

Purpose: Eliminate silent error swallowing, unnecessary byte cloning, case-sensitive MIME matching, unbounded allocation from untrusted wire data, incorrect sentinel placement, and wrong test double return value.
Output: All 6 fixes applied, existing tests pass, cargo check clean.
</objective>

<execution_context>
@/home/wuy6/.claude/get-shit-done/workflows/execute-plan.md
@/home/wuy6/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@CLAUDE.md
</context>

<tasks>

<task type="auto">
  <name>Task 1: Fix error propagation, MIME case-sensitivity, and byte cloning in uc-app</name>
  <files>
    src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs
    src-tauri/crates/uc-app/src/usecases/clipboard/sync_outbound.rs
  </files>
  <action>
**sync_inbound.rs — Fix 1: Error propagation (lines ~424-452)**

In the `execute_async` method (or equivalent V2 inbound handler), find the two error paths that currently return `Ok(InboundApplyOutcome::Skipped)` for decrypt and deserialize failures:

1. The `Err(e)` arm after `self.transfer_decryptor.decrypt(...)` (around line 429-437): Change `return Ok(InboundApplyOutcome::Skipped)` to `return Err(anyhow::anyhow!("V2 inbound: failed to decode chunked payload for message {}: {e}", message.id))`. Keep the `self.rollback_recent_id(&message.id).await` call BEFORE the return. Keep the `error!()` log line.

2. The `Err(e)` arm after `serde_json::from_slice(...)` (around line 444-452): Change `return Ok(InboundApplyOutcome::Skipped)` to `return Err(anyhow::anyhow!("V2 inbound: failed to deserialize ClipboardMultiRepPayloadV2 for message {}: {e}", message.id))`. Keep rollback and log.

Do NOT change the "no representations" case (around line 458-461) — that should remain `Ok(Skipped)` because having no representations is a valid (if unusual) state.

**sync_inbound.rs — Fix 2: Case-insensitive MIME check (lines ~541-547)**

In the `select_highest_priority_repr` function's inner `priority` closure, change the image match arm from:

```rust
Some(m) if m.starts_with("image/") => 4,
```

to:

```rust
Some(m) if m.to_ascii_lowercase().starts_with("image/") => 4,
```

This makes the image MIME check consistent with the other arms that use `eq_ignore_ascii_case`.

**sync_outbound.rs — Fix 3: Eliminate byte cloning (lines ~126-134)**

In `execute_async`, the `snapshot.representations.iter().map(|rep| ... rep.bytes.clone())` copies all byte buffers unnecessarily. Fix by extracting `snapshot_hash()` and `ts_ms` BEFORE consuming representations:

1. Before the `wire_reps` block, add:

```rust
let content_hash = snapshot.snapshot_hash().to_string();
let ts_ms = snapshot.ts_ms;
```

2. Change `snapshot.representations.iter().map(|rep| ...)` to `snapshot.representations.into_iter().map(|rep| ...)` and change `rep.bytes.clone()` to `rep.bytes` (move instead of clone). Also change `rep.mime.as_ref().map(|m| m.as_str().to_string())` to `rep.mime.map(|m| m.into_inner())` — check if `MimeType` has `into_inner()` or equivalent; if not, use `rep.mime.map(|m| m.0)` since `MimeType` is a newtype wrapper `MimeType(String)`. Also change `rep.format_id.as_ref().to_string()` to use the moved value: check `FormatId` — if it wraps String, use `rep.format_id.into_inner()` or `rep.format_id.0`.

3. Change line `ts_ms: snapshot.ts_ms,` to `ts_ms,` (use the pre-extracted variable).

4. Change line `let content_hash = snapshot.snapshot_hash().to_string();` (the old one, around line 159) to `// content_hash computed above before consuming snapshot.representations` and use the pre-extracted `content_hash` variable. Actually, just remove the old line since `content_hash` is already defined above.

NOTE: After `into_iter()`, `snapshot.representations` is consumed. Verify no other code after this block accesses `snapshot.representations`. The only later accesses are `snapshot.ts_ms` (extracted above) and `snapshot.snapshot_hash()` (extracted above). The `representation_count` in the span at line 70 is fine because it executes before `execute_async`.
</action>
<verify>
<automated>cd src-tauri && cargo check -p uc-app 2>&1 | tail -5</automated>
</verify>
<done> - Decrypt/deserialize failures return Err(anyhow) instead of Ok(Skipped) - Image MIME matching is case-insensitive via to_ascii_lowercase() - sync_outbound uses into_iter() with moved bytes, no .clone() on byte buffers - cargo check passes with no errors or warnings on uc-app
</done>
</task>

<task type="auto">
  <name>Task 2: Fix chunked transfer bounds check, wiring sentinel, and test double</name>
  <files>
    src-tauri/crates/uc-infra/src/clipboard/chunked_transfer.rs
    src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs
    src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs
  </files>
  <action>
**chunked_transfer.rs — Fix 4: Validate ciphertext_len before allocation (lines ~209-215)**

In the `ChunkedDecoder::decode` method (or the `decode` function inside `impl TransferPayloadDecryptorPort`), after reading the 4-byte `ciphertext_len` prefix at line 209 (`let ciphertext_len = u32::from_le_bytes(len_buf) as usize;`), add bounds validation BEFORE the `vec![0u8; ciphertext_len]` allocation:

```rust
let ciphertext_len = u32::from_le_bytes(len_buf) as usize;

// XChaCha20-Poly1305 tag is 16 bytes. Valid ciphertext must contain at least
// the tag, and at most one full chunk of plaintext + tag.
const TAG_SIZE: usize = 16;
let max_ciphertext = CHUNK_SIZE + TAG_SIZE;
if ciphertext_len < TAG_SIZE || ciphertext_len > max_ciphertext {
    return Err(ChunkedTransferError::InvalidPayload(format!(
        "chunk {chunk_index}: ciphertext_len {ciphertext_len} outside valid range [{TAG_SIZE}, {max_ciphertext}]"
    )));
}
```

Check that `ChunkedTransferError` has an `InvalidPayload(String)` variant. If not, find the appropriate variant — it may be `MalformedHeader` or similar. If no suitable variant exists, add `InvalidCiphertextLen` to the enum:

```rust
/// Ciphertext length from wire is outside valid range.
InvalidCiphertextLen { chunk_index: u32, ciphertext_len: usize },
```

And update the Display/Error impl accordingly. Use this variant instead of `InvalidPayload`.

**wiring.rs — Fix 5: Only create sentinel after successful purge (lines ~611-644)**

Move the sentinel creation (lines 642-644) INSIDE the `Ok(entries)` arm, after the purge loop completes. Current code:

```rust
match std::fs::read_dir(&blob_store_dir) {
    Ok(entries) => { /* purge loop */ }
    Err(e) => { /* warn */ }
}
// BUG: sentinel created even on read_dir failure
if let Err(e) = std::fs::File::create(&sentinel) { ... }
```

Change to:

```rust
match std::fs::read_dir(&blob_store_dir) {
    Ok(entries) => {
        /* existing purge loop */
        /* existing info! log */

        // Only mark migration complete after successful directory scan
        if let Err(e) = std::fs::File::create(&sentinel) {
            tracing::warn!(error = %e, "Failed to create V2 migration sentinel");
        }
    }
    Err(e) => {
        tracing::warn!(error = %e, "Failed to read blob directory for cleanup");
    }
}
```

**runtime.rs — Fix 6: Fix NoopPort BlobStorePort::put return value (lines ~1484-1490)**

In the `impl BlobStorePort for NoopPort` block, change the `put` method's return from:

```rust
Ok((std::path::PathBuf::from("/tmp/noop"), None))
```

to:

```rust
Ok((std::path::PathBuf::from("/tmp/noop"), Some(_data.len() as i64)))
```

This makes the test double return a realistic `compressed_size` instead of `None`, matching what the real `FilesystemBlobStore` returns.
</action>
<verify>
<automated>cd src-tauri && cargo check -p uc-infra -p uc-tauri 2>&1 | tail -5 && cargo test -p uc-infra -- chunked_transfer 2>&1 | tail -10</automated>
</verify>
<done> - ciphertext_len is validated against [TAG_SIZE, CHUNK_SIZE+TAG_SIZE] before allocation - Sentinel file is only created inside Ok(entries) arm - NoopPort::put returns Some(data.len() as i64) instead of None - cargo check passes for uc-infra and uc-tauri - Existing chunked_transfer tests pass
</done>
</task>

<task type="auto">
  <name>Task 3: Full test suite verification</name>
  <files></files>
  <action>
Run the full cargo test suite from src-tauri/ to confirm no regressions. All existing tests must pass.

If any test fails due to the changes (e.g., a test that asserts `Ok(Skipped)` for decrypt failures now gets `Err`), update that test to assert the new `Err` behavior — the test expectations should match the corrected semantics.

Do NOT modify production code in this task — only test assertions if needed.
</action>
<verify>
<automated>cd src-tauri && cargo test 2>&1 | tail -20</automated>
</verify>
<done> - All tests pass (0 failures) - No new warnings from cargo check
</done>
</task>

</tasks>

<verification>
After all tasks complete:
1. `cd src-tauri && cargo check` — zero errors, zero warnings
2. `cd src-tauri && cargo test` — all tests pass
3. Manual spot-check: grep for `Ok(InboundApplyOutcome::Skipped)` in sync_inbound.rs — should only appear for the "no representations" case, NOT for decrypt/deserialize failures
4. grep for `rep.bytes.clone()` in sync_outbound.rs — should not exist
5. grep for `ciphertext_len` in chunked_transfer.rs — bounds check should appear before vec allocation
</verification>

<success_criteria>

- 6 review findings fixed as specified
- 2 findings explicitly deferred with documented justification (AAD protocol change, spawn_blocking abort)
- All existing tests pass with no regressions
- cargo check clean (no errors, no new warnings)
  </success_criteria>

<output>
After completion, create `.planning/quick/1-verify-and-fix-review-findings-across-uc/1-SUMMARY.md`
</output>
