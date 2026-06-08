# Module Boundaries

This document defines the **responsibilities and boundaries** for each crate in the UniClipboard architecture. It serves as a reference for both implementation and code review.

## Quick Reference

| Crate              | Core Responsibility                     | May Depend On                         | Must NOT Depend On                |
| ------------------ | --------------------------------------- | ------------------------------------- | --------------------------------- |
| `uc-core`          | Domain models + Port trait definitions  | Nothing external                      | ❌ Database, OS, Frameworks        |
| `uc-application`   | Use cases, facades, orchestration       | `uc-core` + `uc-observability`        | ❌ `uc-infra`, `uc-platform`       |
| `uc-infra`         | Infrastructure adapters (DB, P2P, crypto)| `uc-core`                            | ❌ `uc-application`, business logic |
| `uc-platform`      | Platform adapters (clipboard, OS, keychain)| `uc-core` + `uc-app-paths`          | ❌ `uc-application`, business logic |
| `uc-bootstrap`     | **唯一组合根** (DI wiring)              | All core/app/infra/platform/observability | ❌ Business decisions           |
| `uc-observability` | Tracing, analytics, redaction           | Nothing external (leaf)               | ❌ Domain logic                    |
| `uc-app-paths`     | Directory layout resolution             | Nothing external (leaf)               | ❌ App-stack logic                 |
| `uc-daemon-contract`| HTTP API transport types (serde)       | `uc-core`                             | ❌ Infrastructure, GUI frameworks  |
| `uc-daemon-process`| Process management (PID, socket, spawn) | `uc-daemon-contract` + `uc-app-paths` | ❌ iroh, diesel, GUI frameworks    |
| `uc-daemon-local`  | Local daemon metadata (auth, health)    | `uc-daemon-contract` + `uc-daemon-process` | ❌ GUI frameworks             |
| `uc-webserver`     | Daemon HTTP + WebSocket API (axum)      | `uc-application` + `uc-core` + contract | ❌ GUI frameworks               |
| `uc-daemon`        | Daemon runtime + `uniclipd` binary      | `uc-bootstrap` + webserver + all      | ❌ GUI frameworks (Tauri/AppKit)   |
| `uc-daemon-client` | HTTP/WS client to daemon                | contract + process                    | ❌ iroh, diesel, sqlite            |
| `uc-desktop`       | Desktop host logic (GUI-framework-agnostic) | daemon-client + contract + process | ❌ Tauri, AppKit, egui             |
| `uc-tauri`         | Tauri shell adapter (commands, tray)    | `uc-desktop` + daemon-client + contract | ❌ `uc-application` directly     |
| `uc-cli`           | CLI `uniclip` binary                    | daemon-client + contract + process    | ❌ iroh, diesel (release builds)   |

## uc-core (Domain Layer)

### Purpose

Define the **business model** and **interfaces (Ports)** that the application needs. Pure domain logic with zero external dependencies.

### Responsibilities

✅ **Domain Entities** - Core business concepts

```rust
pub struct ClipboardContent {
    pub id: ClipboardId,
    pub content_type: ContentType,
    pub representations: Vec<Representation>,
}
```

✅ **Port Definitions** - Traits defining what the application needs

```rust
pub trait ClipboardRepositoryPort: Send + Sync {
    fn save(&self, content: ClipboardContent) -> Result<(), RepoError>;
    fn find_by_id(&self, id: ClipboardId) -> Result<Option<ClipboardContent>, RepoError>;
}
```

✅ **Domain Logic** - Business rules that don't depend on external systems

```rust
impl ClipboardContent {
    pub fn is_duplicate(&self, other: &ClipboardContent) -> bool {
        self.content_hash == other.content_hash
    }
}
```

✅ **Pure DTOs** - Data transfer objects for configuration

```rust
pub struct AppConfig {
    pub device_name: String,    // May be empty (fact, not error)
    pub vault_path: PathBuf,    // May be empty (fact, not error)
}
```

### Prohibited

❌ **External dependencies** - No database types, OS APIs, or framework code

```rust
// ❌ WRONG: External dependency in domain
use diesel::prelude::*;
use tauri::Manager;

pub struct ClipboardContent {
    pub sqlite_id: i32,  // Database type in domain!
}
```

❌ **Validation logic** - Ports don't validate, they define interfaces

```rust
// ❌ WRONG: Port contains validation
pub trait ClipboardRepositoryPort {
    fn save(&self, content: ClipboardContent) -> Result<(), RepoError> {
        if content.is_empty() {
            return Err(RepoError::Invalid("Empty content".into()));
        }
        // Validation belongs in use case or domain entity
    }
}
```

❌ **Default values** - DTOs don't provide defaults

