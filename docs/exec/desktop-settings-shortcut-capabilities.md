# Desktop shortcut settings: capability and contract review

- Date: 2026-11-22.
- Source baseline: `fcf5611662039c1db79fc6537c4f79aff2a5b436`, with the settings work in progress kept separate from this review.
- Pinned mygo: `v0.8.4-0.20261108082913-9233663736ca`.
- v1 reference: `c6483d73c7a0234be68571fdb5fa3d38a17c4871`.
- Scope: E-group implementation preparation only. No runtime, public contract or shared file changes are made by this review.
- Parent plan: [settings parallel work agreement](desktop-settings-parity.md).
- Authority: [Desktop quick-panel contract](../contracts/desktop-quick-panel-contract.md).

## Conclusions

The existing fixed global-shortcut choices can support a better editor,
reset-to-default, effective-registration status and conflict recovery without
changing the public contract. Free-form global accelerators exceed the contract's
explicit choice list. Remapping Enter, Shift+Enter or Command/Ctrl+Enter changes
explicit public command semantics. Those are the two actual contract decisions.

The already proposed panel-enabled and panel-position fields do not themselves
require another user approval. Their implementation must preserve target capture,
paste safety, process ownership and lightweight-mode behavior. The proposed
modifier-double-tap field also has no conflicting explicit contract clause if
its tray-owned trigger invokes the existing panel-opening path. It does require
platform capability work before accepting a non-disabled value.

## Evidence map

All library findings below come from the pinned local module source. These are
source-inspection results, not OS execution results.

| Surface | Inspected source | Finding |
|---|---|---|
| Global registration | mygo `modules.go`, `GlobalShortcutModule.Register`, `IsRegistered`, `Unregister`, `UnregisterAll` | Public registration and ownership inspection exist. No public parse-only or recorder API is exposed. |
| Accelerator parsing | mygo `internal/accelerator/accelerator.go`, `Parse` | The parser is internal to mygo. Unic cannot import it. `Register` parses and registers; parsing cannot be separated from its native side effect through that public method. |
| Focused key recording | mygo `ui/handler.go`, `InputEvent`; `ui/api_gen.go`, `Element.HandleInput`, `Context.KeysDown` | A focused application-owned recorder can receive key-down/up, physical code, modifiers and repeat information. This is not background keyboard monitoring. |
| Permission flow | mygo `plugins/permissionflow/permissionflow.go` and platform files | Existing `Check`, `Request`, `OpenSettings`, `Wait` and `Guide` APIs support permission status and explicit guidance. No new dependency is required. |
| Display geometry | mygo `modules.go`, `ScreenModule`; `window.go`, `SetPosition`, `Center` | Public cursor position, display bounds/work area, nearest-display selection and native positioning are available. Platform support still bounds the result. |
| Current global owner | [`quickpanel/shortcut.go`](../../app/desktop/internal/quickpanel/shortcut.go), [`roles_native.go`](../../app/desktop/cmd/unic-desktop/roles_native.go) | Tray registers the shortcut and preserves the previous effective registration when replacement fails. |
| Current opening path | [`roles.go`](../../app/desktop/cmd/unic-desktop/roles.go), `trayRuntime.openPanel`; [`process_panel.go`](../../app/desktop/internal/desktopui/process_panel.go) | Tray captures the original target; the independent panel owns no global registration or clipboard adapter. |
| Current preferences | [`localconfig/config.go`](../../internal/localconfig/config.go), `Validate`; [`live_settings.go`](../../app/desktop/internal/desktopui/live_settings.go) | The service rejects choices outside the four named combinations or empty. The live UI offers those choices and registration status/retry. |
| Current placement | [`quick_panel_window.go`](../../app/desktop/internal/desktopui/quick_panel_window.go) | The history panel uses mygo's normal initial placement; there is no saved center/follow-cursor setting. Preview placement already uses a native flyout. |

## Accelerator syntax and conflicts

mygo's internal parser recognizes modifier aliases, platform-dependent
`CmdOrCtrl`, characters, function keys and named keys. That breadth does not
expand Unic's accepted configuration automatically. The current public list is:

- `Ctrl+\``.
- `CmdOrCtrl+Shift+Space`.
- `Ctrl+Alt+V`.
- `Cmd+Ctrl+V`.
- Empty to disable the registration.

