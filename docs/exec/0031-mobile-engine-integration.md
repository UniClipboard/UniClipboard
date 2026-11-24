# 0031 Execution Receipt · Mobile Engine Integration (`bindings/mobile` + `app/expo`)

- Decision: [0031 Mobile Engine integration](../decisions/proposed/0031-mobile-engine-integration.md)
- Head baseline before changes: `c307f39f2` (merged `origin/main` at `78fde27dc`)
- Prototypes: [`prototype/setup-ios.html`](../../prototype/setup-ios.html), [`prototype/setup-android.html`](../../prototype/setup-android.html)

## Implemented Architecture

1. **Root Go credential build tags** (`internal/v1migration/`):
   - Restricted [`credentials_darwin.go`](../../internal/v1migration/credentials_darwin.go) to `darwin && !ios` and [`credentials_linux.go`](../../internal/v1migration/credentials_linux.go) to `linux && !android`.
   - Added [`credentials_mobile.go`](../../internal/v1migration/credentials_mobile.go) (`ios || android`) returning `credentialError(...)` (`errors.Is(..., ErrUnlock)`) so mobile hosts supply Keychain/passphrase secrets via `mobile.SecretProvider`.
2. **Nested Go mobile binding module** ([`bindings/`](../../bindings/go.mod), [`bindings/mobile/`](../../bindings/mobile/engine.go)):
   - Exposes gomobile-safe `Engine`, `MigrationSession`, `CancelToken`, `ContentStream`, `EventSink`, `FileOpener`, `FileStream`, `SecretProvider`, and `MigrationProgressSink`.
   - Embeds core `engine`, `builtin.tailcat`, `builtin.classify`, `builtin.text`, and `internal/v1migration` without importing any `app/*` package.
   - Maps `uint64` IDs/revisions as decimal strings in JSON, canonicalizes directory paths past symlinks (`resolveDirPath`), and closes content leases cleanly.
   - Built via [`bindings/scripts/build-mobile.sh`](../../bindings/scripts/build-mobile.sh) (`GOWORK=off`, pinned `go1.27.3` toolchain, `GOTOOLCHAIN=local`):
     - iOS XCFramework: `app/expo/modules/unic-engine/ios/Frameworks/UnicCore.xcframework` (named `UnicCore` to prevent Clang module collision with the `UnicEngine` CocoaPods target)
     - Android AAR: `app/expo/modules/unic-engine/android/libs/unic-engine.aar` (`-androidapi=24` matching Expo SDK 57 `minSdk = 24`)
3. **Expo Local Module (`app/expo/modules/unic-engine/`)**:
   - [`UnicEngineModule.swift`](../../app/expo/modules/unic-engine/ios/UnicEngineModule.swift): serial background queue execution, `.resolvingSymlinksInPath()` path normalization, `UNIC_MOBILE_DATA_DIR` / `UNIC_MOBILE_CACHE_DIR` / `UNIC_MOBILE_V1_DIR` test/runtime overrides, `UIApplication` foreground/background runtime state, `NWPathMonitor`, `UIPasteboard` apply/observe anti-loopback (`BeginApplyHistory` / `ReportApplyResult`), `UIDocumentPickerViewController` security-scoped `FileOpener` streaming, iOS Keychain `SecretProvider`, and `isIdleTimerDisabled` while migrating.
   - [`UnicEngineModule.kt`](../../app/expo/modules/unic-engine/android/src/main/java/app/uniclipboard/engine/UnicEngineModule.kt): single-thread background `CoroutineScope`, `build.gradle` local AAR unpacking (`classes.jar` + `jni/`), `ConnectivityManager.NetworkCallback`, `ClipboardManager` anti-loopback, SAF `ACTION_OPEN_DOCUMENT` / `ACTION_OPEN_DOCUMENT_TREE`, `ContentResolver` chunked `FileOpener`, and `FLAG_KEEP_SCREEN_ON` during migration.
