# UniClipboard Port Definition Specification

## 1. Scope

This specification applies to all abstraction interfaces for external capabilities in `uc-core`, including but not limited to:

- repository / store
- network
- clipboard
- security
- timer / clock
- event emitter / subscriber
- settings / state persistence

The goal is not to unify naming style, but to unify **responsibility boundaries**, **dependency granularity**, and **evolution strategy**.

## 2. General Principles

### 2.1 Ports must target "use-case intent" or "stable capability", not "convenient method grouping"

When defining a port, always ask first:

> Is this interface expressing a specific business action, or a specific foundational capability?

Do not put methods into the same port just because they all relate to `PairedDevice`.

### 2.2 Use cases must not depend on "catch-all" repository interfaces

A use case may only depend on:

- The minimal capability set it actually needs, or
- A stable, naturally indivisible small capability interface.

**It is forbidden** for a use case to directly depend on a "full repository / store interface" unless the use case itself is a low-level aggregate maintenance use case.

### 2.3 Adding a new business action defaults to a new small port, not appending to an old port

This is the most critical rule.

Default strategy:

- New requirement ≠ add a method to an existing port
- New requirement = define a new intent interface first
- Only merge into an existing port when the methods share the **same responsibility**, **same change direction**, and **same consumer set**.

### 2.4 Port stability takes priority over writing convenience

Prefer multiple small traits over fewer large interfaces.
In Rust, once a trait is widely depended upon, the cost of adding methods grows rapidly.

## 3. Port Classification

All ports must belong to exactly one of the following four categories.

### 3.1 Query Port

Read-only. No persistent state changes. No business side effects.

Characteristics:

- Read state
- Query lists
- Get details
- Compute results without writing back

Example:

```rust
#[async_trait]
pub trait FindPairedDeviceByPeerIdPort: Send + Sync {
    async fn get_by_peer_id(
        &self,
        peer_id: &PeerId,
    ) -> Result<Option<PairedDevice>, PairedDeviceRepositoryError>;
}

#[async_trait]
pub trait ListPairedDevicesPort: Send + Sync {
    async fn list_all(&self) -> Result<Vec<PairedDevice>, PairedDeviceRepositoryError>;
}
```

Constraints:

- Must not mix in save / update / delete / emit / publish
- Method names must express read semantics: `get`, `find`, `list`, `load`, `read`

### 3.2 Command Port

Executes actions. Produces state changes or side effects.

Characteristics:

- Write to database
- Update state
- Delete records
- Send network messages
- Push events
- Start / stop capabilities

Example:

```rust
#[async_trait]
pub trait UpdatePairedDeviceStatePort: Send + Sync {
    async fn set_state(
        &self,
        peer_id: &PeerId,
        state: PairingState,
    ) -> Result<(), PairedDeviceRepositoryError>;
}

#[async_trait]
pub trait DeletePairedDevicePort: Send + Sync {
    async fn delete(&self, peer_id: &PeerId) -> Result<(), PairedDeviceRepositoryError>;
}
```

Constraints:

- Must not mix in pure queries
- Method names must express action semantics: `save`, `insert`, `update`, `delete`, `emit`, `send`, `start`, `stop`

### 3.3 Capability Port

Represents a stable, reusable foundational capability — not directly tied to a specific domain entity's read/write.

Applicable scenarios:

- `ClockPort`
- `TimerPort`
- `EncryptionPort`
- `SecureStoragePort`
- `SystemClipboardPort`

Example:

```rust
pub trait ClockPort: Send + Sync {
    fn now_ms(&self) -> i64;
}

#[async_trait]
pub trait EncryptionPort: Send + Sync {
    async fn encrypt_blob(...);
    async fn decrypt_blob(...);
}
```

Constraints:

- Must be a naturally cohesive capability
- Do not mix domain state management into capability ports
- A capability port may contain multiple methods, but they must belong to the same stable capability domain

### 3.4 Event Port

Dedicated to event publishing or subscribing.

Example:

```rust
#[async_trait]
pub trait SetupEventPort: Send + Sync {
    async fn emit_setup_state_changed(
        &self,
        state: SetupState,
        session_id: Option<String>,
    );
}

#[async_trait]
pub trait NetworkEventPort: Send + Sync {
    async fn subscribe_events(
        &self,
    ) -> anyhow::Result<tokio::sync::mpsc::Receiver<NetworkEvent>>;
}
```

Constraints:

- Separate publishing from subscribing where possible
- Do not combine with repository / state update in the same trait

## 4. Port Granularity

