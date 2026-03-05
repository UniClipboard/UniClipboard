---
phase: quick-03
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
  - src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs
  - src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs
  - src-tauri/crates/uc-infra/src/clipboard/chunked_transfer.rs
autonomous: true
requirements: [REVIEW-R3]
must_haves:
  truths:
    - 'snapshot.clone() removed from V2 inbound path, avoiding unnecessary memory duplication'
    - 'V2 blob purge logic skips files with UCBL magic bytes, protecting valid V2 blobs'
    - 'Chunked transfer decoder uses checked arithmetic for overflow safety on 32-bit targets'
  artifacts:
    - path: 'src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs'
      provides: 'Clone-free snapshot write in V2 inbound'
      contains: 'self.local_clipboard.write_snapshot(snapshot)'
    - path: 'src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs'
      provides: 'V2-aware blob purge logic'
      contains: 'is_v2_blob'
    - path: 'src-tauri/crates/uc-infra/src/clipboard/chunked_transfer.rs'
      provides: 'Overflow-safe capacity check'
      contains: 'checked_mul'
  key_links:
    - from: 'wiring.rs purge logic'
      to: 'BLOB_MAGIC in encrypted_blob_store.rs'
      via: 'Same UCBL magic bytes [0x55, 0x43, 0x42, 0x4C] used for detection'
      pattern: '0x55.*0x43.*0x42.*0x4C'
---

<objective>
Fix three verified code review findings from round 3: remove unnecessary snapshot clone in V2 inbound path, make blob purge V2-aware, and add overflow-safe arithmetic in chunked transfer decoder.

Purpose: Reduce peak memory usage, prevent accidental deletion of valid V2 blobs during migration retries, and ensure correctness on 32-bit targets.
Output: Three targeted fixes with passing tests.

Findings triaged:

- Finding 1 (snapshot.clone): VALID -- fix
- Finding 2 (wiring.rs purge): VALID -- fix
- Finding 3 (sync_outbound.rs streaming): REJECTED -- requires port trait redesign (encrypt takes &[u8], not Read); architectural change out of scope
- Finding 4 (2-PLAN.md code blocks): REJECTED -- completed plan document, cosmetic-only, zero value
- Finding 5 (chunked_transfer overflow): VALID -- fix
  </objective>

<execution_context>
@/home/wuy6/.claude/get-shit-done/workflows/execute-plan.md
@/home/wuy6/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs
@src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs
@src-tauri/crates/uc-infra/src/clipboard/chunked_transfer.rs
@src-tauri/crates/uc-infra/src/security/encrypted_blob_store.rs

<interfaces>
From src-tauri/crates/uc-core/src/ports/clipboard/local_clipboard.rs:
```rust
pub trait SystemClipboardPort: Send + Sync {
    fn read_snapshot(&self) -> Result<SystemClipboardSnapshot>;
    fn write_snapshot(&self, snapshot: SystemClipboardSnapshot) -> Result<()>;
}
```

From src-tauri/crates/uc-infra/src/security/encrypted_blob_store.rs:

```rust
/// Magic bytes identifying a UniClipboard blob file ("UCBL")
const BLOB_MAGIC: [u8; 4] = [0x55, 0x43, 0x42, 0x4C];
```

From src-tauri/crates/uc-infra/src/clipboard/chunked_transfer.rs:

```rust
pub const CHUNK_SIZE: usize = 256 * 1024; // 256 KiB
```

</interfaces>
</context>

<tasks>

<task type="auto">
  <name>Task 1: Remove unnecessary snapshot.clone() in V2 inbound path</name>
  <files>src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs</files>
  <action>
At line 498, change:
```rust
if let Err(err) = self.local_clipboard.write_snapshot(snapshot.clone()) {
```
To:
```rust
if let Err(err) = self.local_clipboard.write_snapshot(snapshot) {
```

Rationale: `write_snapshot` takes ownership (`snapshot: SystemClipboardSnapshot`). The `if self.mode.allow_os_write()` block (lines 490-507) always returns early (success at line 506, error at line 503). The other `snapshot` usage at line 516 (`execute_with_origin(snapshot, ...)`) is in a mutually exclusive branch (only reached when `allow_os_write()` is false). Therefore the `.clone()` is dead allocation -- `snapshot` is never used after `write_snapshot` within this branch.
</action>
<verify>
<automated>cd /home/wuy6/myprojects/UniClipboard/src-tauri && cargo test -p uc-app --lib 2>&1 | tail -20</automated>
</verify>
<done> - `snapshot.clone()` removed from line 498 - `snapshot` passed directly to `write_snapshot` - All uc-app tests pass
</done>
</task>

