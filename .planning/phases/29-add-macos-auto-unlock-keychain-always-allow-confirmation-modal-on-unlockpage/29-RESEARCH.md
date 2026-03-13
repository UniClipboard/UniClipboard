# Phase 29: Add macOS auto-unlock keychain Always Allow confirmation modal on UnlockPage - Research

**Researched:** 2026-03-13
**Domain:** macOS Keychain permission verification + React modal UI
**Confidence:** HIGH

## Summary

This phase adds a confirmation modal to the UnlockPage that appears when macOS users toggle the auto-unlock switch ON. The modal guides users through granting "Always Allow" permission in the macOS Keychain popup, then verifies the permission was actually granted before persisting the setting. Non-macOS platforms bypass the modal entirely.

The implementation spans two layers: (1) a new Tauri command + use case that calls `key_material.load_kek()` to verify keychain access succeeds silently, and (2) a frontend AlertDialog modal with step-by-step instructions, error display, and loading state. All building blocks exist in the codebase already -- the `keyring` crate, `KeyMaterialPort`, `AlertDialog` component, `usePlatform` hook, and `useSetting` hook with `updateSecuritySetting`.

**Primary recommendation:** Create a dedicated `verify_keychain_access` Tauri command backed by a lightweight use case that calls `load_kek()`. On the frontend, use the existing `AlertDialog` component to build the confirmation modal, intercepting `handleAutoUnlockChange` on macOS only.

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

- Switch ON triggers Modal immediately; Switch stays OFF until verification passes
- Modal has "Cancel" button (closes Modal, Switch stays OFF) and confirmation button
- Pure text step-by-step instructions (no screenshots/illustrations)
- Steps: 1) Keychain popup will appear next 2) Select "Always Allow" 3) Enter password to confirm
- New dedicated Tauri command (not reusing unlock_encryption_session)
- Verification logic: call `key_material.load_kek()` -- if it succeeds without user prompt, Always Allow is granted
- On verification failure: show red error message inside Modal
- Modal stays open on failure -- user can retry indefinitely or cancel
- macOS: Switch ON -> Modal -> verification -> enable
- Windows/Linux: Switch ON -> directly enable auto-unlock, no Modal
- The existing macOS Keychain hint text remains macOS-only

### Claude's Discretion

- Exact error message wording and styling
- Loading state during verification
- New Tauri command naming and error types

### Deferred Ideas (OUT OF SCOPE)

None

</user_constraints>

## Standard Stack

### Core (Already in project)

| Library                  | Purpose                                          | Why Standard                                    |
| ------------------------ | ------------------------------------------------ | ----------------------------------------------- |
| `keyring` crate          | macOS Keychain access via `SecureStoragePort`    | Already used for KEK storage                    |
| `AlertDialog` (Radix UI) | Modal with Cancel/Action pattern                 | Already in `src/components/ui/alert-dialog.tsx` |
| `usePlatform` hook       | `isMac` detection for platform-gating            | Already used in UnlockPage                      |
| `useSetting` hook        | `updateSecuritySetting({ auto_unlock_enabled })` | Already used in UnlockPage                      |
| `invokeWithTrace`        | Tauri command invocation with tracing            | Standard pattern in `src/api/security.ts`       |

### No New Dependencies Required

This phase uses only existing libraries and patterns. No new npm packages or Rust crates needed.

## Architecture Patterns

### Backend: New Tauri Command + Use Case

Follow the established command pattern:

```
src-tauri/crates/uc-app/src/usecases/verify_keychain_access.rs  (new use case)
src-tauri/crates/uc-tauri/src/commands/encryption.rs             (add command)
src-tauri/src/main.rs                                            (register in invoke_handler)
src/api/security.ts                                              (add frontend API)
```

**Verification use case structure:**

1. Get current `KeyScope` via `KeyScopePort`
2. Call `key_material.load_kek(&scope)`
3. If `Ok(_)` -> keychain access granted silently (Always Allow is set) -> return `Ok(true)`
4. If `Err(EncryptionError::KeyNotFound)` -> KEK not stored yet -> return specific error
5. If `Err(EncryptionError::PermissionDenied)` -> user denied or didn't grant Always Allow -> return `Ok(false)` or error
6. If `Err(other)` -> unexpected error -> propagate

**Key insight:** The `load_kek()` call itself triggers the macOS Keychain popup if "Always Allow" hasn't been granted. If the user has granted it, the call succeeds silently. This is the verification mechanism.

### Frontend: Modal Flow State Machine

