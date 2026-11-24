# 0031 M3 mobile pairing, migration and operating-system entry points

## Implementation

- The device page displays the local name and device ID. The fingerprint shown
  for a peer is the shared pairing fingerprint. Both peers still require user
  approval. The app renders a ticket QR code, accepts a scanned ticket or
  six-digit code, and reads the Engine operation on events and resync. Closing
  progress stops observation of the operation. The Engine can finish pairing
  and surface a pending peer afterward.
- Device rename and candidate ignore use the current Engine state. History
  uses revision-checked pin and tag changes. The normal history list prefixes
  the latest page of matching pinned items. Older pinned items are available
  through the tag filter.
- Android stages the selected Storage Access Framework backup in private
  cache for the existing v1 migrator. It checks `vault/keyslot.json`, validates
  child names, and bounds bytes, tree depth and entry count. Completed or
  cancelled migration clears the staged copy. An in-session retry can stage
  the same granted tree again. The source remains read-only.
- Android accepts single and multiple system-share intents. The iOS Share
  Extension stages content in an isolated App Group and returns to the sending
  application. The main application presents each staged item for confirmation.
  It saves locally or dispatches to one selected device. It processes PNG
  as an image representation and preserves other binary formats as files.
  The extension has no Engine dependency.
- Android background sync defaults off. It requests notification access,
  uses a `dataSync` foreground service, and returns to the suspended Engine
  state when Android stops the service while the app is in the background.
  Background clipboard access remains unavailable.
- Pairing, sending and migration user interface strings are available in
  Simplified Chinese, English, Russian and Brazilian Portuguese. Existing
  appearance and history-display preferences are read if the production
  storage keys have not yet been written.

## Local commit contract

`CommitRequest.LocalOnly` preserves captured processing and local persistence
policy while bypassing all backend dispatch. The mobile JSON key is
`local_only`. Local and targeted submissions are mutually exclusive. The
request mode participates in idempotency. The zero value preserves the
behavior of all existing call sites. See the
[Engine contract](../contracts/engine-contract.md) and
[pipeline contract](../contracts/pipeline-contract.md).

The process E2E uses separate writer and reader processes and a disposable
profile with sync enabled. It exercises local-only submission, changed-mode
conflict, target rejection, local text/image/file shares, replay and history
readback after process restart. Before the Engine option was implemented, the
request reached dispatch and returned `no_targets`. The fixed path returned
`not_requested` and created no send operation.

Run from the repository root:

```sh
(cd bindings && go run ./e2e/local-commit)
(cd bindings && go test -race ./...)
./bindings/scripts/build-mobile.sh
(cd app/expo && npm run verify)
```

## Migration compatibility and acceptance limits

The mobile release source at
[UniClip v1.3.0.165](https://github.com/UniClipboard/UniClip/tree/v1.3.0.165)
writes history through [its Expo SQLite database](https://github.com/UniClipboard/UniClip/blob/v1.3.0.165/src/services/db/database.ts).
Its [SecureStorage layer](https://github.com/UniClipboard/UniClip/blob/v1.3.0.165/src/services/SecureStorage.ts)
uses AsyncStorage for app credentials. The existing Go migration contract
requires the encrypted backup format containing `vault/keyslot.json`. An
Android Keystore alias, iOS Keychain access group and matching mobile backup
format for that vault have not been established. The existing passphrase
path applies to compatible v1 backups. Import of the released mobile history
and physical-device coverage of both upgrade paths remain open.

Physical-device camera pairing, slow-network pairing, provisioned iOS Share
Extension acceptance, and physical-device background receive are separate
from native compilation. The isolated iOS share group and extension need
provisioning before a physical-device run.

## Verification

- The two-process E2E passed with four history records after reopen. Exported
  PNG and file payloads matched the staged bytes.
- The root and bindings Go race suites passed. The first root suite run used a
  macOS temporary path that exceeded the Unix socket limit and triggered the
  default cache path check. Re-running with a short canonical directory under
  `/private/tmp` passed.
- The iOS Simulator Release and Android arm64 Release builds passed. The
  iOS build embedded the `app.uniclipboard.shell.Share` extension. The
  extension executable imports no Engine or React Native framework. The
  Android merged manifest contains both share intent actions and the
  `dataSync` foreground service.
- The four locale sets have matching keys for the changed views and errors.
- Expo type checking, native prebuild, both Metro/Hermes bundles, and
  checksum verification passed. The initial source boundary rejected the
  camera dependency. The verifier now permits it only in the pairing
  scanner.
- The native UI, simulator pairing, and physical-device checks remain open.