```rust
// ❌ WRONG: DTO provides default value
impl AppConfig {
    pub fn default_vault_path() -> PathBuf {
        dirs::home_dir().join(".uniclipboard")  // External dependency!
    }
}
```

### Allowed Dependencies

- **Rust stdlib only** - No external crates (except maybe serde for serialization)
- **No other uc-\* crates** - Core is the foundation, nothing depends on it

### Code Review Checklist

When reviewing `uc-core` code:

- ☐ Does this code depend on any external crate? (should be no)
- ☐ Does this code contain database/OS/framework types? (should be no)
- ☐ Does this code make policy decisions? (should be no)
- ☐ Are all trait definitions pure interfaces (no implementation)?
- ☐ Are DTOs pure data structures (no validation, no defaults)?

## uc-application (Application Layer)

### Purpose

Orchestrate business logic using **only Port interfaces**. Contains use cases, facades, and application state management. External crates access this layer only through `src/facade/`; internal modules are `pub(crate)`.

### Responsibilities

✅ **Use Cases** - Application business workflows

```rust
pub struct SyncClipboardUseCase {
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

✅ **Application State** - Runtime state management

```rust
pub struct AppState {
    current_device: DeviceId,
    is_encrypted: bool,
}
```

✅ **Event Handling** - Application-level event orchestration

```rust
impl ClipboardEventHandler {
    pub fn on_new_content(&self, content: ClipboardContent) {
        if let Err(e) = self.sync_use_case.execute(content) {
            error!("Sync failed: {}", e);
        }
    }
}
```

✅ **Business Validation** - Validate before calling ports

```rust
impl SyncClipboardUseCase {
    pub fn execute(&self, content: ClipboardContent) -> Result<(), UseCaseError> {
        if content.is_empty() {
            return Err(UseCaseError::InvalidContent);
        }
        // Validation here, not in port
        self.clipboard_repo.save(content)?;
    }
}
```

### Prohibited

❌ **Concrete implementation dependencies**

```rust
// ❌ WRONG: Use case depends on concrete implementation
use uc_infra::db::SqliteClipboardRepository;

pub struct MyUseCase {
    repo: SqliteClipboardRepository,  // Concrete!
}
```

❌ **Direct infrastructure access**

```rust
// ❌ WRONG: Use case accesses database directly
use diesel::prelude::*;

pub fn execute(&self) -> Result<(), UseCaseError> {
    let conn = establish_connection()?;  // Direct DB access!
}
```

❌ **Framework dependencies**

```rust
// ❌ WRONG: Use case depends on Tauri
use tauri::AppHandle;

pub struct MyUseCase {
    handle: AppHandle,  // Framework dependency!
}
```

### Allowed Dependencies

- ✅ `uc-core` - All domain models and port interfaces
- ✅ Rust stdlib and common libraries (chrono, uuid, etc.)
- ❌ **NOT** `uc-infra` or `uc-platform`

### Code Review Checklist

When reviewing `uc-app` code:

- ☐ Does this depend on `uc-infra` or `uc-platform`? (should be no)
- ☐ Does this depend on concrete implementations? (should be no)
- ☐ Does this use `Arc<dyn PortTrait>` for dependencies? (should be yes)
- ☐ Does business logic belong here, not in infrastructure?
- ☐ Are errors converted to domain error types?

## uc-infra (Infrastructure Layer)

### Purpose

Implement **Port interfaces** to connect to infrastructure services (database, file system, encryption).

### Responsibilities

✅ **Repository Implementations** - Implement repository ports

```rust
pub struct SqliteClipboardRepository {
    pool: SqlitePool,
}

impl ClipboardRepositoryPort for SqliteClipboardRepository {
    fn save(&self, content: ClipboardContent) -> Result<(), RepoError> {
        let entity = ClipboardEntity::from_domain(content);
        // Execute SQL query
        Ok(())
    }
}
```

✅ **Entity Mapping** - Convert between domain and database models

```rust
impl ClipboardEntity {
    pub fn from_domain(content: ClipboardContent) -> Self {
        Self {
            id: content.id.to_string(),
            content_type: content.content_type.to_string(),
            // Map domain → database
        }
    }

