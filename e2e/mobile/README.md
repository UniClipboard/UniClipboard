# Mobile Runtime E2E Verification (`e2e`)

End-to-end verification suite for the mobile UniClipboard app (`app/expo`), built with the `e2e` framework (`e2e@0.24.0` + `@e2e-dev/mobile@0.12.8`) matching the repository's web E2E harness pattern in `origin/v1`.

All tests are deterministic (`test` + `screen` + `device` + `expect`) and do not require LLM API keys.

## Scenarios Covered

1. **Complete v1 migration (`complete`)**:
   - Seeds an encrypted v1 directory (`keyslot.json` + `profile.sqlite` with text, HTML, tags, PNG image, binary file, repaired row, and soft-deleted tombstone excluded from migration).
   - Launches the native app with isolated `UNIC_MOBILE_DATA_DIR`, `UNIC_MOBILE_CACHE_DIR`, and `UNIC_MOBILE_V1_DIR` launch overrides.
   - Drives first-install Setup (`已检测到旧版数据` → tap `开始迁移` → Keychain unavailable → `m-auth`), tests wrong passphrase recovery (`旧口令不正确或无法解锁凭据，请重新输入`), submits the valid passphrase, asserts `迁移完成` / `已完整校验`, and enters live History (`剪贴板` / `Clipboard`, `shell-banner` absent, migrated entries rendered).
   - Relaunches the app against the migrated data directory and asserts Setup is bypassed directly into live History.
   - Closes the app and runs [`cmd/v1fixturegen`](cmd/v1fixturegen/main.go) (`-mode verify -delete-source`) to independently assert:
     - `mobile-setup.json` marks setup complete;
     - source `profile.sqlite` SHA-256 is unchanged (read-only access);
     - v1 source directory deletion does not affect v2 storage;
     - opening the live Go `engine.Engine` reads back all 5 records (`origin=migration`), `"标签"` and `"unused"` tags, and byte-exact `pixel.png` and `binary\x00fixture` attachments.
2. **Partial v1 migration (`partial`)**:
   - Seeds a fixture with one broken representation (`entry-bad`), verifies the UI reports `部分记录未能导入` honestly rather than claiming `迁移完成`, verifies entry into live History, and confirms 4 records + tags/attachments via `v1fixturegen`.
3. **Clean fresh install (`clean`)**:
   - Launches with an empty v1 directory, verifies `全新空间已就绪` → `开始使用` enters live History with zero blocking, and verifies `mobile-setup.json` and a valid empty v2 Engine store.

## Prerequisites & Build

1. Build native mobile Go libraries and native app binaries from repository root:
   ```bash
   TMPDIR=/private/tmp bash ./bindings/scripts/build-mobile.sh
   (cd app/expo/ios && TMPDIR=/private/tmp pod install)
   TMPDIR=/private/tmp xcodebuild -workspace app/expo/ios/UniClipboardUIShell.xcworkspace \
     -scheme UniClipboardUIShell -configuration Release -sdk iphonesimulator \
     -destination 'id=A1B5F28E-5DB7-41BE-A80A-DFEB6608EDA2' \
     -derivedDataPath /private/tmp/unic-ios-derived build
   (cd app/expo/android && TMPDIR=/private/tmp ./gradlew :app:assembleDebug)
   ```
2. Install JS dependencies for the E2E suite:
   ```bash
   cd e2e/mobile
   bun install
   ```

## Running

```bash
cd e2e/mobile

# Preflight device & build check
E2E_PLATFORM=ios ./node_modules/.bin/e2e doctor

# Run on booted iOS Simulator
bun run test:ios

# Run on booted/connected Android Emulator (debug APK with run-as)
bun run test:android
```

Optional environment overrides:
- `E2E_PLATFORM`: `ios` (default), `android`, or `both`
- `E2E_IOS_DEVICE`: simulator name or UDID (defaults to `A1B5F28E-5DB7-41BE-A80A-DFEB6608EDA2`)
- `E2E_IOS_APP`: path to the built `UniClipboardUIShell.app`
- `E2E_ANDROID_DEVICE`: Android emulator serial (e.g. `emulator-5554`)
- `E2E_ANDROID_APP`: path to `app-debug.apk`
