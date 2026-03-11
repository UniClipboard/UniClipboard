# Codebase Concerns

**Analysis Date:** 2026-03-11

## Tech Debt

**Architecture Migration in Progress - Hexagonal vs Clean Architecture:**

- Issue: The codebase is mid-migration from Clean Architecture (legacy in `src-tauri/src/`) to Hexagonal Architecture (new in `src-tauri/crates/`). Both patterns coexist.
- Files: `src-tauri/src/` (legacy), `src-tauri/crates/` (new architecture)
- Impact: Code reviewers must understand two architectural patterns; dependency injection duplicated; risk of introducing violations when adding features to the wrong layer
- Fix approach: Continue migration phase by phase; retire `src-tauri/src/` directories once all functionality is migrated

**Large Monolithic Modules:**

- Issue: Several files are extremely large and mix concerns.
  - `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` — 5,227 lines: production DI, mock structs, disabled integration tests, and compile checks all coexist
  - `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` — 1,872 lines: full runtime wiring and all use case accessors
  - `src-tauri/crates/uc-tauri/src/commands/clipboard.rs` — 1,496 lines: too many responsibilities
- Files: Listed above
- Impact: Slow compilation; hard to navigate; tests silently disabled inside production module (`wiring.rs:5042` has `#[ignore]` with SQLite locking note)
- Fix approach: Split `wiring.rs` into `wiring/production.rs`, `wiring/tests.rs`, `wiring/checks.rs`; move disabled integration tests to `src-tauri/tests/`

**Stub Implementations in Platform Layer:**

- Issue: Several platform adapters are incomplete stubs that return errors or emit empty events:
  - `src-tauri/crates/uc-platform/src/adapters/blob.rs` — `PlaceholderBlobWriterPort` always returns `Err("BlobWriterPort not implemented yet")`
  - `src-tauri/crates/uc-tauri/src/services/clipboard_monitor.rs` — monitor loop does nothing except emit a heartbeat; TODOs on lines 36–38 indicate actual capture is not wired
- Files: `src-tauri/crates/uc-platform/src/adapters/blob.rs`, `src-tauri/crates/uc-tauri/src/services/clipboard_monitor.rs`
- Impact: Any code path that reaches these stubs silently fails; clipboard monitoring does not capture content
- Fix approach: Wire `BlobWriterPort` to `BlobWriter` in `uc-infra`; implement clipboard capture in the monitor using `CaptureClipboardUseCase`

**Stub / Empty Use Case File:**

- Issue: `src-tauri/crates/uc-app/src/usecases/change_passphrase.rs` is a 2-byte empty file (just a blank line). The use case does not exist yet.
- Files: `src-tauri/crates/uc-app/src/usecases/change_passphrase.rs`
- Impact: Passphrase change feature is completely missing at the app layer despite being referenced elsewhere
- Fix approach: Implement `ChangePassphrase` use case using existing `EncryptionPort` primitives

**Incomplete Property Implementations in Clipboard List:**

- Issue: `is_encrypted` and `is_favorited` fields in clipboard entry projections are hardcoded to `false`. `pinned` is also hardcoded `false` in the DB mapper.
- Files: `src-tauri/crates/uc-app/src/usecases/clipboard/list_entry_projections/list_entry_projections.rs` (lines 365–366), `src-tauri/crates/uc-infra/src/db/mappers/clipboard_entry_mapper.rs` (line 17)
- Impact: Frontend cannot show encryption or favorite status; API contracts promise these fields but always return `false`
- Fix approach: Implement `is_encrypted` by checking representation encryption metadata; add `pinned`/`is_favorited` columns to the DB schema and corresponding migrations

**Space Access Protocol Uses Wrong Message Variant:**

- Issue: `SpaceAccessNetworkAdapter` sends space-access protocol messages encoded inside `PairingMessage::Busy.reason` JSON (a hack). A dedicated `PairingMessage::SpaceAccess` variant is needed.
- Files: `src-tauri/crates/uc-app/src/usecases/space_access/network_adapter.rs` (line 30)
- Impact: Parsing fragility; protocol boundaries unclear; future protocol changes to `PairingMessage::Busy` could silently break space access
- Fix approach: Add a dedicated `SpaceAccessPayload` variant to `PairingMessage` in `uc-core/src/network/`

