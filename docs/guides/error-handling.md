# Error Handling Strategy

This document defines the error handling strategy for UniClipboard, following the principle of **explicit error handling at appropriate boundaries**.

## Core Principles

> **Never use `unwrap()` or `expect()` in production code.**
>
> Always handle errors explicitly and provide meaningful context.

> **Errors should be handled at the appropriate layer.**
>
> Infrastructure errors → Convert to domain errors → Convert to user-facing messages.

## Error Layering

Errors flow through three layers:

```
Infrastructure/Platform Layer (uc-infra, uc-platform)
    ↓ convert to
Domain Errors (uc-core)
    ↓ convert to
Application Errors (uc-app)
    ↓ convert to
User-Facing Messages (UI)
```

### Error Type Hierarchy

```rust
// Layer 1: Infrastructure errors
pub enum SqliteRepoError {
    Connection(String),
    Query(String),
    Migration(String),
}

// Layer 2: Domain errors (Port errors)
pub enum RepoError {
    NotFound,
    Conflict(String),
    Storage(String),
}

// Layer 3: Application errors (Use case errors)
pub enum UseCaseError {
    ClipboardNotFound,
    DeviceNotRegistered,
    EncryptionFailed(String),
}

// Layer 4: User-facing messages
pub enum UiError {
    Message(String),  // User-friendly message
}
```

## Error Handling by Layer

### Infrastructure Layer (uc-infra, uc-platform)

**Responsibility**: Convert external errors to domain errors.

```rust
// uc-infra/src/db/repositories/clipboard.rs
impl ClipboardRepositoryPort for SqliteClipboardRepository {
    fn save(&self, content: ClipboardContent) -> Result<(), RepoError> {
        self.pool
            .execute(&sql, &[&content.id])
            .map_err(|e| RepoError::Storage(format!("Failed to save clipboard: {}", e)))?;
        Ok(())
    }
}
```

**Rules**:

- ✅ Convert external errors to domain errors
- ✅ Add context about what operation failed
- ❌ Don't swallow errors silently
- ❌ Don't use `unwrap()` or `expect()`

### Application Layer (uc-app)

**Responsibility**: Convert domain errors to application errors.

```rust
// uc-app/src/use_cases/sync_clipboard.rs
impl SyncClipboardUseCase {
    pub fn execute(&self, content: ClipboardContent) -> Result<(), UseCaseError> {
        self.clipboard_repo
            .save(content.clone())
            .map_err(|e| UseCaseError::SaveFailed(e.to_string()))?;

        self.network
            .broadcast(content)
            .map_err(|e| UseCaseError::NetworkFailed(e.to_string()))?;

        Ok(())
    }
}
```

**Rules**:

- ✅ Convert port errors to use case errors
- ✅ Add business context
- ✅ Log errors at appropriate level
- ❌ Don't expose internal details to UI

### Transport Handlers (formerly Tauri commands)

**Responsibility**: Convert application errors to user-friendly messages. The examples below were written for the retired Tauri command layer (see [gui-go-tauri-retirement](../architecture/gui-go-tauri-retirement.md)); the pattern applies unchanged to daemon HTTP handlers and host command handlers.

```rust
// transport handler (illustrative)
pub async fn get_clipboard_items(
    state: tauri::State<'_, AppRuntime>,
) -> Result<Vec<ClipboardItem>, String> {
    state
        .app
        .use_cases
        .clipboard_list
        .execute()
        .map_err(|e| match e {
            UseCaseError::NotInitialized => "Please complete setup first".to_string(),
            UseCaseError::EncryptionFailed(msg) => format!("Encryption error: {}", msg),
            _ => "Failed to load clipboard items".to_string(),
        })
}
```

**Rules**:

- ✅ Convert errors to user-friendly messages
- ✅ Avoid exposing technical details
- ✅ Provide actionable error messages
- ❌ Don't expose stack traces or internal paths

## Common Error Patterns

### Pattern 1: Error Context Chain

When an error occurs, add context at each layer:

```rust
// Infrastructure layer
fn save_to_db(&self, content: &ClipboardContent) -> Result<(), DbError> {
    diesel::insert_into(clipboards::table)
        .values(content)
        .execute(&mut self.conn)
        .map_err(|e| DbError::Query(format!("Failed to insert clipboard: {}", e)))
}

// Repository layer
fn save(&self, content: ClipboardContent) -> Result<(), RepoError> {
    self.save_to_db(&content)
        .map_err(|e| RepoError::Storage(format!("Failed to save clipboard {}: {}", content.id, e)))
}

// Use case layer
fn execute(&self, content: ClipboardContent) -> Result<(), UseCaseError> {
    self.repo.save(content)
        .map_err(|e| UseCaseError::SaveFailed(format!("Unable to save clipboard: {}", e)))
}
```