```
[Switch OFF] --toggle ON--> [isMac?]
                              |
                     yes      |      no
                     v        |      v
              [Show Modal]    | [updateSecuritySetting(true)]
                     |
        [User clicks verify]
                     |
              [Loading state]
                     |
            [verify_keychain_access]
                /          \
          success         failure
              |              |
    [updateSecurity-   [Show error in Modal,
     Setting(true)]     user can retry or cancel]
    [close Modal]
```

### File Changes Summary

```
# Backend (Rust)
src-tauri/crates/uc-app/src/usecases/verify_keychain_access.rs   # NEW use case
src-tauri/crates/uc-app/src/usecases/mod.rs                      # Export new use case
src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs                # Add accessor
src-tauri/crates/uc-tauri/src/commands/encryption.rs              # Add command
src-tauri/src/main.rs                                             # Register command

# Frontend (TypeScript/React)
src/api/security.ts                                               # Add verifyKeychainAccess()
src/pages/UnlockPage.tsx                                          # Add modal + intercept logic
src/i18n/locales/en-US.json                                       # Add modal text keys
src/i18n/locales/zh-CN.json                                       # Add modal text keys (Chinese)
```

### Anti-Patterns to Avoid

- **Don't reuse `unlock_encryption_session`:** It does much more (unwrap master key, set session, start lifecycle). Verification only needs `load_kek()`.
- **Don't set switch to ON optimistically:** Switch must stay OFF until verification passes AND setting is persisted.
- **Don't use `Dialog` instead of `AlertDialog`:** The `AlertDialog` has built-in Cancel/Action button pattern which matches the design exactly.

## Don't Hand-Roll

| Problem                   | Don't Build                       | Use Instead                                 | Why                                                  |
| ------------------------- | --------------------------------- | ------------------------------------------- | ---------------------------------------------------- |
| Modal with Cancel/Confirm | Custom modal div                  | `AlertDialog` component                     | Already has proper overlay, animation, accessibility |
| Platform detection        | `navigator.userAgent` parsing     | `usePlatform().isMac`                       | Already exists, tested, memoized                     |
| Keychain access           | Direct `keyring` calls in command | Use case with `KeyMaterialPort`             | Follows hexagonal architecture, testable             |
| Settings persistence      | Direct storage calls              | `updateSecuritySetting()` from `useSetting` | Already handles merge + persist + state update       |

## Common Pitfalls

### Pitfall 1: Keychain popup timing

**What goes wrong:** The macOS Keychain popup appears when `load_kek()` is called during verification. If the user hasn't stored a KEK yet (e.g., first-time setup), there's nothing to verify.
**Why it happens:** `load_kek()` returns `KeyNotFound` if KEK was never stored, not a permission error.
**How to avoid:** The verification command should first check if encryption is initialized AND KEK exists. If not, the auto-unlock toggle shouldn't be available at all (which is already the case since UnlockPage only shows when encryption IS initialized).
**Warning signs:** Verification always fails even after granting Always Allow.

### Pitfall 2: Race condition between Modal close and setting update

**What goes wrong:** If Modal closes before `updateSecuritySetting` completes, the switch may flicker.
**Why it happens:** React state updates are async.
**How to avoid:** Await `updateSecuritySetting` before closing the modal. Use a single state update flow: verify -> update setting -> close modal.

### Pitfall 3: load_kek behavior on non-macOS

**What goes wrong:** On Linux/Windows, `load_kek()` may behave differently (no interactive prompt).
**Why it happens:** Different keyring backends have different permission models.
**How to avoid:** The modal and verification are strictly macOS-only (gated by `isMac`). Non-macOS platforms skip directly to enabling the setting.

### Pitfall 4: Error message i18n

**What goes wrong:** Error messages shown in the modal are hardcoded in one language.
**Why it happens:** Mixing hardcoded Chinese text with i18n system.
**How to avoid:** All user-facing strings must go through `t()` function with keys in both locale files. The CONTEXT.md mentions Chinese text like "已勾选始终允许" and "未检测到始终允许授权" -- these are the zh-CN translations, en-US equivalents must also be provided.

## Code Examples

### Verification Use Case (Rust)