**Excessive `unwrap()`/`expect()` in Production Security Code:**

- Issue: `src-tauri/crates/uc-infra/src/security/encryption.rs` has 10+ `.expect()` calls inside `async fn` implementations for key derivation, wrapping, and encryption operations. `src-tauri/crates/uc-infra/src/security/key_material.rs` similarly panics on mutex lock failures.
- Files: `src-tauri/crates/uc-infra/src/security/encryption.rs`, `src-tauri/crates/uc-infra/src/security/key_material.rs`
- Impact: A panic in any encryption routine crashes the entire Tauri process; no error propagation to the caller
- Fix approach: Convert all `.expect()` in `async` implementations to `?`-propagated errors using the existing `EncryptionError` type

**Sync Mutex `lock().unwrap()` on Shared State in App Layer:**

- Issue: `src-tauri/crates/uc-app/src/usecases/setup/orchestrator.rs` (lines 380, 384) and `mark_complete.rs` (lines 50, 54) call `.lock().unwrap()` on a `std::sync::Mutex` inside mock implementations that are also used in production tests. Mutex poison on any panic in a lock-holding task will cascade.
- Files: `src-tauri/crates/uc-app/src/usecases/setup/orchestrator.rs`, `src-tauri/crates/uc-app/src/usecases/setup/mark_complete.rs`
- Impact: Mutex poisoning causes all subsequent lock attempts to panic
- Fix approach: Replace with `lock().unwrap_or_else(|p| p.into_inner())` or use `tokio::sync::Mutex` throughout async paths

**Deprecated `BlobWriterPort::write` Method Not Removed:**

- Issue: `src-tauri/crates/uc-core/src/ports/blob_writer.rs` still contains a legacy `write` method marked `#[deprecated(note = "Use write_if_absent for atomic semantics")]` on line 38. Both the deprecated and replacement methods coexist.
- Files: `src-tauri/crates/uc-core/src/ports/blob_writer.rs`
- Impact: Risk of callers accidentally using non-atomic write; deprecated API noise
- Fix approach: Remove the deprecated method once all callers are migrated to `write_if_absent`

**Console Logging in Frontend Production Code:**

- Issue: Multiple `console.log` and `console.error` calls remain in production frontend code with no log level control.
- Files: `src/App.tsx` (lines 62, 213), `src/hooks/useClipboardEvents.ts` (9 occurrences), `src/contexts/SettingContext.tsx` (multiple)
- Impact: Debug output in production builds; potential leakage of internal state information
- Fix approach: Replace `console.log` with a logging abstraction that is silenced in production; keep `console.error` only for genuine errors

**Dead Code Suppressed with `#[allow(dead_code)]`:**

- Issue: Multiple `#[allow(dead_code)]` annotations scattered across production modules suppress compiler warnings rather than removing unused code.
- Files: `src-tauri/crates/uc-infra/src/security/encryption_state.rs` (lines 11, 14, 19), `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` (lines 323, 1690, 1700, 1711, 1724), `src-tauri/crates/uc-platform/src/runtime/runtime.rs` (lines 27, 29, 33, 36, 38, 93)
- Impact: Code is being built but never executed; inflated binary size; masks genuine errors
- Fix approach: Remove unused code or restructure to expose it through public APIs that are actually called

---

## Known Bugs

**Test Database Locking in Integration Tests:**

- Symptoms: `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs:5042` test disabled with `#[ignore]` because parallel test execution locks the SQLite file
- Files: `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` (line 5042)
- Trigger: `cargo test` with default parallel execution touches the same DB file from multiple test threads
- Workaround: Test is `#[ignore]`d; functionality only verified manually
- Fix approach: Move to `src-tauri/tests/` with `tempfile::TempDir` per test; or use `:memory:` SQLite for isolated tests

**Clipboard Multi-Representation Write is Lossy:**