<task type="auto">
  <name>Task 2: Make blob purge V2-aware and add overflow-safe chunked transfer arithmetic</name>
  <files>src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs, src-tauri/crates/uc-infra/src/clipboard/chunked_transfer.rs</files>
  <action>
**2a. wiring.rs -- V2-aware blob purge (lines 614-659)**

Add a helper function before the wiring code (or as a closure) to detect V2 blobs:

```rust
/// Check if a file starts with the UCBL binary format magic bytes.
/// V2 blobs use magic [0x55, 0x43, 0x42, 0x4C] ("UCBL").
fn is_v2_blob(path: &std::path::Path) -> bool {
    const UCBL_MAGIC: [u8; 4] = [0x55, 0x43, 0x42, 0x4C];
    std::fs::File::open(path)
        .and_then(|mut f| {
            use std::io::Read;
            let mut buf = [0u8; 4];
            f.read_exact(&mut buf)?;
            Ok(buf == UCBL_MAGIC)
        })
        .unwrap_or(false)
}
```

In the purge loop (line 628-638), change the deletion logic to skip V2 blobs:

```rust
if entry.path().is_file() {
    // Skip files that are already in V2 UCBL format
    if is_v2_blob(&entry.path()) {
        continue;
    }
    if let Err(e) = std::fs::remove_file(entry.path()) {
        // ... existing error handling
    }
}
```

Also skip the sentinel file itself from deletion (`.v2_migrated`):

```rust
if entry.path().is_file() {
    let path = entry.path();
    // Skip the sentinel file and valid V2 blobs
    if path.file_name().map_or(false, |n| n == ".v2_migrated") {
        continue;
    }
    if is_v2_blob(&path) {
        continue;
    }
    // ... existing deletion logic
}
```

This ensures that if a previous startup had errors (sentinel not created), new V2 blobs written between startups are preserved on retry.

**2b. chunked_transfer.rs -- Overflow-safe arithmetic (lines 227-234)**

Replace the unchecked multiplication on line 227:

```rust
if total_plaintext_len > total_chunks as usize * CHUNK_SIZE {
```

With checked arithmetic:

```rust
let max_capacity = (total_chunks as usize)
    .checked_mul(CHUNK_SIZE)
    .ok_or_else(|| ChunkedTransferError::InvalidHeader {
        reason: format!(
            "total_chunks {} * CHUNK_SIZE {} overflows usize",
            total_chunks, CHUNK_SIZE
        ),
    })?;
if total_plaintext_len > max_capacity {
    return Err(ChunkedTransferError::InvalidHeader {
        reason: format!(
            "total_plaintext_len {} exceeds maximum capacity {} (total_chunks {} * CHUNK_SIZE {})",
            total_plaintext_len, max_capacity, total_chunks, CHUNK_SIZE
        ),
    });
}
```

Also fix the same pattern in the error message format string on line 231 which also computes `total_chunks as usize * CHUNK_SIZE` -- this is now replaced by `max_capacity`.
</action>
<verify>
<automated>cd /home/wuy6/myprojects/UniClipboard/src-tauri && cargo test -p uc-infra -p uc-tauri --lib 2>&1 | tail -20</automated>
</verify>
<done> - `is_v2_blob` helper detects UCBL magic bytes and skips V2 files during purge - Sentinel file also skipped during purge loop - Chunked transfer decoder uses `checked_mul` for overflow-safe capacity validation - Overflow produces `InvalidHeader` error with descriptive message - All uc-infra and uc-tauri tests pass
</done>
</task>

</tasks>

<verification>
```bash
cd /home/wuy6/myprojects/UniClipboard/src-tauri && cargo test -p uc-app -p uc-infra -p uc-tauri --lib 2>&1 | tail -30
```

Verify no `snapshot.clone()` remains in the V2 inbound write path:

```bash
grep -n 'snapshot\.clone()' src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs
```

Verify `is_v2_blob` exists in wiring.rs:

```bash
grep -n 'is_v2_blob' src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs
```

Verify `checked_mul` exists in chunked_transfer.rs:

```bash
grep -n 'checked_mul' src-tauri/crates/uc-infra/src/clipboard/chunked_transfer.rs
```

</verification>

<success_criteria>

- snapshot.clone() removed from sync_inbound.rs V2 inbound path (zero unnecessary memory duplication)
- Blob purge in wiring.rs skips files with UCBL magic bytes (V2 blobs protected during migration retries)
- Chunked transfer decoder uses checked_mul (overflow-safe on all targets including 32-bit)
- All tests pass across uc-app, uc-infra, uc-tauri
  </success_criteria>

<output>
After completion, create `.planning/quick/3-verify-and-fix-code-review-findings-roun/3-SUMMARY.md`
</output>
