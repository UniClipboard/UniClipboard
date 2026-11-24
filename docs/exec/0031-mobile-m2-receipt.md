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
## Simulator acceptance

The native app came from source commit
`b1e52bc6811501bb7ff1aac4047b4966fe3ca763`. The test used Maestro 2.10.0
and disposable simulators. It verified both app IDs and Release hashes.

| Platform | Tested Release SHA-256 |
| --- | --- |
| iPhone 17 Pro, iOS 26.3 Simulator executable | `61d6167c4df28a6a03eb7f6a15d6b2173301a397def590e76d88e0bd8182ae09` |
| Pixel 8, Android API 36 arm64 Release APK | `e0be3c3e91bca0839e5ce4adf3b648a40f30ba33b08a2a967203873a601c348d` |

| Scenario | iOS | Android |
| --- | --- | --- |
| Edited Engine settings and host switches after process termination and relaunch | Passed | Passed |
| Clipboard capabilities, file-cache usage and preview removal | Passed | Passed |
| File metadata and directory contents | Passed | Passed |
| HTTP(S) handoff to the system browser | Passed | Passed |
| Selected-word copy and system clipboard readback | Passed | Passed |
| Native share sheet with the fixture document | Passed | Passed |
| Document export and photo save with original bytes | Passed | Passed |
| Photo Picker import with matching fixture pixels and dimensions | Passed | Passed |
| Camera capture and history commit | Simulator reported the camera as unavailable | Passed with the emulator virtual scene |

The first runs retained JUnit reports, screenshots and command traces.
Android staging failed when `adb push` widened the fixture's private
permissions. Preserving modes and applying the app sandbox ownership and
label resolved fixture startup. Other retries adjusted localized selectors,
waited for the iOS detail transition and kept iOS photo selection in the
same flow as picker presentation. The Android exact-value readback caught a
URL suffix left by the test cursor. A focused retry verified the exact URL
after relaunch. Production app source stayed at the same revision.

The simulator evidence includes fixture generation and staging instructions,
Maestro flows, JUnit reports, original failures, screenshots and byte-level
readback. Physical-device acceptance and iOS hardware camera capture remain
pending.