- Symptoms: When writing a clipboard snapshot with multiple representations (e.g., text + HTML + image), only the last written representation is preserved. Previous writes are overwritten because `clipboard-rs` high-level APIs replace the clipboard on each call.
- Files: `src-tauri/crates/uc-platform/src/clipboard/common.rs` (line 396 comment), called from `src-tauri/crates/uc-platform/src/clipboard/platform/macos.rs`, `windows.rs`, `linux.rs`
- Trigger: Receiving a multi-representation clipboard sync message and writing it to the system clipboard
- Workaround: `#[cfg(debug_assertions)]` warning logged when snapshot has >1 representation; tracked in GitHub issue #92
- Fix approach: Implement platform-specific clipboard writers that atomically write all representations in a single commit (e.g., `NSPasteboardItem` on macOS)

**`EncryptionState::Initializing` Not Recognized:**

- Symptoms: `EncryptionStateRepository::load_state()` in `uc-infra` only returns `Initialized` or `Uninitialized` by checking file existence; the `Initializing` variant exists in the enum but is never returned
- Files: `src-tauri/crates/uc-infra/src/security/encryption_state.rs` (line 45 TODO comment), `src-tauri/crates/uc-core/src/security/state.rs`
- Trigger: App queried during encryption initialization; race condition window
- Workaround: None; callers cannot distinguish between "not yet started" and "in progress"
- Fix approach: Use a separate marker file or atomic flag for the `Initializing` state; persist it before starting key derivation, remove on completion

**Stub Test Files with All Logic Commented Out:**

- Symptoms: Tests pass trivially but test nothing. `representation_repo_test.rs` has 4 `#[tokio::test]` functions whose bodies are entirely commented out. `phase2_integration_test.rs` has 3 test stubs with only TODO comments.
- Files: `src-tauri/crates/uc-infra/src/db/repositories/representation_repo_test.rs`, `src-tauri/crates/uc-app/tests/phase2_integration_test.rs`
- Trigger: Running the test suite — tests pass but provide zero coverage
- Fix approach: Uncomment and implement; the `TestDbExecutor` in `representation_repo_test.rs` is already written and just needs to be wired

**SpoolScanner Recovery Test Ignored:**

- Symptoms: `src-tauri/crates/uc-app/tests/snapshot_cache_integration_test.rs:766` has `#[ignore = "Requires SpoolScanner recovery flow (Task 14)"]`
- Files: `src-tauri/crates/uc-app/tests/snapshot_cache_integration_test.rs`
- Trigger: Spool recovery after crash/restart is not validated
- Fix approach: Implement SpoolScanner recovery flow and re-enable the test

---

## Security Considerations

**Cryptographic Key Material Not Zeroized on Drop:**

- Risk: `MasterKey` and `Kek` types in `uc-core` hold 32-byte key material in plain arrays. Neither implements `Zeroize` or `Drop` to clear memory. Comments explicitly acknowledge this (`model.rs` line 206, line 264).
- Files: `src-tauri/crates/uc-core/src/security/model.rs` (lines 206–208, 264–266)
- Current mitigation: `SecretString` in `uc-core/src/security/secret.rs` does implement `Zeroize` on drop; key types do not
- Recommendations: Add `zeroize` derive or manual `Drop` impl to `MasterKey` and `Kek`; remove `Clone` from `MasterKey` (forces intentional copies); disable `derive(Debug)` on `Kek` to prevent accidental logging

**Encryption Operations Panic Instead of Propagating Errors:**

- Risk: `expect()` calls in `uc-infra/src/security/encryption.rs` for `derive_kek`, `wrap_master_key`, `encrypt_blob`, `decrypt_blob` will panic the process on any crypto library failure (e.g., invalid parameters, RNG failure)
- Files: `src-tauri/crates/uc-infra/src/security/encryption.rs` (lines 194–289)
- Current mitigation: These functions are only called in encryption use cases, reducing surface
- Recommendations: Replace all `expect()` with `map_err()` and propagate as `EncryptionError::CryptoFailure`

