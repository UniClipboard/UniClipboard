# Phase 29: Add macOS auto-unlock keychain Always Allow confirmation modal on UnlockPage - Context

**Gathered:** 2026-03-13
**Status:** Ready for planning

<domain>
## Phase Boundary

When macOS users toggle the auto-unlock switch ON in UnlockPage, display a modal that explains they need to select "Always Allow" in the macOS Keychain popup, then verify the permission was actually granted before enabling auto-unlock. Non-macOS platforms skip the modal entirely.

</domain>

<decisions>
## Implementation Decisions

### Modal trigger & Switch state

- Switch ON triggers Modal immediately; Switch stays OFF until verification passes
- Modal has "Cancel" button (closes Modal, Switch stays OFF) and "已勾选始终允许" confirmation button
- User can cancel at any time without side effects

### Modal content

- Pure text step-by-step instructions (no screenshots/illustrations)
- Steps: 1) Keychain popup will appear next 2) Select "Always Allow" 3) Enter password to confirm
- After instructions, a prominent "已勾选始终允许" button to trigger verification

### Verification mechanism

- New dedicated Tauri command (not reusing unlock_encryption_session)
- Logic: call `key_material.load_kek()` — if it succeeds without user prompt, Always Allow is granted
- Returns success/failure to frontend

### Failure handling

- On verification failure: show red error message inside Modal ("未检测到始终允许授权")
- Modal stays open — user can retry after re-granting permission
- No retry limit — user can retry indefinitely or cancel

### Cross-platform behavior

- macOS: Switch ON → Modal → verification → enable
- Windows/Linux: Switch ON → directly enable auto-unlock, no Modal
- The existing macOS Keychain hint text (`unlock.macOSNote`) at bottom of UnlockPage remains macOS-only (already gated by `isMac`)

### Claude's Discretion

- Exact error message wording and styling
- Loading state during verification
- New Tauri command naming and error types

</decisions>

<specifics>
## Specific Ideas

- The Modal should clearly communicate that the Keychain popup is a macOS system dialog, not part of UniClipboard
- The "已勾选始终允许" button should feel like a confirmation action, not a primary action
- Verification should feel instant — no unnecessary loading if the keychain read is fast

</specifics>

<code_context>

## Existing Code Insights

### Reusable Assets

- `Dialog` component (`src/components/ui/dialog.tsx`): Standard modal component to build the confirmation modal
- `AlertDialog` component (`src/components/ui/alert-dialog.tsx`): Alternative with built-in Cancel/Confirm pattern
- `usePlatform` hook with `isMac`: Already used in UnlockPage for platform detection
- `useSetting` hook with `updateSecuritySetting`: Already handles `auto_unlock_enabled` toggle

### Established Patterns

- `handleAutoUnlockChange` in UnlockPage: Current handler that directly updates setting — needs to be intercepted on macOS
- Tauri command pattern: `runtime.usecases().xxx()` accessor pattern for new commands
- `SystemSecureStorage` uses `keyring` crate with service name "UniClipboard" for Keychain access
- `KeyMaterialPort.load_kek()`: Existing port for reading KEK from keychain — verification target

### Integration Points

- `UnlockPage.tsx:32-34` (`handleAutoUnlockChange`): Intercept here to show Modal on macOS
- `src-tauri/crates/uc-tauri/src/commands/encryption.rs`: Add new verification command alongside existing encryption commands
- `src/api/security.ts`: Add frontend API call for new verification command
- `src-tauri/crates/uc-app/src/usecases/`: New use case for keychain verification

</code_context>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

_Phase: 29-add-macos-auto-unlock-keychain-always-allow-confirmation-modal-on-unlockpage_
_Context gathered: 2026-03-13_