### 4.1 A port may only express one responsibility direction

Test: if methods in a trait are depended upon by different use cases in different combinations, the trait is too large.

The following is over-sized:

```rust
// BAD — do not use as a use-case dependency
trait PairedDeviceRepositoryPort {
    async fn get_by_peer_id(...);
    async fn list_all(...);
    async fn upsert(...);
    async fn set_state(...);
    async fn update_last_seen(...);
    async fn delete(...);
    async fn update_sync_settings(...);
    async fn update_last_known_addresses(...);
}
```

It simultaneously expresses: query, full write, state update, field patch, and delete — that is not a single responsibility.

### 4.2 A use case constructor must not depend on a larger parent interface for convenience

Wrong:

```rust
pub struct UpdateAddressUseCase<R: PairedDeviceRepositoryPort> {
    repo: R,
}
```

Correct:

```rust
pub struct UpdateAddressUseCase<R: UpdatePairedDeviceAddressesPort> {
    repo: R,
}
```

Even if the underlying implementation type is the same, the upper layer must not hold a larger capability surface than needed.

### 4.3 Small ports take priority over large repository ports

Default priority:

1. Use-case intent port
2. Stable capability port
3. Low-level store / repository port

Repository / store is **not** the default first choice exposed to use cases.

## 5. Store / Repository Definition

### 5.1 A larger low-level Store is allowed, but only at the inner layer

For persistence adapters, it is acceptable to have:

```rust
#[async_trait]
pub trait PairedDeviceStore: Send + Sync {
    async fn get_by_peer_id(...);
    async fn list_all(...);
    async fn upsert(...);
    async fn set_state(...);
    async fn update_last_seen(...);
    async fn delete(...);
    async fn update_sync_settings(...);
    async fn update_last_known_addresses(...);
}
```

But this interface is positioned as:

- Facing low-level persistence implementation
- For adapter reuse
- **Not** the default interface for use case dependencies

### 5.2 Store vs Port

- `XxxStore` — low-level data access full set
- `XxxPort` — upper-layer dependency interface
- `XxxQueryPort` / `XxxCommandPort` / `FindXxxPort` / `UpdateXxxPort` — fine-grained intent interface

### 5.3 New Store methods must not automatically propagate upward

Even when a Store gains new methods, it does not mean all use-case ports must follow.
Only the use cases that actually consume the new method should add or adjust a corresponding small port.

## 6. Naming

### 6.1 Forbidden vague names

Do not use these as upper-layer generic dependency names:

- `RepositoryPort`
- `ServicePort`
- `ManagerPort`
- `HandlerPort`

These names are too broad and naturally encourage method accumulation.

### 6.2 Query Port naming

Use read-intent naming:

- `FindPairedDeviceByPeerIdPort`
- `ListPairedDevicesPort`
- `LoadSetupStatusPort`
- `GetEncryptionStatePort`

### 6.3 Command Port naming

Use action naming:

- `UpdatePairedDeviceStatePort`
- `DeletePairedDevicePort`
- `SaveSettingsPort`
- `EmitSetupEventPort`

### 6.4 Capability Port naming

Use capability-domain naming:

- `ClockPort`
- `TimerPort`
- `EncryptionPort`
- `SystemClipboardPort`

Prerequisite: it must genuinely be a stable capability, not a miscellaneous catch-all.

## 7. Method Design

### 7.1 Each method must express one clear action

Avoid vague methods:

```rust
// BAD — unless context is extremely clear
async fn save(&self, data: Something) -> Result<()>;
```

Prefer explicit semantics:

```rust
async fn update_last_seen(...)
async fn update_sync_settings(...)
```

### 7.2 Do not mix "full save" and "partial patch" in the same interface without clear layer separation

Example of the anti-pattern (coexisting update models):

```rust
// Both present in one trait — problematic
async fn upsert(&self, device: PairedDevice);         // aggregate full overwrite
async fn update_last_seen(&self, peer_id: &PeerId, last_seen_at: DateTime<Utc>);  // field-level partial update
```

Rule:

- At the use-case layer, prefer intent-specific partial command ports
- `upsert` is only for initialization, import, or full replacement scenarios
- Do not abuse `upsert` in normal state progression flows

### 7.3 Parameters must center on domain semantics, not leak implementation details

Prefer:

```rust
async fn set_state(&self, peer_id: &PeerId, state: PairingState)
```

Avoid:

```rust
async fn update_state_row(&self, id: i64, state_code: i32)
```

### 7.4 Error types must be stable and explicit