**`EncryptionState::Initializing` Race Window:**

- Risk: During encryption initialization, `load_state()` returns `Uninitialized` rather than `Initializing`. If another operation checks encryption state concurrently, it may proceed as if encryption is not set up, potentially operating on plaintext data.
- Files: `src-tauri/crates/uc-infra/src/security/encryption_state.rs`
- Current mitigation: Initialization is expected to be called early in setup flow; concurrent access unlikely but not prevented
- Recommendations: Implement atomic state marker (file lock or dedicated field) for `Initializing` state; document setup sequencing constraints

**`MasterKey` Has `Clone` Derive:**

- Risk: `MasterKey` derives `Clone`, allowing uncontrolled copies of encryption keys to be made anywhere in the codebase. Any clone that is not explicitly zeroized leaks key material.
- Files: `src-tauri/crates/uc-core/src/security/model.rs` (line 207)
- Current mitigation: Debug representation is redacted `"MasterKey([REDACTED])"`
- Recommendations: Remove `Clone` from `MasterKey`; use `Arc<MasterKey>` for shared access

**Silently Dropped IPC Event Sends in Wiring:**

- Risk: Multiple `let _ = payload_tx_clone.try_send(...)` patterns in `wiring.rs` (lines 4054, 4116, 4212, 4258, 4302, 4385, 4461, 4520) silently drop events when the channel is full. This can cause the frontend to miss encryption, pairing, or clipboard events without any warning.
- Files: `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs`
- Current mitigation: None — errors are fully suppressed
- Recommendations: Log a `warn!()` on `try_send` failure; consider using bounded channels with back-pressure or switching to `send().await`

---

## Performance Bottlenecks

**Representation Cache Eviction Confirmed, Spool Queue Depth Unconfigured:**

- Problem: `InMemoryRepresentationCache` in `src-tauri/crates/uc-infra/src/clipboard/representation_cache.rs` has explicit `max_entries` and `max_bytes` limits with LRU eviction — this is properly implemented. However, the `MpscSpoolQueue` channel depth is configured at 8 in tests; production depth is determined by each wiring call site.
- Files: `src-tauri/crates/uc-infra/src/clipboard/representation_cache.rs`, `src-tauri/crates/uc-infra/src/clipboard/spooler_task.rs`
- Cause: No centralized config for spool queue depth
- Improvement path: Expose spool queue depth as an `AppConfig` field; add monitoring for dropped spool requests

**Network Event Channels with Buffer=1:**

- Problem: Multiple `mpsc::channel(1)` usages in `wiring.rs` test helpers (lines 2993, 3049, 3986, 4042) mean any producer that emits two events before the consumer reads will block or drop.
- Files: `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs`
- Cause: Tight buffer sizes appropriate for unit tests but not for production event bursts
- Improvement path: Ensure production channel creation (not test helpers) uses adequate buffer sizes; audit all `mpsc::channel(N)` calls in non-test paths

**Large Clipboard Blobs Loaded Entirely into Memory:**

- Problem: Blob write and read operations in `src-tauri/crates/uc-infra/src/blob/blob_writer.rs` operate on `Vec<u8>` — the entire blob is in memory simultaneously during encryption and write. No streaming or chunked I/O is used.
- Files: `src-tauri/crates/uc-infra/src/blob/blob_writer.rs`
- Cause: Simple implementation; acceptable for clipboard text but problematic for large images (10MB+)
- Improvement path: Add a maximum blob size guard before accepting clipboard content; implement chunked streaming for blobs above a configurable threshold

---

## Fragile Areas

**`wiring.rs` — 5,227-Line God Module:**

- Files: `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs`
- Why fragile: Any change to dependency injection order can silently break startup; test code and production wiring are interleaved; disabled tests inside production module are invisible to CI
- Safe modification: Changes to `wire_dependencies` must be followed by running `cargo test -p uc-tauri` in full; understand the dependency graph documented in `src-tauri/crates/uc-tauri/src/bootstrap/AGENTS.md`; never add business logic to this file
- Test coverage: Integration tests disabled; only unit tests of individual builders run

