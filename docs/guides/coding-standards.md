# Coding Standards

This document defines the coding standards for UniClipboard. It is extracted from [CLAUDE.md](../../CLAUDE.md) and serves as a quick reference for implementation and code review.

## Language-Specific Rules

### Rust Error Handling

**CRITICAL**: Never use `unwrap()` or `expect()` in production code. Always handle errors explicitly:

```rust
// ❌ FORBIDDEN
let value = some_option.unwrap();
let result = some_result.expect("failed");

// ✅ CORRECT - Use pattern matching
match some_option {
    Some(value) => { /* handle value */ },
    None => { /* handle error case */ },
}

// ✅ CORRECT - Use ? operator with proper error propagation
pub fn do_something() -> Result<(), MyError> {
    let value = some_option.ok_or(MyError::NotFound)?;
    // ...
}

// ✅ CORRECT - Use unwrap_or/unwrap_or_default for non-critical defaults
let value = some_option.unwrap_or_default();
let config = config_option.unwrap_or_else(|| Config::default());

// ✅ ACCEPTABLE in tests only
#[cfg(test)]
mod tests {
    #[test]
    fn test_something() {
        let value = some_option.unwrap(); // OK in tests
    }
}
```

**Rationale**: Explicit error handling prevents panics in production, provides better error messages, and makes failure modes visible to callers.

### Avoid Silent Failures in Event-Driven Code

**CRITICAL**: When handling events or commands in async/event-driven systems, never silently ignore errors. Always log errors and emit failure events when appropriate.

**Anti-Pattern**: Silent failures with `if let Ok(...)`:

```rust
// ❌ WRONG - Silent failure, caller never knows the operation failed
NetworkCommand::SendPairingRequest { peer_id, message } => {
    if let Ok(peer) = peer_id.parse::<PeerId>() {
        self.swarm.send_request(&peer, request);
        debug!("Sent pairing request to {}", peer_id);
    }
    // If parsing fails, execution silently continues - user has no feedback!
}
```

**Correct Pattern**: Explicit error handling with logging and event emission:

```rust
// ✅ CORRECT - Log error and emit event for frontend to handle
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

**Key Rules**:

1. **Use `match` instead of `if let`** - When the `Err` case represents a failure that users should know about
2. **Always log errors** - Use `warn!()` or `error!()` to ensure failures are visible in logs
3. **Emit error events** - Send `NetworkEvent::Error` or equivalent so the UI can display user-friendly error messages
4. **Handle missing resources** - When an expected resource (like a pending channel) is missing, log a warning

**When to use `if let` vs `match`**:

```rust
// ✅ OK - Using if let when the None/Err case is truly benign
if let Some(value) = optional_cache.get(&key) {
    // Use cached value
}

// ✅ OK - Using if let when fallback behavior is acceptable
if let Ok(config) = read_config() {
    apply_config(config);
} else {
    use_default_config(); // Explicit fallback
}

// ❌ WRONG - Using if let when failure should be reported
if let Ok(peer_id) = str.parse::<PeerId>() {
    send_request(peer_id);
}
// Error is swallowed!
```

## Problem-Solving Philosophy

**CRITICAL**: Don't treat symptoms in isolation. Always step back and analyze problems from a higher-level perspective before implementing fixes.

### Symptoms vs. Root Causes

```
❌ ANTI-PATTERN - Symptom-focused
"Component renders wrong" → Add useEffect hack → "State desync" → Add more hacks → Spaghetti code

✅ CORRECT - Root cause analysis
"Component renders wrong" → Trace data flow → Identify architectural gap → Design proper solution → Fix at the right layer
```

### High-Level Thinking Checklist

Before making changes, ask:

1. **Where does this problem originate?**
   - UI layer issue, or state management problem?
   - API contract mismatch, or business logic gap?
   - Infrastructure limitation, or architectural flaw?

2. **What's the systemic fix?**
   - Can this be solved by improving the abstraction?
   - Would a design pattern eliminate this class of bugs?
   - Is there a missing piece in the architecture?

3. **What are the trade-offs?**
   - Short-term hack vs. long-term maintainability
   - Local fix vs. systemic improvement
   - Quick workaround vs. proper solution

### Examples

```rust
// ❌ WRONG - Treating symptoms everywhere
async fn sync_clipboard() {
    match send_to_device().await {
        Err(_) => sleep(Duration::from_secs(1)).await, // Band-aid
        Ok(_) => {}
    }
}

// ✅ CORRECT - Fix the retry logic at the infrastructure layer
// infrastructure/sync/retry_policy.rs
pub struct RetryPolicy {
    max_attempts: u32,
    backoff_strategy: BackoffStrategy,
}

async fn sync_clipboard_with_retry(policy: &RetryPolicy) -> Result<()> {
    policy.execute(|| send_to_device()).await
}
```

```tsx
// ❌ WRONG - Local state patch
function DeviceList() {
  const [devices, setDevices] = useState([])
  useEffect(() => {
    fetchDevices().then(setDevices)
    setInterval(() => fetchDevices().then(setDevices), 5000) // Manual polling
  }, [])
}

