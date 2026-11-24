# 0031 M2 mobile settings and media receipt

## Implementation

- Settings save the full Engine value using the current revision. A conflict
  refreshes the current settings and surfaces the error. Clipboard detection
  and automatic application persist as host preferences.
- The sync, history, storage, network and clipboard access pages expose live
  settings and state. Storage includes preview cleanup and v1 migration.
- History actions use system HTTP(S) handlers, photo and document pickers,
  camera capture and native sharing. The device send flow remains available
  separately. Word selection copies or shares the selected text.
- Mobile history reads file metadata through short-lived content leases. The
  folder view lists up to 200 snapshot entries or direct reference children.
- Event-specific updates coalesce within 120 ms. The client uses 200 text
  previews and 60 image preview entries in memory. The private disk preview
  cache holds up to 120 files and 100 MiB. Android share staging can live up
  to 24 hours so the receiving app can open its granted URI.

## Reproduction

Run these commands from the repository root.

```sh
(cd bindings && go run ./e2e/settings-media)
(cd bindings && go test -race -count=1 ./...)
go test -race -count=1 ./engine -run TestPublicEngineHasNoDesktopDependencies
./bindings/scripts/build-mobile.sh
(cd app/expo && npm run verify)
```

Run Expo verification before installing Pods or starting a native build.
Expo prebuild can recreate generated project directories.

The process E2E launches separate writer and reader processes against a
disposable directory. It checks settings after restart, stale settings
revisions, clipboard capabilities, cache usage, file names and sizes, exact
file and image export bytes, folder listing and truncation, manual preview
cleanup and orphan cleanup after history deletion. Its JSON output contains
ten passing check names and hashes for the exported fixture bytes.

| Fixture | SHA-256 |
| --- | --- |
| Exported file | `070e55f3faa9b3defb7592a8b998086a070ef5e5d51b8aaede40aef362a1364e` |
| Exported PNG | `93be9e66da42cfee00d9e4d0f9bb4e1fc4339fdaf41efb33b7f94c0c029de262` |

## Acceptance status

- Process E2E passed. The initial harness suspended networking before a commit.
  The first retry exposed that macOS resolves its temporary-directory alias
  before the Engine opens the file cache. Running with active runtime state
  and a canonical fixture path resolved both harness failures.
- The Go binding race suite, Engine boundary check, Expo bundle verification,
  bundle checksums, iOS Simulator arm64 Release build and Android arm64
  Release build passed. The first iOS build had no Pods workspace. Running
  Expo prebuild concurrently with a later build invalidated the generated
  directories. Sequential Expo verification, Pod installation and native
  builds resolved both failures.
- The v1 migration race suite passed with a canonical temporary directory.
  The first run used the macOS temporary-directory alias and failed at the
  existing Engine cache path check.
- Simulator and physical-device interaction acceptance is pending. To close M2,
  verify setting persistence after force-stop and relaunch, HTTP(S) opening,
  photo saving, document export, native sharing, photo selection, camera
  capture, selective copy and directory browsing on the target platforms.
