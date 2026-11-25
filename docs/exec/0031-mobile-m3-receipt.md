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
and Keychain-backed upgrade remain open. The iPhone run below exercises the
passphrase path with a compatible synthetic backup.

Physical-device camera pairing, slow-network pairing, iOS Share Extension
behavior, and physical-device background receive require device evidence.

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

## iOS physical-device acceptance

XCUITest ran on an iPhone 16 Pro with iOS 27.3 (24C5037i). The Release build
used the isolated `app.uniclipboard.shell` identifier with a development
profile. The app and extension build artifacts compiled for iOS arm64. The
installed app used the app-only profile because the shell and extension
profiles did not contain the isolated App Group entitlement.

The first device run used a stale app container path after installation. The
retry resolved the current path and found that the native settings table
omitted the local device ID. On-device accessibility data also showed that
the modal backdrop grouped the controls into a single element. The fixes
restore the settings subtitle and expose the modal controls individually.
The bounded runner retry waits for the app process and the actual controls
while excluding animation-idle notifications from XCTest synchronization.

| Scenario | iPhone result |
| --- | --- |
| Local identity and invalid manual input | The device ID appeared and invalid input triggered the device error prompt. |
| Ticket QR | The app displayed an Engine-generated QR code and removed it when the sheet closed. |
| Six-digit code failure | The operation row reported a terminal error against the disposable rendezvous endpoint. Stopping observation removed the row. |
| History pin and tag | The app exposed Unpin and the new tag. SQLite readback confirmed both tags on the fixture text. |
| Pairing approval and rename | The app displayed fingerprint review for the staged pending peer. It moved the row after confirmation and preserved the edited name after restart. SQLite readback confirmed trusted status and the new name. Two local Engine processes established the staged pending state. |
| Compatible encrypted backup | The app accepted the passphrase, reported partial import for the deliberately malformed record, and opened History. Four valid records survived retention. |

The first compatible migration fixture used old timestamps. The Engine
imported the four valid records and then expired them under its default
history retention. A freshly dated copy went through the same passphrase
and partial-import screens. SQLite readback from the iPhone confirmed two
text records, one image record and one file record with `migration` origin.

The final retries corrected the invalid-input message assertion and restored a
fresh pending peer fixture. The app executable SHA-256 was
`68dc9e4cede5d1c4de9f23c7f5c669522aecf60021af0fc944fd61b131254265`.
The JavaScript bundle SHA-256 was
`002685bdf28df4be9c43acbf2d277ffc95ac4e6254367d1acf4e760a9f6e3a87`.
The reproducibility archive includes the test source template, build steps,
source patch and filtered readback. Physical QR scanning and successful
pairing from a physical phone remain open.

### iOS Share Extension acceptance

The follow-up registered `group.app.uniclipboard.shell.incoming-share` and
assigned it to the isolated application and its `.Share` extension. A new
arm64 Release build used development profiles that each contained the exact
App Group. The final signatures for both bundles included that entitlement.
The complete signed application installed on the same iPhone.

The original physical run staged a file URL as `shared-file` because the
provider did not supply a suggested name. The extension now uses the source
URL to recover the file name and type when the item provider offers a file
URL. The corrected Release build ran against a new disposable profile.

| System share sample | Readback after reopening the iPhone profile |
| --- | --- |
| Text | The saved text matched the 40-byte fixture. |
| PNG | The record used the image kind and PNG MIME type. Export matched the 359-byte original, including the 32 × 32 dimensions and two sampled pixels. |
| File | Export retained `m03-physical-share.txt` and matched the 41-byte original. |

The system share sheet opened the isolated extension for each sample. Its
receiving sheet saved all three through **Save to Local History**. The
reopened profile showed that sync was enabled and each item had
`not_requested` sync state and no backend dispatch record.

The first retry updated the fixture app for the iOS 27 scene lifecycle. The
test then scoped gestures to the system app row and selected the isolated
app in **More**. The first corrected-build run passed text and staged the
two remaining shares. The PNG provider offered a synthesized preview name.
The receive-only retry accepted that preview, saved the PNG and then saved
the file already queued behind it. The archive includes all run summaries,
the source patch, fixture and runner sources, both signing checks, and the
independent profile readback. The final extension executable SHA-256 was
`26dc05fbdd89ad68a2b699dc92fa4685999147d642270029a9aeca85327888a3`.