Defaults remain `Cmd+Ctrl+V` on macOS and `Ctrl+Alt+V` on Windows/Linux.
An editor restricted to producing these exact stored strings stays within the
existing contract. It may record a recognized supported chord, reject the others
with a clear message, and reset to the OS default. It must not persist parser
aliases that the service rejects.

`GlobalShortcut.Register` canonicalizes its internal key for duplicate ownership,
claims it before the native call, and removes its claim when that call fails.
`IsRegistered` describes this app's ownership, not all applications' shortcuts.
macOS/Windows/X11 native registration can fail; Unic's `Shortcut.Set` registers
the replacement before unregistering the previous binding. Preserve that order.

On Wayland, registration uses the desktop portal asynchronously. The desktop can
ask the user to approve or select a binding. A successful method return or
`IsRegistered=true` must not be presented as confirmed portal approval. The pinned
public API has no authoritative final portal-approval status projection.

Do not use speculative registrations as a parse-only validator or copy mygo's
internal parser to broaden the accepted syntax. If free-form global editing is
approved, prefer a maintained public parse/normalize surface upstream. Define
accepted syntax and platform behavior explicitly before implementation.

## Capability matrix and next work

| Feature | Current behavior | Proposed target | Implementation status under the current contract |
|---|---|---|---|
| Global shortcut selection | Four choices or disabled, with requested/effective status and retry | Improve editing, reset and conflict presentation | Implementable now using the same choices and registration owner. |
| Free-form global recorder | No recorder; strict service choice validation | Record an arbitrary supported chord | Decision required: expands the explicit Shortcut preference choice list. Public mygo parse-only support is also missing. |
| Local shortcut overrides | Existing main/panel handlers use fixed actions | Configure nonreserved local actions | No blanket approval gate. First inventory action IDs and exclusions; preserve contracted panel commands and text editing. |
| Reserved panel command overrides | Enter pastes; Shift+Enter transforms to plain and pastes; Command/Ctrl+Enter pastes and reopens | Allow remapping these commands | Decision required: changes Session and commands semantics. A proposal may instead keep these commands reserved. |
| Panel enablement | Empty shortcut disables hotkey registration; other explicit panel-open paths remain possible | Independent enabled flag controlling all panel invocation paths | Approved field proposal; implementation can proceed with one tray-owned effective gate. Disabled must not discard the saved shortcut. |
| Panel position | Native default placement | Center or follow cursor, bounded to display work area | Approved field proposal; use existing Screen/Window APIs on supported platforms. Do not claim absolute positioning on Wayland. |
| Modifier double tap | No v2 monitor | Disabled, Alt, Control or Meta where actually supported | Approved field proposal; requires native monitor capability, lifecycle tests and explicit unavailable state. It is not a `GlobalShortcut` accelerator. |
| macOS permission status | Paste checks `AXIsProcessTrusted`; no settings guidance | Show actual Accessibility status; explicit request/open-settings action | Implementable with existing `permissionflow` module. Do not auto-request on page load or imply the page-open action grants permission. |
| Other platform permission status | Windows/X11 have no macOS Accessibility gate; Wayland target injection is unsupported | Accurate platform/capability status | Show unsupported/not-required separately from granted. Do not use `Allowed` to imply Wayland paste support. |

## Modifier-double-tap implementation evidence

Pinned mygo has no public cross-platform global modifier snapshot or double-tap
monitor. Focused `HandleInput` events stop being a global source when another
application has focus. They cannot implement the background trigger alone.

v1 provides a concrete reusable detector in `apps/gui-go/modifier_double_tap.go`:
20 ms snapshots, a 400 ms window, selected modifier only, another held key
invalidates the tap, and a bounded worker shutdown. It documents that a press and
release entirely between samples can be missed. Windows uses
`GetAsyncKeyState`; native X11 uses `XQueryKeymap`. The macOS v1 Go shell reports
that the native panel helper owns this feature; its Rust panel worker and core
detector are the reference rather than a nonexistent Go macOS monitor.

Reuse those established state-machine semantics and the existing x/sys,
purego and xgb dependencies where feasible. Do not add the retired native panel
as a v2 dependency. Inspect actual macOS snapshot APIs and permission behavior
before accepting a non-disabled preference there. Wayland and XWayland must
report unavailable, rather than deriving global state from an X11 client.

