# Architecture Principles

UniClipboard follows **Hexagonal Architecture** (also known as **Ports and Adapters**). This document explains the core principles and how they apply to our codebase.

## What is Hexagonal Architecture?

Hexagonal Architecture is a pattern that separates **business logic** from **external concerns** (database, network, UI, OS APIs).

### The Core Idea

> **The application should be independent of all external details.**
>
> Business logic should not know about:
>
> - Which database we use (SQLite, PostgreSQL, in-memory)
> - How we store files (local disk, S3, memory)
> - How we communicate (WebSocket, HTTP, libp2p)
> - What OS we run on (macOS, Windows, Linux)

Instead, the application defines **interfaces (Ports)** for what it needs, and external details implement those interfaces as **Adapters**.

## The Three Layers

### Layer 1: Core Domain (uc-core)

**Purpose**: Define the business model and interfaces (Ports)

**Contains**:

- Domain entities (Clipboard, Device, Encryption, Network)
- Port definitions (traits that define what the application needs)
- Pure business rules (no external dependencies)

**Key Rule**: **Zero dependencies on infrastructure or platform code**

```rust
// uc-core/src/ports/clipboard.rs
/// Port: What the application needs from clipboard storage
pub trait ClipboardRepositoryPort: Send + Sync {
    fn save(&self, content: ClipboardContent) -> Result<(), RepoError>;
    fn find_by_id(&self, id: ClipboardId) -> Result<Option<ClipboardContent>, RepoError>;
}

// uc-core/src/clipboard/mod.rs
/// Domain model: Pure business entity
#[derive(Debug, Clone)]
pub struct ClipboardContent {
    pub id: ClipboardId,
    pub content_type: ContentType,
    pub representations: Vec<Representation>,
    // No database types, no framework types, just business concepts
}
```

### Layer 2: Application Layer (uc-app)

**Purpose**: Orchestrate business logic using only Ports

**Contains**:

- Use cases (application business workflows)
- Application state management
- Event handling

**Key Rule**: **Depend only on uc-core (Ports), not on implementations**

```rust
// uc-app/src/use_cases/sync_clipboard.rs
use uc_core::ports::{ClipboardRepositoryPort, NetworkPort};

pub struct SyncClipboardUseCase {
    // Depend on ABSTRACTIONS, not implementations
    clipboard_repo: Arc<dyn ClipboardRepositoryPort>,
    network: Arc<dyn NetworkPort>,
}

impl SyncClipboardUseCase {
    pub fn execute(&self, content: ClipboardContent) -> Result<(), UseCaseError> {
        // Business logic: Save locally, then broadcast
        self.clipboard_repo.save(content.clone())?;
        self.network.broadcast(content)?;
        Ok(())
    }
}
```

### Layer 3: Adapters (uc-infra + uc-platform)

**Purpose**: Implement Ports to connect to external systems

**Infrastructure Adapters (uc-infra)**:

- Database repositories (SQLite implementation of ClipboardRepositoryPort)
- File system storage (BlobStoragePort implementation)
- Encryption (EncryptionPort with XChaCha20-Poly1305)

**Platform Adapters (uc-platform)**:

- OS clipboard access (macOS pasteboard, Windows clipboard)
- Network layer (libp2p implementation)
- System notifications

**Key Rule**: **Implement Port traits, don't contain business logic**

```rust
// uc-infra/src/db/repositories/clipboard.rs
use uc_core::ports::ClipboardRepositoryPort;

/// Adapter: SQLite implementation of clipboard storage
pub struct SqliteClipboardRepository {
    pool: SqlitePool,
}

impl ClipboardRepositoryPort for SqliteClipboardRepository {
    fn save(&self, content: ClipboardContent) -> Result<(), RepoError> {
        // Map domain entity to database model
        let entity = ClipboardEntity::from_domain(content);
        // Execute SQL query
        // Return result
    }
}
```

## Dependency Flow