    pub fn to_domain(self) -> ClipboardContent {
        ClipboardContent {
            id: ClipboardId::new(self.id),
            content_type: ContentType::from_str(&self.content_type),
            // Map database → domain
        }
    }
}
```

✅ **Infrastructure Services** - Implement infrastructure ports

```rust
impl EncryptionPort for EncryptionRepository {
    fn encrypt(&self, data: &[u8], key: &Key) -> Result<Vec<u8>, CryptoError> {
        // XChaCha20-Poly1305 encryption
    }
}
```

### Prohibited

❌ **Business logic**

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

❌ **Policy decisions**

```rust
// ❌ WRONG: Adapter makes policy decisions
impl DeviceRepositoryPort for SqliteDeviceRepository {
    fn save(&self, device: Device) -> Result<(), RepoError> {
        if device.name.len() > 50 {
            return Err(RepoError::Invalid("Name too long".into()));
        }
        // Policy belongs in use case
    }
}
```

❌ **Dependencies on uc-app**

```rust
// ❌ WRONG: Infrastructure depends on application
use uc_app::use_cases::SyncUseCase;
```

### Allowed Dependencies

- ✅ `uc-core` - Port interfaces and domain models
- ✅ Infrastructure crates (diesel, sqlx, chacha20poly1305, etc.)
- ❌ **NOT** `uc-app` or `uc-platform`

### Code Review Checklist

When reviewing `uc-infra` code:

- ☐ Does this depend on `uc-app`? (should be no)
- ☐ Does this contain business logic? (should be no)
- ☐ Does this implement a Port trait? (should be yes)
- ☐ Does this convert between domain and external models? (should be yes)
- ☐ Are errors mapped to domain error types?

## uc-platform (Platform Layer)

### Purpose

Implement **Port interfaces** for platform-specific functionality (clipboard, network, OS APIs).

### Responsibilities

✅ **Platform Adapters** - OS-specific implementations

```rust
pub struct MacOSClipboard {
    pasteboard: MacOSPasteboard,
}

impl ClipboardPort for MacOSClipboard {
    fn get_content(&self) -> Result<ClipboardContent, ClipboardError> {
        // Read from macOS pasteboard
    }
}
```

✅ **Network Adapters** - Network layer implementations (note: iroh P2P lives in `uc-infra`, not `uc-platform`)

```rust
// uc-platform handles OS-level capabilities like secure storage, autostart, clipboard access
pub struct SystemSecureStorage { /* OS keychain/keyring */ }

impl SecureStoragePort for SystemSecureStorage {
    fn store(&self, key: &str, value: &[u8]) -> Result<(), SecureStorageError> {
        // Store in macOS Keychain / Windows Credential Manager / Linux Secret Service
    }
}
```

✅ **Runtime Management** - Application lifecycle

```rust
pub struct AppRuntime {
    // Runtime state and lifecycle management
}
```

### Prohibited

❌ **Business logic** - Same rules as uc-infra
❌ **Dependencies on uc-app** - Platform doesn't know about application
❌ **Cross-platform abstractions** - Each adapter is platform-specific

### Allowed Dependencies

- ✅ `uc-core` - Port interfaces
- ✅ Platform-specific crates (cocoa, winapi, libp2p, etc.)
- ❌ **NOT** `uc-app` or `uc-infra`

### Code Review Checklist

When reviewing `uc-platform` code:

- ☐ Does this depend on `uc-app`? (should be no)
- ☐ Does this contain business logic? (should be no)
- ☐ Is this platform-specific? (should be yes)
- ☐ Does this implement a Port trait? (should be yes)

## uc-tauri (GUI Shell Layer)

### Purpose

Tauri 框架适配壳——Tauri command 注册、tray 图标、Quick Panel 窗口管理。自 ADR-008 后，大部分命令委托给 daemon HTTP API，只有系统级操作（窗口、tray、autostart）保留为直接 Tauri command。

Bootstrap/DI wiring 已迁移到 `uc-bootstrap` crate。

### Responsibilities

✅ **Dependency Injection** - Wire all implementations

```rust
pub fn wire_dependencies(config: &AppConfig) -> Result<AppDeps, WiringError> {
    let clipboard_repo = Arc::new(SqliteClipboardRepository::new(db_pool));
    let clipboard = Arc::new(MacOSClipboard::new()?);
    // ... wire all dependencies

    Ok(AppDeps {
        clipboard_repo,
        clipboard,
        // ...
    })
}
```

✅ **Configuration Loading** - Read TOML into DTO

```rust
pub fn load_config() -> Result<AppConfig, ConfigError> {
    let content = std::fs::read_to_string(path)?;
    AppConfig::from_toml(&content)
}
```

✅ **Tauri Commands** - Expose functionality to UI

```rust
#[tauri::command]
pub async fn get_clipboard_items(
    state: tauri::State<'_, AppRuntime>,
) -> Result<Vec<ClipboardItem>, String> {
    state.app.use_cases.clipboard_list.execute()
}
```

### Prohibited

❌ **Business logic**

```rust
// ❌ WRONG: Bootstrap contains business logic
pub fn wire_dependencies(config: &AppConfig) -> Result<AppDeps, WiringError> {
    if !encryption.is_initialized()? {
        encryption.initialize()?;  // Business logic!
    }
}
```

❌ **Policy decisions**

```rust
// ❌ WRONG: Bootstrap makes policy decisions
pub fn load_config() -> Result<AppConfig, ConfigError> {
    if config.vault_path.is_empty() {
        return Err(ConfigError::MissingVault);  // Policy!
    }
}
```

### Allowed Dependencies

- ✅ **ALL crates** - Bootstrap is the only place that can depend on everything
- ✅ Tauri framework

### Code Review Checklist

When reviewing `uc-tauri` code:

- ☐ Is business logic in `bootstrap/`? (should be no)
- ☐ Does `config.rs` only return facts? (should be yes)
- ☐ Does `wiring.rs` only create implementations? (should be yes)
- ☐ Are Tauri commands thin wrappers around use cases? (should be yes)

## Dependency Diagram

```
┌─────────────────────────────────────────────────────────────┐
│                 uc-bootstrap (Composition Root)              │
│  唯一允许依赖: uc-core + uc-application + uc-infra + uc-platform │
└─────────────────────────────────────────────────────────────┘
                            ↓ wires