Each port should return an error type corresponding to its capability domain:

- `PairedDeviceRepositoryError`
- `EncryptionError`
- `SearchError`

Do not default to `anyhow::Result<T>` in core ports unless the capability is an infrastructure boundary where errors need no further classification.

## 8. Dependency Injection

### 8.1 Use cases inject only the minimal ports they need

Correct:

```rust
pub struct ListDevicesUseCase<P: ListPairedDevicesPort> {
    port: P,
}
```

Wrong:

```rust
pub struct ListDevicesUseCase<P: PairedDeviceRepositoryPort> {
    port: P,
}
```

### 8.2 A use case depending on multiple small ports is normal

Do not merge small ports into a large one just to reduce constructor parameters.

```rust
pub struct SyncPeerStatusUseCase<
    Q: FindPairedDeviceByPeerIdPort,
    U: UpdatePairedDeviceStatePort,
    T: UpdatePairedDeviceLastSeenPort,
> {
    query: Q,
    state_port: U,
    last_seen_port: T,
}
```

This is healthier than depending on one catch-all interface.

### 8.3 Outer wiring may inject the same adapter into multiple small ports

This is the recommended pattern.

For example, `SqlitePairedDeviceRepository` simultaneously implements:

- `FindPairedDeviceByPeerIdPort`
- `ListPairedDevicesPort`
- `UpdatePairedDeviceStatePort`
- `UpdatePairedDeviceAddressesPort`

Upper layers receive small interfaces; lower layers still reuse a single implementation.

## 9. Mock / Testing

### 9.1 Test mocks target small ports, not the low-level large Store

Tests should mock:

- `ListPairedDevicesPort`
- `UpdatePairedDeviceStatePort`

Not default to mocking:

- `PairedDeviceStore`

This way, adding `update_last_known_addresses()` only affects related tests, not all of them.

### 9.2 Centralized mocks are allowed but must be split by domain

Forbidden: a single file maintaining all mocks for the entire system.

Should split by domain:

- `test_mocks/paired_device.rs`
- `test_mocks/setup.rs`
- `test_mocks/clipboard.rs`

### 9.3 Adding a Store method must not force changes to unrelated mocks

If a test does not depend on the new capability, it should not be forced to modify its mock trait.

This is exactly the problem that small port design solves.

## 10. Port Evolution

### 10.1 Before adding a method, perform this checklist

Before adding a method, ask in order:

1. Is this a new intent, or a natural extension of an existing responsibility?
2. Will this method be "passively inherited" by many existing consumers?
3. Does this method serve only a few use cases?
4. Can this be expressed via a new small port instead of extending an old one?

If the answer to 2 or 3 is "yes", prefer creating a new small port.

### 10.2 Conditions for adding a method to an existing port

All of the following must be true:

- The new method belongs to the same responsibility direction as existing methods
- The current consumer set is essentially the same
- Adding it will not significantly expand the dependency surface of unrelated tests or use cases
- The port itself remains small and stable

Otherwise, do not append.

### 10.3 Mandatory split signals

Split if **any** of these are true:

- Contains both reads and writes
- Contains both full save and field patch
- Method count is growing, exceeds 4–6, with a clear upward trend
- Different use cases only use a small subset of its methods
- Mock modifications frequently cascade to many unrelated tests
- The port name has become increasingly vague and no longer represents a single responsibility

## 11. Templates

### 11.1 Query Port template

```rust
#[async_trait]
pub trait FindXxxPort: Send + Sync {
    async fn find_xxx(&self, id: &XxxId) -> Result<Option<Xxx>, XxxError>;
}
```

### 11.2 Command Port template

```rust
#[async_trait]
pub trait UpdateXxxPort: Send + Sync {
    async fn update_xxx(&self, id: &XxxId, value: XxxValue) -> Result<(), XxxError>;
}
```

### 11.3 Capability Port template

```rust
#[async_trait]
pub trait XxxCapabilityPort: Send + Sync {
    async fn do_xxx(&self, input: Input) -> Result<Output, XxxError>;
}
```

If the capability is very stable, naming it directly as `ClockPort` or `EncryptionPort` is fine — no need to force a `Capability` suffix.

## 12. Case Studies

### 12.1 Anti-pattern: `PairedDeviceRepositoryPort`

The current interface should not continue to be used as a direct use-case dependency:

```rust
pub trait PairedDeviceRepositoryPort: Send + Sync {
    async fn get_by_peer_id(...);
    async fn list_all(...);
    async fn upsert(...);
    async fn set_state(...);
    async fn update_last_seen(...);
    async fn delete(...);
    async fn update_sync_settings(...);
    async fn update_last_known_addresses(...);
}
```