```
┌─────────────────────────────────────────────────────────────┐
│                    uc-tauri (Bootstrap)                     │
│  The ONLY place that can depend on all layers simultaneously │
└─────────────────────────────────────────────────────────────┘
                            ↓ creates
┌─────────────────────────────────────────────────────────────┐
│                      uc-app                                 │
│  Depends on: uc-core (Ports)                                 │
│  Must NOT depend on: uc-infra, uc-platform                  │
└─────────────────────────────────────────────────────────────┘
                            ↓ defines
┌─────────────────────────────────────────────────────────────┐
│                      uc-core                                │
│  Defines: Ports (interfaces)                                 │
│  Depends on: Nothing external                                │
└─────────────────────────────────────────────────────────────┘
                            ↑ implements
        ┌───────────────────┴───────────────────┐
        │                                       │
┌──────────────────┐                  ┌──────────────────┐
│   uc-infra       │                  │  uc-platform     │
│  Implements:     │                  │  Implements:     │
│  - Repo Ports    │                  │  - OS Ports      │
│  - Crypto Ports  │                  │  - Network Ports │
└──────────────────┘                  └──────────────────┘
```

**Iron Rule**: `uc-app` → `uc-core` ← `uc-infra` / `uc-platform`

- Application layer depends on Core (interfaces)
- Infrastructure and Platform implement Core interfaces
- No circular dependencies

## Key Principles

### 1. Dependency Inversion

**Traditional Layered Architecture** (WRONG):

```
UI → Service → Repository → Database
      ↓         ↓
   Business logic depends on database implementation
```

**Hexagonal Architecture** (CORRECT):

```
UI → Use Case → Port ← Repository → Database
      ↓                      ↑
   Business logic       Implementation
   depends on           depends on
   abstraction          business interface
```

**In Code**:

```rust
// ❌ WRONG: Use case depends on concrete implementation
pub struct SyncClipboardUseCase {
    repo: SqliteClipboardRepository,  // Concrete!
}

// ✅ CORRECT: Use case depends on abstraction
pub struct SyncClipboardUseCase {
    repo: Arc<dyn ClipboardRepositoryPort>,  // Interface!
}
```

### 2. External Isolation

All external dependencies are hidden behind Ports:

| External Concern | Port Interface            | Adapter Implementation               |
| ---------------- | ------------------------- | ------------------------------------ |
| SQLite Database  | `ClipboardRepositoryPort` | `SqliteClipboardRepository`          |
| File System      | `BlobStoragePort`         | `FileSystemBlobStorage`              |
| OS Clipboard     | `ClipboardPort`           | `MacOSClipboard`, `WindowsClipboard` |
| Network (libp2p) | `NetworkPort`             | `Libp2pNetworkAdapter`               |
| Encryption       | `EncryptionPort`          | `AesGcmEncryption`                   |
| Keyring          | `KeyringPort`             | `SystemKeyringAdapter`               |

**Benefit**: Swap any implementation without changing use cases.

### 3. Testability Without Mocks

Because `uc-app` depends only on traits, we can test with test doubles:

```rust
// Test use case without real database
#[cfg(test)]
mod tests {
    use super::*;
    use uc_core::ports::ClipboardRepositoryPort;

    struct InMemoryClipboardRepo {
        items: Vec<ClipboardContent>,
    }

    impl ClipboardRepositoryPort for InMemoryClipboardRepo {
        fn save(&self, content: ClipboardContent) -> Result<(), RepoError> {
            // Store in memory
        }
    }

    #[test]
    fn test_sync_clipboard() {
        let repo = Arc::new(InMemoryClipboardRepo::new());
        let network = Arc::new(FakeNetwork::new());
        let use_case = SyncClipboardUseCase::new(repo, network);

        // Test business logic without real DB or network
    }
}
```

### 4. Clear Module Boundaries

Each crate has a specific responsibility:

| Crate         | Responsibility           | May Depend On    | Must NOT Depend On           |
| ------------- | ------------------------ | ---------------- | ---------------------------- |
| `uc-core`     | Domain models + Ports    | Nothing external | ❌ Database, OS, Frameworks  |
| `uc-app`      | Use cases, orchestration | `uc-core` only   | ❌ `uc-infra`, `uc-platform` |
| `uc-infra`    | Infrastructure adapters  | `uc-core`        | ❌ `uc-app`, business logic  |
| `uc-platform` | Platform adapters        | `uc-core`        | ❌ `uc-app`, business logic  |
| `uc-tauri`    | Bootstrap, wiring        | All crates       | ❌ Business decisions        |

See [Module Boundaries](module-boundaries.md) for detailed rules.

## Commands Layer (Driving Adapter)

The **Commands Layer** (`uc-tauri/src/commands/`) is a **Driving Adapter** that translates external requests into use case executions.

### Architecture Position

```
Frontend (React)
    ↓ Tauri IPC
Commands Layer (Driving Adapter)
    ↓ runtime.usecases().xxx()
UseCases Layer (Application)
    ↓ execute()
Ports (Interface Layer)
    ↓
Adapters (Infrastructure/Platform)
```

### Mandatory Rules

1. **Always use UseCases accessor** - Commands MUST call `runtime.usecases().xxx()` to get use case instances
2. **Never access Ports directly** - Commands MUST NOT call `runtime.deps.xxx` ports
3. **Use map_err for errors** - All error conversion must use the `map_err` utility
4. **Convert parameters** - Frontend types → domain models before passing to use cases
5. **Convert results** - Domain models → DTOs before returning to frontend

### Example

```rust
// ✅ CORRECT - Through UseCases accessor
#[tauri::command]
pub async fn initialize_encryption(
    runtime: State<'_, AppRuntime>,
    passphrase: String,
) -> Result<(), String> {
    let uc = runtime.usecases().initialize_encryption();
    uc.execute(Passphrase(passphrase))
        .await
        .map_err(map_err)?;
    Ok(())
}

// ❌ FORBIDDEN - Direct Port access
#[tauri::command]
pub async fn initialize_encryption(
    runtime: State<'_, AppRuntime>,
    passphrase: String,
) -> Result<(), String> {
    // Directly calling ports - VIOLATES ARCHITECTURE
    runtime.deps.encryption.derive_kek(...).await?;
    runtime.deps.key_material.store_keyslot(...).await?;
    Ok(())
}
```

**Key Point**: Commands Layer is a thin adapter layer. All business logic MUST be in Use Cases. See [Commands Layer Specification](commands-layer-specification.md) for complete details.

## How Bootstrap Works

**Bootstrap** is the "wiring operator" that assembles everything:

```rust
// uc-tauri/src/bootstrap/wiring.rs
pub fn wire_dependencies() -> Result<AppDeps, WiringError> {
    // Create infrastructure implementations
    let db_pool = create_db_pool()?;
    let clipboard_repo = Arc::new(SqliteClipboardRepository::new(db_pool));

    // Create platform implementations
    let clipboard_adapter = Arc::new(MacOSClipboard::new()?);
    let network = Arc::new(Libp2pNetwork::new()?);

    // Inject into application
    Ok(AppDeps {
        clipboard_repo,
        clipboard_adapter,
        network,
        // ... all other dependencies
    })
}
```

**Key Point**: Only `uc-tauri::bootstrap` knows about concrete implementations. `uc-app` only sees trait objects.

See [Bootstrap System](bootstrap.md) for complete details.

## Common Mistakes to Avoid

### ❌ Mistake 1: Use Case Depends on Implementation

```rust
// ❌ WRONG
use uc_infra::db::SqliteClipboardRepository;

pub struct MyUseCase {
    repo: SqliteClipboardRepository,  // Concrete dependency!
}
```

**Fix**: Depend on the Port trait:

```rust
// ✅ CORRECT
use uc_core::ports::ClipboardRepositoryPort;

pub struct MyUseCase {
    repo: Arc<dyn ClipboardRepositoryPort>,  // Abstract dependency
}
```

### ❌ Mistake 2: Port Defines Implementation Details