**Pairing State Machine — 2,277 Lines:**

- Files: `src-tauri/crates/uc-core/src/network/pairing_state_machine.rs`
- Why fragile: Many states × many events = complex transition table; timeout, network failure, and user-cancel paths during pairing can leave state inconsistent
- Safe modification: Always add a test case for new transitions; use the existing transition test harness pattern; verify no unreachable states are introduced
- Test coverage: Tests exist in the same file and `src-tauri/crates/uc-app/tests/setup_flow_integration_test.rs`

**libp2p Network Adapter — Large and Concurrent:**

- Files: `src-tauri/crates/uc-platform/src/adapters/libp2p_network.rs`
- Why fragile: `PeerCaches` hash maps are shared across concurrent tasks; `AtomicU8` used for start state with manual compare-exchange; race-condition note at line 825 ("network start race detected, retrying compare_exchange")
- Safe modification: Do not access peer cache from multiple tasks without synchronization; avoid adding new shared state; add new peer handling code in dedicated sub-modules
- Test coverage: Unit tests at end of file; platform integration tests in `src-tauri/crates/uc-platform/tests/`

**Setup Orchestrator — State/Action Serialization Mutex:**

- Files: `src-tauri/crates/uc-app/src/usecases/setup/orchestrator.rs`
- Why fragile: A serialization mutex prevents concurrent dispatch, but `lock().unwrap()` calls inside mock state objects will propagate panics if any async task panics while holding the lock
- Safe modification: Never hold the setup context lock across `.await` points; treat the orchestrator as single-threaded logically; use the `#[tokio::test]` harness for all orchestrator tests

**Encryption Session Lifecycle:**

- Files: `src-tauri/crates/uc-infra/src/security/key_material.rs`, `src-tauri/crates/uc-platform/src/adapters/encryption.rs`
- Why fragile: 21 `.expect("lock ... state")` calls in `key_material.rs` — a single panic in any thread holding the mutex poisons all subsequent operations; the comment "MasterKey will be zeroized automatically" in `encryption.rs` line 30 relies on `Drop` but `MasterKey` does not implement `Zeroize`
- Safe modification: Wrap mutex lock in `unwrap_or_else(|p| p.into_inner())` as a minimum; do not hold the lock across async operations

**Clipboard Write Snapshot — Lossy Multi-Representation:**

- Files: `src-tauri/crates/uc-platform/src/clipboard/common.rs` (line 396), `src-tauri/crates/uc-platform/src/clipboard/platform/macos.rs`, `windows.rs`, `linux.rs`
- Why fragile: Writing a multi-representation snapshot overwrites the previous representation each time; the last written wins; the debug assertion warns but production silently loses representations
- Safe modification: Do not add new representation types to `write_snapshot` without first implementing atomic multi-rep write; test clipboard write on target platform after any changes

---

## Scaling Limits

**SQLite — Single Writer, Pool Size Unconfigured:**

- Current capacity: Pool created with `Pool::builder()` defaults — no explicit `max_size`; default r2d2 pool size is 10
- Limit: SQLite allows only one writer at a time; concurrent writes block (5s busy timeout configured); no horizontal scaling
- Scaling path: Document the default pool size; add pool utilization metrics; for high-write scenarios, batch writes or use a queue

**Network Peer Cache — Unbounded HashMap:**

- Current capacity: `PeerCaches` in `libp2p_network.rs` uses `HashMap` with no eviction
- Limit: In large LAN environments with many devices, peer cache grows unbounded
- Scaling path: Add TTL-based eviction for stale peers; cap peer cache size at a configurable maximum

---

## Dependencies at Risk

**`src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` in Cargo workspace:**

- Risk: This file is 5,227 lines and re-compiled in full on any change; it depends on all four workspace crates simultaneously, meaning any crate change triggers a full recompile of this module
- Impact: Developer iteration speed; CI build times
- Migration plan: Decompose into smaller modules; evaluate if some dependency wiring can be deferred to runtime via config

---

## Test Coverage Gaps

**Clipboard Representation Repository — All Tests Commented Out:**