### Pattern 2: Match on Error Type

Handle different error cases appropriately:

```rust
pub async fn sync_clipboard(
    state: tauri::State<'_, AppRuntime>,
    content: ClipboardContent,
) -> Result<(), String> {
    state
        .app
        .use_cases
        .sync_clipboard
        .execute(content)
        .map_err(|e| match e {
            UseCaseError::NetworkOffline => {
                "Network is offline. Clipboard will sync when connection is restored.".to_string()
            }
            UseCaseError::DeviceNotRegistered => {
                "Device not registered. Please pair this device first.".to_string()
            }
            UseCaseError::EncryptionFailed(msg) => {
                format!("Encryption failed. Please check your password. Error: {}", msg)
            }
            _ => "Failed to sync clipboard. Please try again.".to_string(),
        })
}
```

### Pattern 3: Recoverable vs Fatal Errors

Distinguish between errors that can be recovered from and those that cannot:

```rust
pub enum UseCaseError {
    // Recoverable: User can take action
    DeviceNotRegistered,
    NetworkOffline,
    InvalidPassword,

    // Fatal: Requires system intervention
    DatabaseCorrupted(String),
    EncryptionKeyMissing,
}

impl UseCaseError {
    pub fn is_recoverable(&self) -> bool {
        matches!(self, Self::DeviceNotRegistered | Self::NetworkOffline | Self::InvalidPassword)
    }
}
```

## Error Handling in Async Code

### Never Ignore Errors in Event Handlers

**Wrong** - Silent failure:

```rust
// ❌ WRONG: Error is swallowed
NetworkCommand::SendPairingRequest { peer_id, message } => {
    if let Ok(peer) = peer_id.parse::<PeerId>() {
        self.swarm.send_request(&peer, request);
        debug!("Sent pairing request to {}", peer_id);
    }
    // If parsing fails, execution silently continues!
}
```

**Correct** - Explicit error handling:

```rust
// ✅ CORRECT: Log error and emit event
NetworkCommand::SendPairingRequest { peer_id, message } => {
    match peer_id.parse::<PeerId>() {
        Ok(peer) => {
            self.swarm.send_request(&peer, request);
            debug!("Sent pairing request to {}", peer_id);
        }
        Err(e) => {
            warn!("Invalid peer_id '{}': {}", peer_id, e);
            let _ = self
                .event_tx
                .send(NetworkEvent::Error(format!(
                    "Failed to send pairing request: invalid peer_id '{}': {}",
                    peer_id, e
                )))
                .await;
        }
    }
}
```

### When to Use `if let` vs `match`

**Use `if let`** when the None/Err case is truly benign:

```rust
// ✅ OK: Using if let when fallback is acceptable
if let Some(value) = optional_cache.get(&key) {
    return value;
}
// Continue with fallback behavior
```

**Use `match`** when the Err case represents a failure:

```rust
// ✅ CORRECT: Using match when error should be reported
match peer_id.parse::<PeerId>() {
    Ok(peer) => send_request(peer),
    Err(e) => {
        warn!("Invalid peer_id: {}", e);
        emit_error_event(e);
    }
}
```

## Bootstrap Error Handling

Bootstrap has its own error types for infrastructure initialization:

### ConfigError

```rust
pub enum ConfigError {
    Io(std::io::Error),
    Parse(toml::de::Error),
}
```

**Handling**: Recoverable (use defaults for missing config)

### WiringError

```rust
pub enum WiringError {
    DatabaseInit(String),
    NetworkInit(String),
    ClipboardInit(String),
    EncryptionInit(String),
}
```

**Handling**: Defaults to fatal, but `main.rs` can decide based on runtime mode (CLI/GUI/debug).

## Common Mistakes

### ❌ Mistake 1: Using `unwrap()` in Production

```rust
// ❌ WRONG: Will panic in production
let content = self.clipboard.get_content().unwrap();
```

**Fix**:

```rust
// ✅ CORRECT: Handle error explicitly
let content = self.clipboard.get_content()
    .map_err(|e| UseCaseError::ClipboardAccessFailed(e.to_string()))?;
```

