# Desktop shortcut permission settings

- Scope: passive Accessibility permission status and an explicit System Settings action.
- Status: runtime implemented; canonical nonvisual process acceptance passed. Native permission acceptance remains not run.
- Capability review: [shortcut capabilities and contract boundaries](desktop-settings-shortcut-capabilities.md).
- Existing contract: [Desktop quick panel](../contracts/desktop-quick-panel-contract.md).

## Implemented behavior

Settings > Quick Panel displays the current application's Accessibility
permission status. It distinguishes granted, denied, unknown and a platform
without this permission switch. It does not label the receipt as automatic-paste
readiness. Native target validation, secure-input restrictions, focus checks and
the paste owner's final permission check remain independent.

The Desktop host reuses pinned mygo's `plugins/permissionflow` package.
On macOS, a passive read calls `Check(Accessibility)`. The explicit button calls
`OpenSettings(Accessibility)`. It never calls `Request` or `Guide`, never changes
OS permission state, and never opens System Settings from a passive read.
Other platforms expose unsupported status and no settings action for this
macOS-specific permission switch.

The host queries the current executable. Installed GUI, tray and panel roles
share one executable/bundle identity, but this receipt does not prove permission
for another binary or profile. The actual native paste owner retains its
immediate pre-operation checks.

Status reads and settings actions use independent workers and bounded contexts.
Neither blocks the other or the live service/configuration queue. Completions
return through the existing view-owned result queue. A binding generation and
the Desktop context reject replaced or cancelled results. Raw native errors are
not displayed. An accepted settings-open request produces guidance, never a
permission-granted result.

No configuration field, role IPC command, accelerator grammar, core panel
command, global monitor or public protocol changes are part of this slice.
The fixed global choices and panel command contract remain unchanged.

## Acceptance written before implementation

The `settings-shortcuts` canonical case builds a production `unicd`, starts a
private profile, and uses the real Desktop live binding. Only the native host
permission callbacks are controlled. No renderer, native permission API, GUI,
browser, OS settings page, system clipboard or personal profile is used.

Failure scenarios:

1. A passive read opens settings or requests permission.
2. Opening settings implies that permission was granted.
3. A blocked read prevents the settings action from completing.
4. An unsupported or failed read becomes granted.
5. A native error exposes its private text.
6. A replaced callback overwrites a newer status.
7. A cancelled Desktop publishes a late settings-open success.

```sh
make desktop-e2e CASE=settings-shortcuts
```

Keep the first failed artifact. Diagnose before at most two corrective replays.
The scenario receipt binds the production service hash, completed check groups,
permission-substitution boundary and cleanup. The canonical manifest binds the
source snapshot and all retained evidence hashes.

## Executed receipt

Canonical run `20261122T073218Z-d03f705f` passed all five process check groups.
The case and scenario receipts report PASS; private cleanup passed. All five
entries in `SHA256SUMS` verified independently. The production service SHA-256 is
`ec75114c75d79356f11566c1e26c15ea589bb4d97bf4cf86f711d7b1e73d6795`.
The scenario explicitly records `native_permission=false`, `native_ui=false`,
`renderer=false` and `system_clipboard=false`. No failed attempt preceded this
run for this permission slice. The earlier appearance failures remain separate.

## Remaining acceptance

Actual control interaction, macOS permission reporting and settings navigation,
denial/revocation, native executable/bundle identity and other-platform native
presentation remain not run. A callback-based permission receipt cannot prove
OS authorization, shortcut delivery, native focus or receiving-application paste.
Free-form shortcut recording and reserved-command remapping still require the
contract decisions identified in the capability review. Modifier double-tap,
panel enablement and placement are separate unimplemented settings slices.