// ✅ CORRECT - Leverage existing state management (Redux RTK Query)
function DeviceList() {
  const { data: devices } = useGetDevicesQuery() // Built-in caching, refetch, error handling
}
```

**Rationale**: High-level problem-solving prevents technical debt, reduces code complexity, and creates more maintainable solutions. Always identify the root cause and fix it at the appropriate abstraction layer.

## Frontend (React + TypeScript)

### Tailwind CSS: Avoid Fixed Pixels

**CRITICAL**: Avoid fixed pixel values (`w-[XXpx]`, `h-[XXpx]`) for cross-platform compatibility. Use Tailwind's built-in utilities or relative units (rem) instead:

```tsx
// ❌ FORBIDDEN - Fixed pixels don't scale across platforms/DPI
<div className="w-[200px] h-[60px]" />
<div className="min-w-[80px]" />
<div className="h-[1px]" />

// ✅ CORRECT - Use Tailwind utilities (rem-based)
<div className="w-52 h-15" />           // w-52 = 13rem, h-15 = 3.75rem
<div className="min-w-20" />            // min-w-20 = 5rem
<div className="h-px" />                // 1px height (special case)

// ✅ CORRECT - Use rem values directly when needed
<div className="w-[3.75rem]" />         // 60px = 3.75rem
<div className="h-[0.0625rem]" />       // 1px = 0.0625rem

// ✅ ACCEPTABLE - For truly fixed sizes (borders, shadows, etc.)
<div className="border shadow-lg" />
```

**Rationale**: Rem-based units scale with the root font size, providing better cross-platform consistency across different screen densities, DPI settings, and user accessibility preferences. Tailwind's default configuration uses `1rem = 16px`.

**Common Tailwind Width Reference**:

- `w-16` = 4rem (64px)
- `w-20` = 5rem (80px)
- `w-52` = 13rem (208px)
- `h-px` = 1px (special utility)

### Theme Support Best Practices

**ALWAYS test components in both light and dark themes** to ensure proper contrast and visibility.

**Container Components** (Dialog, Card, Popover, etc.):

- Use `bg-card` + `text-card-foreground` for containers with content
- Use `bg-background` only for page/base backgrounds
- Use `bg-muted` for disabled/readonly states with `text-foreground` (not `text-muted-foreground`)

**Common Pitfalls**:

```tsx
// ❌ WRONG - Background color on containers makes them blend in
<DialogContent className="bg-background" />

// ✅ CORRECT - Card color creates proper visual hierarchy
<DialogContent className="bg-card text-card-foreground" />

// ❌ WRONG - Muted text on readonly inputs is hard to read
<input className="bg-muted text-muted-foreground" readOnly />

// ✅ CORRECT - Muted background with foreground text
<input className="bg-muted/50 text-foreground" readOnly />
```

**Status Messages**:

- Add `border border-{color}/20` to banners for better visibility in light mode
- Use `font-medium` on text for better readability
- Ensure hover states use `/70` opacity (not `/60`) for visibility

## Backend (Rust)

### Transport Layer Architecture

**CRITICAL**: Transport entry points (daemon HTTP/WS handlers and the host command handlers in `apps/gui-go`) are thin adapters. All business logic MUST be in the Use Cases layer; the adapters only parse input, call a use case or facade, and serialize the result. Never reach into Ports directly from the transport layer.

The former Tauri command layer (state registration with `.manage()`, `runtime.usecases()` accessor, `map_err`) was retired together with the Tauri host; see [gui-go-tauri-retirement](../architecture/gui-go-tauri-retirement.md).


## General Development Principles

### Avoid Over-Engineering

- **Don't add features, refactor code, or make "improvements" beyond what was asked**
- A bug fix doesn't need surrounding code cleaned up
- A simple feature doesn't need extra configurability
- Don't add docstrings, comments, or type annotations to code you didn't change
- Only add comments where the logic isn't self-evident

### Don't Create Helpers for One-Time Operations

- Don't create helpers, utilities, or abstractions for one-off operations
- Don't design for hypothetical future requirements
- The right amount of complexity is the minimum needed for the current task—three similar lines of code is better than a premature abstraction

## Quick Reference Checklist

### Before Committing Code

- ☐ No `unwrap()` or `expect()` in production code
- ☐ All errors handled explicitly (no silent failures)
- ☐ Event handlers use `match` instead of `if let` for error cases
- ☐ Fixed pixel values replaced with Tailwind utilities
- ☐ Components tested in both light and dark themes
- ☐ Transport handlers call use cases (no direct Port access)
- ☐ Problem analyzed at root cause level, not symptom level
- ☐ No over-engineering or premature abstractions

### Code Review Checklist

- ☐ Does this code handle all error cases?
- ☐ Are event failures logged and emitted?
- ☐ Are cross-platform concerns addressed?
- ☐ Do transport handlers follow hexagonal architecture (call use cases, no direct Port access)?
- ☐ Does this fix the root cause, not symptoms?
- ☐ Is this the minimum solution, or over-engineered?

## Further Reading

- [Architecture Principles](../architecture/principles.md) - Hexagonal architecture fundamentals
- [Module Boundaries](../architecture/module-boundaries.md) - Module responsibilities
- [Error Handling](../guides/error-handling.md) - Error handling strategy
- [CLAUDE.md](../../CLAUDE.md) - Full project instructions