```rust
// src-tauri/crates/uc-app/src/usecases/verify_keychain_access.rs
// Simplified -- only needs KeyScopePort + KeyMaterialPort

pub struct VerifyKeychainAccess {
    key_scope: Arc<dyn KeyScopePort>,
    key_material: Arc<dyn KeyMaterialPort>,
}

impl VerifyKeychainAccess {
    pub async fn execute(&self) -> Result<bool, VerifyKeychainError> {
        let scope = self.key_scope.current_scope().await
            .map_err(|e| VerifyKeychainError::ScopeFailed(e.to_string()))?;

        match self.key_material.load_kek(&scope).await {
            Ok(_) => Ok(true),  // Keychain access succeeded silently
            Err(EncryptionError::PermissionDenied) => Ok(false),
            Err(EncryptionError::KeyNotFound) => Err(VerifyKeychainError::KekNotFound),
            Err(e) => Err(VerifyKeychainError::Unexpected(e.to_string())),
        }
    }
}
```

### Tauri Command (Rust)

```rust
// Added to src-tauri/crates/uc-tauri/src/commands/encryption.rs
#[tauri::command]
pub async fn verify_keychain_access(
    runtime: State<'_, Arc<AppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<bool, String> {
    let uc = runtime.usecases().verify_keychain_access();
    uc.execute().await.map_err(|e| e.to_string())
}
```

### Frontend API (TypeScript)

```typescript
// Added to src/api/security.ts
export async function verifyKeychainAccess(): Promise<boolean> {
  try {
    return await invokeWithTrace('verify_keychain_access')
  } catch (error) {
    console.error('Keychain verification failed:', error)
    throw error
  }
}
```

### Modal Integration in UnlockPage (React)

```tsx
// Key state additions to UnlockPage
const [showKeychainModal, setShowKeychainModal] = useState(false)
const [verifying, setVerifying] = useState(false)
const [verifyError, setVerifyError] = useState<string | null>(null)

const handleAutoUnlockChange = async (checked: boolean) => {
  if (checked && isMac) {
    setShowKeychainModal(true) // Show modal instead of direct update
    return
  }
  await updateSecuritySetting({ auto_unlock_enabled: checked })
}
```

### i18n Keys (structure)

```json
{
  "unlock": {
    "keychainModal": {
      "title": "...",
      "description": "...",
      "step1": "...",
      "step2": "...",
      "step3": "...",
      "confirm": "...",
      "cancel": "...",
      "verifying": "...",
      "error": "..."
    }
  }
}
```

## State of the Art

| Aspect                  | Current State                                      | Impact                                                    |
| ----------------------- | -------------------------------------------------- | --------------------------------------------------------- |
| `keyring` crate         | Uses macOS Keychain backend via Security framework | `load_kek()` triggers system prompt if not "Always Allow" |
| `AlertDialog` component | Supports `size="default"` and `size="sm"` variants | Use default size for multi-step instructions              |
| Tauri command pattern   | `runtime.usecases().xxx()` accessor                | New command follows same pattern                          |
| i18n                    | `react-i18next` with `en-US.json` and `zh-CN.json` | Add keys to both locale files                             |

## Open Questions

1. **What error does `keyring` return when user cancels the macOS Keychain prompt?**
   - What we know: `PlatformFailure` is caught and mapped to `PermissionDenied` in `SystemSecureStorage`
   - What's unclear: Whether canceling the prompt vs. selecting "Deny" vs. timeout produces the same error variant
   - Recommendation: Treat any non-success result from `load_kek` as "not granted" and show the retry error message. The use case returns `Ok(false)` for permission denied and `Err` for unexpected errors -- both should show the error message in the modal.

2. **Should the verification command also check `EncryptionState::Initialized`?**
   - What we know: UnlockPage is only shown when encryption IS initialized, so the precondition is met
   - What's unclear: Edge case where state changes between page render and verification
   - Recommendation: Add an initialization check in the use case as a safety guard, returning a specific error if not initialized

## Sources

### Primary (HIGH confidence)

- Project codebase: `src-tauri/crates/uc-platform/src/system_secure_storage.rs` -- keyring integration
- Project codebase: `src-tauri/crates/uc-app/src/usecases/auto_unlock_encryption_session.rs` -- existing auto-unlock flow
- Project codebase: `src-tauri/crates/uc-infra/src/security/key_material.rs` -- `load_kek` implementation
- Project codebase: `src/pages/UnlockPage.tsx` -- current UI
- Project codebase: `src/components/ui/alert-dialog.tsx` -- modal component

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH - all components already exist in the project
- Architecture: HIGH - follows established patterns (use case + command + API + component)
- Pitfalls: HIGH - understood from reading the actual keyring/secure storage implementation
- Verification mechanism: MEDIUM - keychain prompt behavior on cancel/deny needs runtime testing

**Research date:** 2026-03-13
**Valid until:** 2026-04-13 (stable -- no external dependencies changing)