```rust
// ❌ WRONG: Port exposes SQL types
pub trait ClipboardRepositoryPort {
    fn save(&self, content: ClipboardContent) -> Result<(), SqliteError>;
}
```

**Fix**: Port uses domain error types:

```rust
// ✅ CORRECT: Port uses domain error
pub trait ClipboardRepositoryPort {
    fn save(&self, content: ClipboardContent) -> Result<(), RepoError>;
}
```

### ❌ Mistake 3: Business Logic in Adapter

```rust
// ❌ WRONG: Adapter contains business rule
impl ClipboardRepositoryPort for SqliteClipboardRepository {
    fn save(&self, content: ClipboardContent) -> Result<(), RepoError> {
        if content.is_empty() {
            return Err(RepoError::Invalid("Empty content not allowed".into()));
        }
        // Business rule belongs in use case, not adapter!
    }
}
```

**Fix**: Move validation to use case:

```rust
// ✅ CORRECT: Adapter just stores
impl ClipboardRepositoryPort for SqliteClipboardRepository {
    fn save(&self, content: ClipboardContent) -> Result<(), RepoError> {
        // Just persist to DB
    }
}

// Validation in use case
impl SyncClipboardUseCase {
    pub fn execute(&self, content: ClipboardContent) -> Result<(), UseCaseError> {
        if content.is_empty() {
            return Err(UseCaseError::InvalidContent);
        }
        self.repo.save(content)?;
    }
}
```

### ❌ Mistake 4: Circular Dependencies

```
❌ WRONG: uc-app → uc-infra → uc-app
```

**Fix**: All dependencies point toward `uc-core`:

```
✅ CORRECT:
uc-app → uc-core ← uc-infra
         ↑
         (Ports defined here)
```

## How to Apply This Architecture

### Adding a New Feature

1. **Define Port in uc-core** (if needed)
2. **Implement Use Case in uc-app** (using only Ports)
3. **Implement Adapter in uc-infra/uc-platform** (implementing Port)
4. **Wire in Bootstrap** (inject adapter into use case)
5. **Expose via Tauri Command** (if needed by UI)

### Example: Adding "Export to File" Feature

**Step 1: Define Port** (uc-core/src/ports/export.rs)

```rust
pub trait ExportPort {
    fn export_to_json(&self, items: Vec<ClipboardContent>) -> Result<PathBuf, ExportError>;
}
```

**Step 2: Implement Use Case** (uc-app/src/use_cases/export_history.rs)

```rust
pub struct ExportHistoryUseCase {
    clipboard: Arc<dyn ClipboardRepositoryPort>,
    exporter: Arc<dyn ExportPort>,
}

impl ExportHistoryUseCase {
    pub fn execute(&self) -> Result<PathBuf, UseCaseError> {
        let items = self.clipboard.find_all()?;
        self.exporter.export_to_json(items)
            .map_err(|e| UseCaseError::ExportFailed(e.to_string()))
    }
}
```

**Step 3: Implement Adapter** (uc-infra/src/export/json_exporter.rs)

```rust
pub struct JsonFileExporter;

impl ExportPort for JsonFileExporter {
    fn export_to_json(&self, items: Vec<ClipboardContent>) -> Result<PathBuf, ExportError> {
        // Write JSON to file
    }
}
```

**Step 4: Wire in Bootstrap** (uc-tauri/src/bootstrap/wiring.rs)

```rust
let exporter = Arc::new(JsonFileExporter::new());

Ok(AppDeps {
    // ... existing dependencies
    exporter,
})
```

## Benefits in Practice

### We Can:

- ✅ Test use cases without real database/network
- ✅ Swap SQLite → PostgreSQL without changing use cases
- ✅ Replace libp2p with different P2P library
- ✅ Run business logic in CLI, GUI, or server
- ✅ Mock external dependencies for fast tests

### We Cannot:

- ❌ Accidentally couple business logic to database
- ❌ Silently introduce framework dependencies in domain
- ❌ Create spaghetti dependencies between crates
- ❌ Make untestable code (Rust compiler prevents it)

## Further Reading

