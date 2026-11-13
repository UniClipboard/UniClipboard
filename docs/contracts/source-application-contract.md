# Clipboard source application metadata

## Data model

`source_application` is optional on clipboard snapshots, content inputs, content descriptors,
history items, processor items, backend views, and peer clip headers.
The Engine exports the shared `sourceapp.Application` as `engine.SourceApplication`.
The previous unused `source_app` string is removed. This unreleased schema has no migration.

| Field | Meaning | Bound |
| --- | --- | --- |
| `platform` | Origin application platform: `darwin`, `windows`, `linux`, `android`, `ios` | Enumerated |
| `identifier` | Bundle ID, application ID, WM_CLASS, or executable basename | Required; 256 UTF-8 bytes |
| `name` | Optional application display name | 256 UTF-8 bytes |
| `attribution` | `clipboard_owner`, `source_marker`, or `foreground` | Enumerated |
| `icon_png` | Optional PNG bytes; JSON uses base64 | 16 KiB; dimensions 1..128 |

Identity fields reject control characters and path separators.
Metadata never contains executable paths, window titles, command lines, or document names.
The application identity is a sender-supplied hint. Device authentication does not prove it.
`foreground` is explicitly an inference; polling can observe an application activated after copying.

## Capture and icons

The Desktop host captures provenance alongside the clipboard read.
macOS prefers `org.nspasteboard.source`; otherwise it samples `NSWorkspace.frontmostApplication`.
It uses the bundle identifier, localized name, and `NSRunningApplication.icon` or the installed bundle icon.
This follows the foreground attribution pattern used by [Maccy](https://github.com/p0deje/Maccy/blob/master/Maccy/Clipboard.swift).
The [AppKit application API](https://developer.apple.com/documentation/appkit/nsrunningapplication) supplies these attributes.

Windows resolves the [clipboard owner](https://learn.microsoft.com/en-us/windows/win32/dataxchg/clipboard-operations),
uses a lowercase executable basename as the identifier, and extracts the executable icon when available.
The executable path remains local. This identifier does not distinguish installations with the same basename.
X11 reads the CLIPBOARD selection owner's WM_CLASS and optional `_NET_WM_ICON`.
An owner window without WM_CLASS has unknown provenance. Native Wayland provenance remains unavailable.
Unsupported platforms and unavailable identities return no source metadata.

macOS and Windows check clipboard change counters around the content read.
X11 checks the selection owner; changes within the same owner retain the existing non-atomic read limitation.
The host retries a read if this marker changes.
Icon extraction failure produces an identity without an icon.
The host normalizes extracted icons to 64 by 64 PNG pixels.
Its icon cache holds at most 128 entries, including negative entries, for five minutes.

## Persistence, synchronization, and ownership

Import validates metadata and copies icon bytes. Returned DTOs are independent snapshots.
Malformed identity or supplied icons return `invalid_argument` before commit.
Native extraction validates and drops an unavailable icon before import.
PNG validation checks encoded size and decoded dimensions before decoding pixels.

History caches the complete immutable metadata snapshot, including icon bytes, in its SQLite record.
The exact encoded metadata length counts toward history and live-content storage budgets.
Delete, expiry, and ordinary history cleanup remove the cached icon with its record.
A retained history content lease remains valid under the existing content ownership rules.
No separate unbounded icon directory or retention policy exists.

Adapter IPC, in-process and external plugin backends, RPC, and version-1 peer wire headers preserve metadata.
Peer receive admission validates and reserves metadata storage before reading content bodies.
Receipt uses the admitted metadata; it never substitutes the receiving application.
History restore sends the stored metadata. Applying clipboard content does not manufacture a new capture.
Directory source references retain their captured application when explicit or automatic preparation creates an immutable snapshot.
Local copy-action markers remain separate from application provenance and are never transmitted.
Action results and explicit submissions have no inferred application; a host may supply metadata on import.

Application metadata is excluded from content hashes, observation comparisons, and cross-backend view hashes.
A metadata change alone therefore does not create history or trigger sync.
Existing baseline, echo, and unchanged-content suppression stays in force.
Application fields and icons stay out of observer events, operational logs, analytics, and errors.
History list APIs include the bounded metadata object; they perform no application lookup or network icon fetch.

## Acceptance

- `UNIC_E2E=1 go test ./e2e/cli -run 'TestCLI/clipboard-source-metadata$' -count=1 -v`
  covers capture through adapter IPC, two service processes, peer sync, restart, restore sync,
  missing icons, unknown source, and metadata-only changes.
- `UNIC_HEADLESS_E2E=1 go test -race ./e2e/headless -run '^TestProductionHeadless$' -count=1 -v`
  covers production binaries, rich text/image/file metadata, received descriptors, history, and invalid metadata rejection.
- Set `UNIC_E2E_ARTIFACTS` and `UNIC_HEADLESS_ARTIFACTS` to retain reproducible manifests.
  Manifests record source files and binary hashes. These process runs do not prove native OS attribution or icon extraction.

## Desktop detail presentation

Desktop displays the original application's icon and name immediately after device/time in the history detail header.
An empty display name falls back to the identifier. Missing or malformed icon bytes use a generic application icon.
Unknown provenance adds no application item. Long names truncate inside the detail header, with the complete name and identifier in the tooltip.
The source application image is independent of clipboard preview images and survives asynchronous content-detail refresh.
Only the accessible group label is localized; application identities remain unchanged.
