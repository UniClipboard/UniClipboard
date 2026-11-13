# Expo UI shell migration

## Accepted scope

Import the existing Expo mobile UI from `UniClipboard/UniClip` commit
`ac90f94805a9f2003f6e319aa84484cf0bd615c9` into `app/expo`.
Keep one Expo project for Android and iOS. Preserve platform-resolved UI,
existing components, navigation, history layouts, detail presentation, themes,
languages, settings, onboarding and device presentation.

Use explicit in-memory demo records. Local presentation actions may filter,
select, edit or delete demo records. Network, pairing, transfer, clipboard,
permissions, diagnostics and extension actions must report that integration is
unavailable. They must not claim a real operation succeeded.

Do not import the old Engine, business runtime, database, migration, background
services, native Share/Keyboard targets, telemetry or release infrastructure.
Do not introduce an Engine binding or future integration abstraction.

## Sequence

1. Freeze the source identity and inventory the presentation dependencies.
2. Import original presentation files and required UI assets/patches.
3. Replace runtime ownership with a small, explicit shell state and composition.
4. Install locked dependencies and check TypeScript and both native bundles.
5. Record imported-file provenance and validation artifacts.
6. Update the repository overview and platform placeholders.

## Failure scenarios to verify

- A presentation import reaches the old Engine or a native service.
- The app waits for settings, a database or a network service during startup.
- A demo operation reports real send, pair, copy or sync success.
- Search, filters, selection or deletion disagree with the displayed records.
- Closing search or a detail page loses the underlying navigation state.
- A platform resolver selects the wrong UI or a native dependency is missing.
- A navigation patch is silently skipped after dependency drift.
- Build output lacks source identity or reproducible checksums.

## Acceptance boundaries

Bundle acceptance checks the real Metro dependency graph for iOS and Android.
It is not native visual or interaction acceptance. Native interaction acceptance
uses the checked-in Maestro flows and records reports/screenshots against the
actual app artifact. Never count an unexecuted flow as passing.

## Results

Implemented under `app/expo`. The source manifest records 311 upstream paths;
270 files retain the exact upstream bytes. Adaptations replace runtime ownership,
remove system operations, preserve UI build patches and provide shell composition.
The imported MIT license and notices remain in the application directory.

Validation on 2026-11-13, Node 22.23.4 and npm 10.9.9:

- Clean `npm ci --offline` from the locked graph: passed, 746 packages installed;
  both mandatory navigation patches applied.
- `npm run verify` equivalent (`node scripts/verify-shell.mjs`): passed.
  It checks 227 project source files, TypeScript, both patches, clean dual-platform
  Expo prebuild without dependency installation, and real Metro/Hermes exports.
- iOS: 3,451 source modules and 24 output files, including bundle/map/metadata/assets.
- Android: 1,813 source modules and 35 output files.
- Both source-map boundary checks passed. Neither bundle includes the old
  application feature, platform, support or runtime source directories.
- Source-tree SHA-256:
  `ad71fbab3f4252c1ab6364429597fc71b161b0c7faec4b67126962277727932b`.
- All three Maestro flow files and the CI workflow parse as YAML.
- Documentation link/architecture checks passed with six pre-existing stale-page
  warnings. The overview marker names source commit
  `98ceabdfe`; unrelated verification claims remain unchanged.

Local logs, module counts and checksums are in
`app/expo/dist/verification/report.json`, `ios.sha256` and `android.sha256`.
`npm run verify` reproduces them. The CI workflow runs the same bundle checks;
it has not been dispatched or observed remotely in this migration.

At the initial migration checkpoint, native `.app`/APK compilation,
CocoaPods/Gradle acceptance, application launch, visual inspection and Maestro execution were **NOT RUN**. The native flows and
runner are supplied, but are not counted as passing. List/dark-mode/language/tablet
visual acceptance also remains pending. No Engine integration, release or
publication is included.

## iOS Simulator follow-up

At the user's explicit request, source commit
`041901389` was built with Xcode 27.0 (27A5194q) and installed on an iPhone 17 Pro
Simulator running iOS 26.3 on 2026-11-13.

- CocoaPods installation: passed.
- Release Simulator build: passed with code signing disabled.
- Installation and launch of `app.uniclipboard.shell`: passed.
- Home screenshot: inspected and sent through Lody. It shows the demo banner,
  history cards and the native tab bar.
- Main executable SHA-256:
  `0ebb9dca7d4071fae6c2532358d87a929892e4d79649f69721f6e992e7e74df7`.
- Screenshot SHA-256:
  `7e9e277f253bfd0af836cd8b18e9bb4d68ddd8360ea4e4d41bcdaad13c1fd999`.

Reproduce the build from `app/expo` after installing locked npm dependencies:

```sh
(cd ios && pod install)
xcodebuild -workspace ios/UniClipboardUIShell.xcworkspace \
  -scheme UniClipboardUIShell -configuration Release \
  -destination 'generic/platform=iOS Simulator' \
  -derivedDataPath /tmp/unic-expo-shell-simulator \
  CODE_SIGNING_ALLOWED=NO -jobs 6 build
```

This follow-up verifies native startup and the home screenshot only. Maestro
flows, settings/device interaction, tablet layouts, other appearances/languages,
Android native compilation and Android startup remain unverified. No application
source changed during this follow-up. Temporary native artifacts and screenshot
files are not committed.