┌─────────────────────────────────────────────────────────────┐
│                   uc-application                             │
│  May depend on: uc-core (Ports) + uc-observability           │
│  Must NOT: uc-infra, uc-platform                             │
└─────────────────────────────────────────────────────────────┘
                            ↓
┌─────────────────────────────────────────────────────────────┐
│                      uc-core                                │
│  May depend on: Nothing external                             │
└─────────────────────────────────────────────────────────────┘
                            ↑ implements
        ┌───────────────────┴───────────────────┐
        │                                       │
┌──────────────────┐                  ┌──────────────────┐
│   uc-infra       │                  │  uc-platform     │
│  May depend on:  │                  │  May depend on:  │
│  - uc-core       │                  │  - uc-core       │
│  Must NOT:       │                  │  - uc-app-paths  │
│  - uc-application│                  │  Must NOT:       │
└──────────────────┘                  │  - uc-application│
                                      └──────────────────┘

                ↑ consumed by
┌───────────────┼──────────────────┬──────────────────────┐
│               │                  │                      │
│  uc-daemon    │    uc-tauri      │      uc-cli          │
│  (uniclipd)  │  (GUI shell)     │  (uniclip CLI)       │
└───────────────┘──────────────────┘──────────────────────┘
```

## Common Boundary Violations

### ❌ Violation 1: Use Case Depends on Implementation

```rust
// ❌ WRONG
use uc_infra::db::SqliteClipboardRepository;

pub struct MyUseCase {
    repo: SqliteClipboardRepository,  // Concrete!
}
```

**Impact**: Makes testing hard, couples business logic to SQLite.

**Fix**: Depend on Port trait.

### ❌ Violation 2: Adapter Contains Business Logic

```rust
// ❌ WRONG
impl ClipboardRepositoryPort for SqliteClipboardRepository {
    fn save(&self, content: ClipboardContent) -> Result<(), RepoError> {
        if content.is_empty() {
            return Err(RepoError::Invalid("Empty content".into()));
        }
        // Business rule in adapter!
    }
}
```

**Impact**: Business logic hidden in infrastructure, hard to test.

**Fix**: Move validation to use case.

### ❌ Violation 3: Core Contains External Types

```rust
// ❌ WRONG
use diesel::prelude::*;

pub struct ClipboardContent {
    pub sqlite_id: i32,  // Database type in domain!
}
```

**Impact**: Domain model coupled to database schema.

**Fix**: Use domain types in core, map in adapter.

### ❌ Violation 4: Bootstrap Makes Policy Decisions

```rust
// ❌ WRONG
pub fn load_config() -> Result<AppConfig, ConfigError> {
    if config.vault_path.is_empty() {
        return Err(ConfigError::MissingVault);  // Policy!
    }
}
```

**Impact**: Policy scattered across layers, hard to change.

**Fix**: Return facts, let use case decide policy.

## Quick Reference for Common Tasks

### Adding a New Use Case

1. Create in `uc-application/src/` (通过 facade 暴露)
2. Depend only on `uc-core` Ports
3. Add to `AppDeps`
4. Wire in `uc-bootstrap/src/assembly.rs`
5. Expose via daemon HTTP route (`uc-webserver`) 或 Tauri command（仅系统级操作）

### Adding a New Repository

1. Define Port in `uc-core/src/ports/`
2. Implement in `uc-infra/src/`
3. Add to `AppDeps`
4. Wire in `uc-bootstrap/src/assembly.rs`

### Adding a New Platform Adapter

1. Define Port in `uc-core/src/ports/`
2. Implement in `uc-platform/src/`
3. Add to `AppDeps`
4. Wire in `uc-bootstrap/src/assembly.rs`

## Further Reading

- [Architecture Principles](principles.md) - Hexagonal architecture fundamentals
- [Bootstrap System](bootstrap.md) - How dependency injection works
- [Error Handling](../guides/error-handling.md) - Error handling strategy