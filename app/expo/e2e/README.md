# Native shell E2E

These Maestro flows run the actual shell app. They cover navigation and detail
return, unavailable device actions, local settings, search/empty states, selection
and demo deletion. They produce screenshots and JUnit reports.

Use a disposable, already booted iOS Simulator or Android emulator. The runner
requires an explicit device ID and a real Release Simulator `.app` or `.apk`.
It verifies the isolated application ID and installs that exact artifact before
running flows. It never starts or deletes a device. Flows reset only the shell's
application data. Do not use a device with shell data you want to preserve.

Use Maestro 2.10.0, Xcode for iOS, and Android SDK tools plus `apkanalyzer` for
Android. `MAESTRO_BIN` and `APKANALYZER_BIN` may select installed tools.

Prepare a Release build with the project's normal Expo toolchain. For example,
from `app/expo`:

```sh
npm run prebuild
npx pod-install
xcodebuild -workspace ios/UniClipboardUIShell.xcworkspace \
  -scheme UniClipboardUIShell -configuration Release \
  -destination 'generic/platform=iOS Simulator' \
  -derivedDataPath /tmp/unic-expo-shell CODE_SIGNING_ALLOWED=NO
```

For Android, from `app/expo`:

```sh
(cd android && ./gradlew assembleRelease -PreactNativeArchitectures=arm64-v8a)
```

Select an already booted phone simulator/emulator, then run:

```sh
node e2e/run.mjs --platform ios --device <simulator-uuid> \
  --app /tmp/unic-expo-shell/Build/Products/Release-iphonesimulator/UniClipboardUIShell.app
node e2e/run.mjs --platform android --device emulator-5554 \
  --app android/app/build/outputs/apk/release/app-release.apk
```

The runner writes `e2e/results/<run>/receipt.json` on success and failure. It
records artifact/lockfile/flow hashes, device ID, tool version and every stage's
status. The same directory contains Maestro logs, screenshots and JUnit reports.
Repeat a run using the same artifact and flow revision. A bundle acceptance report
must always show native UI as `NOT RUN`; the native receipt is the separate proof.

The checked-in flows target a phone in the default grid layout. Also inspect
list/compact layouts, dark mode, all four languages, tablet workspace and iOS
sheet transitions during native acceptance. These additional cases are not yet
automated. No native flow was executed as part of the initial migration.

CLI options and artifact behavior follow the
[Maestro CLI reference](https://docs.maestro.dev/maestro-cli/maestro-cli-commands-and-options)
and [artifact reference](https://docs.maestro.dev/maestro-flows/workspace-management/test-reports-and-artifacts).