- [Commands Layer Specification](commands-layer-specification.md) - Commands layer architecture rules
- [Module Boundaries](module-boundaries.md) - Detailed rules for each crate
- [Bootstrap System](bootstrap.md) - How dependency injection works
- [Error Handling](../guides/error-handling.md) - Error handling strategy
- [Overview](../overview.md) - Project overview and crate structure

## Architecture Decision: Use `dyn Port` in uc-app Use Cases

### Decision / 决定

**Use cases in `uc-app` MUST use `dyn Port` trait objects instead of generic type parameters.**

```rust
// ✅ CORRECT - Use trait objects
pub struct InitializeEncryption {
    encryption: Arc<dyn EncryptionPort>,
    key_material: Arc<dyn KeyMaterialPort>,
    // ...
}

// ❌ FORBIDDEN - Use generics
pub struct InitializeEncryption<E, K, ...>
where
    E: EncryptionPort,
    K: KeyMaterialPort,
{
    encryption: Arc<E>,
    key_material: Arc<K>,
    // ...
}
```

### Rationale / 理由

#### 1. Type Stability / 类型稳定性

**Problem with generics**: Each unique combination of generic parameters creates a distinct type.

```rust
// ❌ WRONG - Each concrete adapter creates a different type
type InitCase1 = InitializeEncryption<RustCryptoAdapter, SystemKeyring, ...>;
type InitCase2 = InitializeEncryption<MockCrypto, InMemoryKeyring, ...>;

// These are DIFFERENT types at compile time!
// Cannot store in a single UseCases struct without more generics
```

**Solution with `dyn Port`**: Trait objects provide type erasure, creating a single stable type.

```rust
// ✅ CORRECT - Single type regardless of concrete implementation
type InitializeEncryptionUseCase = InitializeEncryption<
    Arc<dyn EncryptionPort>,
    Arc<dyn KeyMaterialPort>,
    Arc<dyn KeyScopePort>,
    Arc<dyn EncryptionStatePort>,
>;

// This is ONE type that works with any concrete port implementation
```

#### 2. Simplified Bootstrap / 简化的引导

**With generics**: Bootstrap code must expose all type parameters, making it fragile to implementation changes.

```rust
// ❌ WRONG - Bootstrap knows about concrete types
impl UseCases<'_> {
    pub fn initialize_encryption(&self) -> InitializeEncryption<
        RustCryptoAdapter,  // Concrete type leaks here!
        SystemKeyring,
        DefaultKeyScope,
        SqliteEncryptionState,
    > {
        InitializeEncryption::new(
            self.runtime.deps.encryption.clone(),
            // ...
        )
    }
}
```

**With `dyn Port`**: Bootstrap only depends on trait interfaces.

```rust
// ✅ CORRECT - Bootstrap depends on abstractions only
impl UseCases<'_> {
    pub fn initialize_encryption(&self) -> InitializeEncryptionUseCase {
        InitializeEncryptionUseCase::new(
            self.runtime.deps.encryption.clone(),  // Arc<dyn EncryptionPort>
            self.runtime.deps.key_material.clone(),
            self.runtime.deps.key_scope.clone(),
            self.runtime.deps.encryption_state.clone(),
        )
    }
}
```

#### 3. Testing Benefits / 测试优势

**With generics**: Tests must either:

- Use the same concrete types as production (not true unit tests)
- Create complex generic type wrappers

**With `dyn Port`**: Tests can easily inject mock implementations.

```rust
// ✅ Easy testing with trait objects
#[cfg(test)]
mod tests {
    use super::*;

    struct MockEncryptionPort;
    impl EncryptionPort for MockEncryptionPort {
        fn derive_kek(&self, _: &Passphrase, _: &Salt, _: &KdfParams) -> Result<Key, EncryptionError> {
            Ok(Key::new([0u8; 32]))
        }
    }

    #[test]
    fn test_initialize_encryption() {
        let encryption = Arc::new(MockEncryptionPort) as Arc<dyn EncryptionPort>;
        let use_case = InitializeEncryptionUseCase::new(encryption, /* other mocks */);
        // Test business logic without real crypto
    }
}
```