The recommended approach:

**Keep as inner layer** — rename to `PairedDeviceStore`.

**Expose small interfaces upward:**

- `FindPairedDeviceByPeerIdPort`
- `ListPairedDevicesPort`
- `UpsertPairedDevicePort`
- `UpdatePairedDeviceStatePort`
- `UpdatePairedDeviceLastSeenPort`
- `UpdatePairedDeviceSyncSettingsPort`
- `UpdatePairedDeviceAddressesPort`
- `DeletePairedDevicePort`

This ensures that adding "update field X" in the future does not drag all consumers down.

### 12.2 Positive example: receiver-side file-transfer projection ports

The receiver keeps a local projection of inbound file transfers, backed by a
single Diesel-backed store. Instead of exposing that store as one
`FileTransferProjectionRepositoryPort` with ~7 methods, `uc-core` exposes it as
**five small intent ports**, split by responsibility direction
(see `crates/uc-core/src/ports/file_transfer.rs`):

```rust
/// Command: write receiver-side projection rows.
pub trait RecordReceiverTransferPort: Send + Sync {
    async fn upsert_pending_transfer(&self, transfer: &PendingInboundTransfer) -> Result<(), FileTransferProjectionError>;
    async fn link_transfer_to_entry(&self, transfer_id: &str, entry_id: &str, now_ms: i64) -> Result<bool, FileTransferProjectionError>;
}

/// Query: aggregate transfer status for a clipboard entry.
pub trait GetEntryTransferSummaryPort: Send + Sync {
    async fn get_entry_transfer_summary(&self, entry_id: &str) -> Result<Option<EntryTransferSummary>, FileTransferProjectionError>;
}

/// Query: resolve the entry a transfer belongs to.
pub trait FindEntryIdForTransferPort: Send + Sync {
    async fn get_entry_id_for_transfer(&self, transfer_id: &str) -> Result<Option<String>, FileTransferProjectionError>;
}

/// Query: list in-flight transfers that have exceeded their deadlines.
pub trait ListExpiredInflightTransfersPort: Send + Sync {
    async fn list_expired_inflight(&self, pending_cutoff_ms: i64, transferring_cutoff_ms: i64) -> Result<Vec<ExpiredInflightTransfer>, FileTransferProjectionError>;
}

/// Command: finalize in-flight transfers as failed.
pub trait FailInflightTransfersPort: Send + Sync {
    async fn mark_failed(&self, transfer_id: &str, reason: &str, now_ms: i64) -> Result<(), FileTransferProjectionError>;
    async fn bulk_fail_inflight(&self, reason: &str, now_ms: i64) -> Result<Vec<ExpiredInflightTransfer>, FileTransferProjectionError>;
}
```

Why this is the shape §11–§13 ask for:

- **One adapter, many ports.** A single inner store implements all five traits;
  the split lives entirely in the port layer, so there is no duplicated state or
  "parallel old/new logic" — this is the §5.1 "larger low-level Store at the inner
  layer" + §8.3 "same adapter injected into multiple small ports" pattern.
- **Each consumer holds the minimal capability it needs.** The write path that
  seeds pending rows depends only on `RecordReceiverTransferPort`; the UI status
  query depends only on `GetEntryTransferSummaryPort`; the restart-cleanup sweep
  depends only on `ListExpiredInflightTransfersPort` + `FailInflightTransfersPort`.
  None of them is forced to know about the others.
- **Query / command separation is explicit.** Read-only facets
  (`Get…` / `Find…` / `List…`) are separated from mutation facets
  (`Record…` / `Fail…`), so a read-side consumer can never accidentally reach a
  mutating method.
- **Adding a facet does not drag anyone down.** A future "pause transfer" command
  becomes a new `PauseInflightTransfersPort`, not a seventh method on a shared
  interface — exactly the outcome §12.1 was trying to reach by splitting
  `PairedDeviceRepositoryPort`.

Contrast the two case studies: §12.1 shows a fat repository that *still needs to
be split*; §12.2 shows the same situation already split correctly. New
repository-shaped capabilities should be born in the §12.2 shape rather than
grown into the §12.1 shape and refactored later.

## 13. The Final Rule

Every time you want to add a method to a port, ask one question:

> After adding this method, will a batch of use cases and tests that don't care about this functionality be forced to know about it?

If the answer is **yes**, do not add it to the original port.
Create a new, smaller port.