- What's not tested: `get_representation`, `update_blob_id`, `update_processing_result` CAS semantics
- Files: `src-tauri/crates/uc-infra/src/db/repositories/representation_repo_test.rs`
- Risk: CAS (compare-and-swap) on representation state transitions could regress undetected; blob reference updates could silently corrupt
- Priority: High — these are core data integrity operations

**Phase 2 Integration Tests — Empty Stubs:**

- What's not tested: `AppDeps` construction, representation materialization (inline vs blob threshold), blob deduplication
- Files: `src-tauri/crates/uc-app/tests/phase2_integration_test.rs`
- Risk: Integration gaps between uc-app and uc-infra layers; blob deduplication logic not exercised
- Priority: High

**SpoolScanner Recovery Flow — Ignored Test:**

- What's not tested: Crash recovery for in-flight spool entries; orphaned spool file cleanup on restart
- Files: `src-tauri/crates/uc-app/tests/snapshot_cache_integration_test.rs` (line 766)
- Risk: After a crash, spool files may never be processed, silently losing clipboard entries
- Priority: High

**Frontend Component Tests — Minimal Coverage:**

- What's not tested: Setup flow UI, encryption initialization, pairing dialogs, device list rendering
- Files: `src/pages/`, `src/components/`
- Risk: UI regressions not caught; state management changes in Redux slices break rendering silently
- Priority: Medium — add Vitest tests for critical flows

**Settings Sync Frequency Validation:**

- What's not tested: Frontend offering sync frequencies the backend doesn't support
- Files: `src/components/setting/SyncSection.tsx` (line 56 TODO comment)
- Risk: User selects an unsupported frequency; backend silently ignores or defaults to realtime
- Priority: Medium

---

## Missing Critical Features

**`change_passphrase` Use Case — Completely Absent:**

- Problem: `src-tauri/crates/uc-app/src/usecases/change_passphrase.rs` is an empty file (2 bytes). No implementation exists.
- Blocks: Users cannot change their encryption passphrase without re-initializing encryption and losing history

**`is_encrypted` / `is_favorited` / `pinned` Fields:**

- Problem: Always return `false` — see Tech Debt section above
- Blocks: Encryption audit, favorites UI, pinned-entry feature

**Autostart on System Boot:**

- Problem: Autostart platform adapters are stubbed or not wired
- Blocks: Background sync disabled after reboot; users must manually launch the app

**Setup State Machine Has No Explicit `Ready` State:**

- Problem: `src-tauri/crates/uc-core/src/setup/state_machine.rs` (line 343 TODO comment) — "Ready state" is emitted externally by `AppLifecycleCoordinator` rather than being a first-class state in the machine
- Blocks: Network startup timing after setup completion is ambiguous; `start_network_after_unlock` behavior depends on external sequencing not captured in the state machine

---

## Summary Table

| Issue Type         | Count | Severity    | Key Files                                                         |
| ------------------ | ----- | ----------- | ----------------------------------------------------------------- |
| Tech Debt          | 9     | Medium-High | `wiring.rs`, `encryption.rs`, `change_passphrase.rs`              |
| Known Bugs         | 5     | Medium-High | `representation_repo_test.rs`, `common.rs`, `encryption_state.rs` |
| Security           | 5     | Medium-High | `model.rs`, `encryption.rs`, `key_material.rs`, `wiring.rs`       |
| Performance        | 3     | Low-Medium  | `blob_writer.rs`, channel buffer sizes, spool depth               |
| Fragile Areas      | 6     | Medium      | `wiring.rs`, `pairing_state_machine.rs`, `libp2p_network.rs`      |
| Test Coverage Gaps | 5     | High        | `representation_repo_test.rs`, `phase2_integration_test.rs`       |
| Scaling Limits     | 2     | Low-Medium  | SQLite pool, peer cache                                           |
| Dependencies       | 1     | Low         | `wiring.rs` compile footprint                                     |
| Missing Features   | 4     | Medium      | passphrase change, encryption status, autostart                   |

---

_Concerns audit: 2026-03-11_