### ❌ Mistake 2: Silent Failures

```rust
// ❌ WRONG: Error is ignored
let _ = self.network.broadcast(content);
```

**Fix**:

```rust
// ✅ CORRECT: Handle or log the error
self.network.broadcast(content)
    .map_err(|e| {
        warn!("Failed to broadcast clipboard: {}", e);
        UseCaseError::NetworkFailed(e.to_string())
    })?;
```

### ❌ Mistake 3: Exposing Internal Details

```rust
// ❌ WRONG: Exposes internal paths and stack traces
pub fn get_items() -> Result<Vec<Item>, String> {
    repo.find_all()
        .map_err(|e| format!("Database error at {}: {:?}", std::file!(), e))
}
```

**Fix**:

```rust
// ✅ CORRECT: User-friendly message
pub fn get_items() -> Result<Vec<Item>, String> {
    repo.find_all()
        .map_err(|e| {
            error!("Failed to load items: {:?}", e);
            "Failed to load clipboard items".to_string()
        })
}
```

### ❌ Mistake 4: String Errors Everywhere

```rust
// ❌ WRONG: Using String for all errors
fn save(&self, content: ClipboardContent) -> Result<(), String> {
    // ...
}
```

**Fix**:

```rust
// ✅ CORRECT: Use proper error types
fn save(&self, content: ClipboardContent) -> Result<(), RepoError> {
    // ...
}
```

## Error Logging Strategy

### Log Levels

| Level    | When to Use                                   | Example                         |
| -------- | --------------------------------------------- | ------------------------------- |
| `error!` | Fatal errors that prevent operation           | "Failed to initialize database" |
| `warn!`  | Recoverable errors that might indicate issues | "Retrying network connection"   |
| `info!`  | Important state changes                       | "Device paired successfully"    |
| `debug!` | Detailed flow information                     | "Sent clipboard to 3 peers"     |
| `trace!` | Very detailed execution tracing               | "Executing SQL query"           |

### Error Context

Always add context when logging errors:

```rust
// ❌ WRONG: No context
error!("Save failed");

// ✅ CORRECT: Added context
error!("Failed to save clipboard {}: {}", content.id, e);
```

## Testing Error Handling

### Test Error Paths

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_save_failed_on_duplicate() {
        let repo = InMemoryClipboardRepo::new();
        let content = ClipboardContent::test_fixture();

        // First save succeeds
        assert!(repo.save(content.clone()).is_ok());

        // Duplicate save fails
        let result = repo.save(content);
        assert!(matches!(result, Err(RepoError::Conflict(_))));
    }

    #[test]
    fn test_use_case_converts_repo_errors() {
        let repo = FailingRepo::always_fail();
        let use_case = SyncClipboardUseCase::new(repo);

        let result = use_case.execute(test_content());
        assert!(matches!(result, Err(UseCaseError::SaveFailed(_))));
    }
}
```

### Mock Errors for Testing

```rust
struct FailingRepo {
    error_type: RepoError,
}

impl ClipboardRepositoryPort for FailingRepo {
    fn save(&self, _content: ClipboardContent) -> Result<(), RepoError> {
        Err(self.error_type.clone())
    }
}
```

## Quick Reference

### When to Use Which Error Type

| Situation             | Error Type                       | Example                       |
| --------------------- | -------------------------------- | ----------------------------- |
| Database query failed | `RepoError::Storage`             | "Failed to query database"    |
| Entity not found      | `RepoError::NotFound`            | No message needed             |
| Validation failed     | `UseCaseError::Validation`       | "Invalid device name"         |
| Network offline       | `UseCaseError::NetworkOffline`   | "Network is offline"          |
| Encryption failed     | `UseCaseError::EncryptionFailed` | "Encryption failed"           |
| User-facing           | `String` (user-friendly)         | "Please complete setup first" |

### Error Conversion Pattern

```rust
// Infrastructure → Domain
.map_err(|e| RepoError::Storage(e.to_string()))?

// Domain → Application
.map_err(|e| UseCaseError::SaveFailed(e.to_string()))?

// Application → User
.map_err(|e| user_friendly_message(e))?
```

## Further Reading

- [Architecture Principles](../architecture/principles.md) - Hexagonal architecture fundamentals
- [Module Boundaries](../architecture/module-boundaries.md) - Module responsibilities
- [Bootstrap System](../architecture/bootstrap.md) - Bootstrap error handling