The tray must own a new monitor and invoke the same `openPanel` path. Stop it on
disable, preference changes, lightweight retirement and quit. A partial tap
cannot survive a modifier change, permission revocation, shutdown or new monitor
generation. No full keystroke content is retained or logged.

## Permission and placement boundaries

`permissionflow.Check(Accessibility)` is a passive macOS check. `Request` can
display the system prompt and add the app to System Settings, but returns before
the user grants permission. `OpenSettings` opens a page; `Guide` adds a native
guide and waits. `Request`, `Wait` and `Guide` must not block the main thread.
Use these mature APIs instead of hand-building System Settings URL handling.
Only explicit user actions request permission or open settings. Recheck after
activation and retain the current paste-time permission check.

Check the operation-owning executable/bundle identity. A GUI's status is not
independent evidence that a different tray/panel executable has permission.
Unknown, denied, unsupported and not-required remain distinct states. Permission
state is OS-owned and must not be stored as a trusted boolean in TOML.

For placement, sample the cursor when beginning an opening generation, select
its display and clamp the panel to `Display.WorkArea`. Use the same captured
generation when positioning the panel; do not recapture the paste target. Native
flyout anchoring continues to own preview/menu placement. Screen changes,
negative monitor coordinates and mixed DPI need native acceptance. The pinned
GTK backend notes that Wayland tells no global window positions; exposed
`SetPosition` alone is not proof of compositor support. Offer explicit
unavailability or compositor placement there.

## Required contract decisions

| Decision | Existing clause | Concrete proposed change | Alternative that needs no contract change |
|---|---|---|---|
| E1: Arbitrary global chord | Shortcut preference lists the four stored combinations and says they avoid duplicating the private parser. | Replace the finite choice contract with a maintained parse/normalize and validation contract; specify unsupported keys, conflict behavior, aliases and Wayland approval status. | Record/select only the existing combinations and keep the existing strict config validation. |
| E2: Core panel command remapping | Session and commands fixes Enter, Shift+Enter and Command/Ctrl+Enter behavior. | Define a stable action-ID mapping and user override rules for these commands, including plain-mode restrictions, target generation and no-retry semantics. | Keep the contracted commands reserved; expose overrides only for nonreserved local actions. |

The prior field proposals are already part of the agreed work plan. This review
does not create extra approval gates for panel enablement, placement, passive
permission status, reset-to-default or capability research. No runtime or public
contract extension is implemented by this review.

## Acceptance scenarios before implementation

1. Save each existing global combination and reset to the OS default through
   the production settings/service path; restart and read it back.
2. Reject an unsupported captured chord without saving or registering it.
   Recording consumes only its focused field's input and does not invoke an
   application action or system paste.
3. Force registration conflict; preserve the old effective registration, show
   the requested value and error, retry, disable and release on quit.
4. On Wayland, distinguish registration request from confirmed desktop binding;
   cancellation never becomes a success badge.
5. Disable the panel and exercise hotkey, tray, explicit settings button and
   private IPC entry paths. Reenable without losing the chosen shortcut. Verify
   normal and lightweight lifecycle boundaries.
6. Invoke each supported placement on multiple monitors, negative coordinates,
   screen edges and mixed DPI. Verify the original paste target is unchanged.
   Wayland must not report unsupported absolute placement as applied.
7. Feed modifier snapshots before the native implementation: clean double tap,
   held startup key, another key, repeat, timeout, modifier replacement,
   disable, cancellation and bounded shutdown. Then execute supported native
   backends. Keep the 20 ms sampling limitation visible if polling is retained.
8. Query denied/granted/unknown/unsupported permission states without requesting
   a prompt. Explicit settings guidance must stay distinct from a grant. Revoke
   permission before paste and verify no clipboard apply or synthetic input.
9. If E1 or E2 is approved, add its grammar/action cases before implementing the
   contract extension. Preserve current Copy, plain-mode rejection, focus
   validation, generation checks and one-shot input dispatch.

Use the [canonical Desktop runner](../../app/desktop/e2e/README.md) and retain
source/program identities, replay commands, verdicts and cleanup checksums.
Source inspection does not replace native shortcut delivery, permissions, focus
or actual receiving-application acceptance. This review ran no GUI, renderer,
browser, VM, native monitor, registration or permission request.
