# Expo mobile UI shell

This directory owns the shared Android and iOS Expo application. It imports the
mobile presentation from [UniClipboard/UniClip](https://github.com/UniClipboard/UniClip)
commit `ac90f94805a9f2003f6e319aa84484cf0bd615c9`.
The original MIT notice is retained in [LICENSE](LICENSE).
The [source manifest](migration-source.json) records upstream file hashes and
which files were adapted during migration.

## Scope

- Original platform navigation: native iOS tabs and Android floating tabs/rail.
- Original history cards, list/compact layouts, search, filters, multi-selection,
  context menus, word selection, phone detail pages and tablet workspace.
- Original theme, translations, onboarding artwork and platform UI components.
- Settings and device composition reuse the original native rows and groups.
  History preferences, demo reset, appearance and language are interactive.
  System-dependent settings have explicit unavailable pages. Device names are
  fixtures. The connection sheet is a shell-only presentation.

All history is generated in memory. Deletion and favorites affect demo records
only. Demo preferences reset with the process, except appearance, history layout
and word-picker UI preferences, which use this app's own AsyncStorage sandbox.
The main screen opens directly. Onboarding remains available from About and
Developer settings. Every main screen displays the demo banner.

The shell does not start an Engine, open the old database, perform network
requests, access the system clipboard, pair devices, transfer content, register
background services or emit analytics. Those actions show an unavailable notice.
No Engine binding or future integration interface is defined here. Existing
clipboard DTOs are local presentation types, not the new Engine contract.

## Development

Use Node from `.nvmrc` and npm 10. Run commands from this directory:

```sh
nvm use
npm ci
npm start
```

Build and run on an explicitly selected development device:

```sh
npm run ios -- --device <device-id>
npm run android -- --device <device-id>
```

`npm run prebuild` generates the ignored `ios/` and `android/` directories under
this directory. The sibling `app/ios` and `app/android` directories are not build
inputs. The shell uses the isolated identifier `app.uniclipboard.shell` on both
platforms. It does not share App Groups, keychain groups or URL schemes with the
production mobile application. No signing or release workflow is migrated.

The source's pinned `react-native-screens` search-tab and animation patches remain
mandatory. `npm ci` applies them and fails on a dependency mismatch. The two
Xcode 27 plugins retain the source's Scene lifecycle and Pod deployment-target
adaptation. `expo-system-ui` enables Android theme handling.

## Bundle acceptance

```sh
npm run verify
shasum -a 256 -c dist/verification/ios.sha256
shasum -a 256 -c dist/verification/android.sha256
```

The command checks integration boundaries, TypeScript, navigation patches,
Expo prebuild without native dependency installation, and both real Metro/Hermes
bundles. It writes logs, source identity, bundle module counts and checksums to
`dist/verification/`. Both bundle directories include source maps. The CI workflow
runs the same command and uploads its artifacts. This is bundle acceptance, not
native build, visual or interaction acceptance.

## Native UI E2E

The [E2E guide](e2e/README.md) describes the native Maestro flows and artifact
receipt. Native builds and native UI execution are separate from bundle
acceptance. Do not count an unexecuted flow as passing.

See the [migration receipt](../../docs/exec/expo-ui-shell.md) and
[repository architecture](../../docs/architecture/overview.md).