4. **TypeScript Engine Client & Setup UI (`app/expo/src/`)**:
   - [`src/engine/{types,nativeModule,mapper,store}.ts`](../../app/expo/src/engine/store.ts): optional native binding (`requireOptionalNativeModule("UnicEngine")` from `"expo"`), strict fallback to `mode: "unavailable"` when absent in JS-only runs, live coalesced event refresh, Setup gating (`completeSetupAndEnterMain` called only on explicit enter/skip), and "Settings > Storage > Migrate from v1" shutdown/re-open handling.
   - [`src/screens/setup/`](../../app/expo/src/screens/setup/useSetupFlowController.ts): platform-resolved `MigrationSetupScreen.{ios,android}.tsx` sharing `useSetupFlowController.ts`, without `Platform.OS` branching.
   - [`UnicBrandLogo.tsx`](../../app/expo/src/components/UnicBrandLogo.tsx), [`app/expo/assets/unic-logo.svg`](../../app/expo/assets/unic-logo.svg), and `prototype/setup-{ios,android}.html`: aligned mobile launcher icons (`icon.png`, `adaptive-icon*.png`, `adaptive-icon-mono.png` wired in [`app/expo/app.json`](../../app/expo/app.json)), Setup vector emblem, and HTML prototypes to the official v2 pixel-cat mark (`unic-logo.svg` from `origin/main` PR #2294).
   - Wired into `AppNavigator.tsx`, `useHomeController.tsx`, `DevicesScreen.tsx` (mandatory fingerprint review before `ApproveDevice`), `DemoSendSheet.tsx`, `ClipboardDetailPage.tsx`, and `settings.ts`.

## Verification Record

### Go Core, Cross-Build & Binding Race Suite

| Command | Result | Notes |
|---|---|---|
| `TMPDIR=/private/tmp go test -count=1 ./engine -run TestPublicEngineHasNoDesktopDependencies` | PASS (`0.291s`) | Post-merge `engine/` has no desktop/UI dependencies |
| `TMPDIR=/private/tmp go test -count=1 ./internal/v1migration/...` | PASS (`0.474s`) | Post-merge cross-build tags verified (`darwin && !ios`, `linux && !android`, `ios \|\| android`) |
| `cd bindings && TMPDIR=/private/tmp go test -race -count=1 ./...` | PASS (`7.495s`) | Post-merge `builtinclassify.New(nil, nil)` adaptation; all 3 binding suites passed |
| `TMPDIR=/private/tmp go build ./app/service/cmd/... ./app/desktop/cmd/...` | PASS | Post-merge `unic`, `unicd`, and `unic-desktop` built cleanly |
| `TMPDIR=/private/tmp go test ./engine/...` (pre-existing HEAD check) | Partial FAIL (pre-existing) | All cases passed except `TestTailcatConnectedPublicationDeliversEverything`, which fails on baseline `c307f39f2` with `unavailable: the transfer failed` |

### Mobile Native Bindings & Binary Builds

| Artifact | Path | Size / Digest |
|---|---|---|
| `UnicCore.xcframework` (`ios,iossimulator`, min iOS 16.0) | `app/expo/modules/unic-engine/ios/Frameworks/UnicCore.xcframework` | `299M`; sim SHA-256 `6507033d37db5ab6dbd72816c0c4c7ba5e842817bdc19a197a8db5d97bc3a417`; device SHA-256 `cff7d864473bea7b659bfe34af0a1c5860ff51bdc14913ceb7f9c773beac49d9` |
| `unic-engine.aar` (`android/arm64,android/amd64`, API 24) | `app/expo/modules/unic-engine/android/libs/unic-engine.aar` | `56M`; SHA-256 `4f5b1c8cb8e0b940ce8236dde8a0d7084ed1bdad33ec6a6a03998dca1ffe7bf2` |
| iOS Simulator Release `.app` (`arm64`) | `/private/tmp/unic-ios-derived/Build/Products/Release-iphonesimulator/UniClipboardUIShell.app` | `188M` (CocoaPods 104 pods, `UnicEngine 0.1.0` + `UnicCore.xcframework`, Xcode 27.0) |
| Android Debug APK | `app/expo/android/app/build/outputs/apk/debug/app-debug.apk` | `309M`; SHA-256 `4b1f3168c7a02b6e9842a454d11c422ac4b29be5bc039666371494a082cec4a6`; packages `lib/arm64-v8a/libgojni.so` (`41,696,008` B), `lib/x86_64/libgojni.so` (`44,238,184` B), and regenerated pixel-cat `mipmap-*/ic_launcher*.webp` (`ic_launcher_monochrome.webp` included) |

### Expo Bundle Acceptance (`app/expo`)

- `(cd app/expo && npm run verify)` -> **PASS** (`dist/verification/report.json`):
  - `source-boundary`: 236 files, exit 0
  - `types` (`tsc --noEmit`): exit 0
  - `search-tab-patch`, `tab-animation-patch`: exit 0
  - `prebuild` (`expo prebuild --no-install --platform all`): exit 0 (regenerated iOS `AppIcon.appiconset` and Android `mipmap-*/ic_launcher*.webp` with the v2 pixel-cat logo)
  - `bundle-ios` (3458 modules, 24 artifacts), `bundle-android` (1820 modules, 35 artifacts): exit 0

### iOS Simulator Runtime Verification (`iPhone 17 Pro`, UDID `A1B5F28E-5DB7-41BE-A80A-DFEB6608EDA2`)

Executed the compiled Release `UniClipboardUIShell.app` against isolated fixture directories under `/private/tmp/unic-sim-fixtures` (`SIMCTL_CHILD_UNIC_MOBILE_DATA_DIR` / `SIMCTL_CHILD_UNIC_MOBILE_V1_DIR`) and inspected screenshots:

1. **Fresh clean profile (`fresh-data-v2logo` + empty `empty-v1`)**:
   - Native `MobileOpenSetupFlow` + `DetectSource("")` wrote `fresh-data-v2logo/mobile-setup.json` (`{"complete":false}`).
   - Rendered Liquid Glass setup welcome screen with the vector v2 pixel-cat `UnicBrandLogo` (`欢迎使用 UniClipboard`, `让每一次复制更有用`, `全新空间已就绪`, primary capsule button `开始使用`, secondary `手动选择旧版目录`). Screenshot: `/private/tmp/unic-mobile-ios-welcome-clean-v2logo.png`.
2. **Detected v1 fixture profile (`v1-target-v2logo` + `v1fixture.Build` `v1-src`)**:
   - Native `DetectSource("")` located `vault/keyslot.json` and wrote `v1-target-v2logo/mobile-setup.json` (`{"complete":false,"source":"/private/tmp/unic-sim-fixtures/v1-src"}`).
   - Rendered v1 detected state with the vector v2 pixel-cat `UnicBrandLogo` (`已检测到旧版数据`, `发现可迁移的旧版剪贴板数据`, `/private/tmp/unic-sim-fixtures/v1-src`, `开始迁移`, `选择其他旧版目录`, `暂不迁移`). Screenshot: `/private/tmp/unic-mobile-ios-welcome-detected-v2logo.png`.
3. **Seeded v2 profile (`seeded-v2` with `complete: true` and 3 real Engine records)**:
   - Bypassed Setup and launched directly into native Liquid Glass History (`剪贴板` header + floating tab bar, no `shell-banner`).
   - Displayed the 3 persisted Go Engine records (text cards + classified `github.com/UniClipboard/UniClipboard` link card hydrated via `ReadHistoryTextPreview`). Screenshot: `/private/tmp/unic-mobile-ios-live-history.png`.

## Remaining Acceptance Items

- Interactive physical-device (and Android Emulator) verification of live Tailcat peer-to-peer pairing/transfer over real Wi-Fi/cellular interfaces, real Keychain / BiometricPrompt v1 unlock, background suspension/resumption (`SetRuntimeState`), SAF external tree paths, and interactive tap-through of migration progress (`m-importing` / `m-verifying` / `r-done` / `r-partial` / `r-cancelled`) remain to be executed on target devices.
- Production bundle ID / App Group / Keychain entitlement & release signing pipelines are separate scope under [0018](../decisions/proposed/0018-independent-product-build-and-release.md).