#### 4. Dependency Direction / 依赖方向

The `dyn Port` pattern enforces the **Dependency Inversion Principle**:

```text
✅ CORRECT dependency flow:
uc-app use cases → uc-core Port traits ← uc-infra implementations

❌ WRONG with generics:
uc-app use cases → uc-core Port traits
uc-app also knows about → uc-infra concrete types (via generics!)
```

With `dyn Port`, `uc-app` truly depends only on abstractions defined in `uc-core`.

### Trade-offs / 权衡

#### Disadvantages / 劣势

1. **Runtime overhead**: Dynamic dispatch has a small performance cost (virtual function call)
   - **Impact**: Negligible for I/O-bound operations (database, network, crypto)
   - **Acceptable**: Our use cases are not performance-critical loops

2. **No compile-time specialization**: Cannot optimize for specific port implementations
   - **Impact**: Minimal - ports are thin wrappers around external operations
   - **Benefit**: Simpler code, easier refactoring

#### Advantages / 优势

1. **Type stability**: Single type regardless of implementation
2. **Simplified bootstrap**: No generic type explosion
3. **Easy testing**: Direct mock injection without type wrappers
4. **Better separation**: `uc-app` truly depends only on abstractions
5. **Refactoring safety**: Can swap implementations without changing use case types

### Implementation Pattern / 实现模式

#### Use Case Definition

```rust
// uc-app/src/usecases/initialize_encryption.rs
use uc_core::ports::{EncryptionPort, KeyMaterialPort, KeyScopePort, EncryptionStatePort};
use std::sync::Arc;

pub struct InitializeEncryption {
    encryption: Arc<dyn EncryptionPort>,
    key_material: Arc<dyn KeyMaterialPort>,
    key_scope: Arc<dyn KeyScopePort>,
    encryption_state_repo: Arc<dyn EncryptionStatePort>,
}

impl InitializeEncryption {
    pub fn new(
        encryption: Arc<dyn EncryptionPort>,
        key_material: Arc<dyn KeyMaterialPort>,
        key_scope: Arc<dyn KeyScopePort>,
        encryption_state_repo: Arc<dyn EncryptionStatePort>,
    ) -> Self {
        Self {
            encryption,
            key_material,
            key_scope,
            encryption_state_repo,
        }
    }

    pub async fn execute(&self, passphrase: Passphrase) -> Result<(), UseCaseError> {
        // Business logic using trait objects
        let kek = self.encryption
            .derive_kek(&passphrase, &salt, &kdf)
            .await?;
        // ...
    }
}
```

#### Type Alias (Optional but Recommended)

```rust
// uc-app/src/usecases/mod.rs
pub type InitializeEncryptionUseCase = InitializeEncryption<
    Arc<dyn EncryptionPort>,
    Arc<dyn KeyMaterialPort>,
    Arc<dyn KeyScopePort>,
    Arc<dyn EncryptionStatePort>,
>;
```

#### UseCases Accessor

```rust
// uc-tauri/src/bootstrap/runtime.rs
impl UseCases<'_> {
    pub fn initialize_encryption(&self) -> InitializeEncryptionUseCase {
        InitializeEncryptionUseCase::new(
            self.runtime.deps.encryption.clone(),
            self.runtime.deps.key_material.clone(),
            self.runtime.deps.key_scope.clone(),
            self.runtime.deps.encryption_state.clone(),
        )
    }
}
```

### When to Use This Pattern

**Apply to**: All use cases in `uc-app` that depend on ports

**Exceptions**: None - this is the mandatory pattern for our architecture

### References / 参考

- [Dynamic Dispatch in Rust](https://doc.rust-lang.org/book/ch17-02-trait-objects.html)
- [Trait Objects vs. Generics](https://doc.rust-lang.org/book/ch17-02-trait-objects.html#trait-objects-perform-dynamic-dispatch)
- [Dependency Inversion Principle](https://en.wikipedia.org/wiki/Dependency_inversion_principle)
