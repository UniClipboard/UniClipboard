# 0031 M05 mobile release preparation

## Production identity

- The preceding mobile source at [v2.4.1.204](https://github.com/UniClipboard/UniClip/tree/v2.4.1.204) declares `app.uniclipboard.UniClipboard` on iOS and `app.uniclipboard.android` on Android. The production variant keeps those identifiers and the `uniclipboard` scheme.
- The proposed candidate version is `2.5.0 (205)` for each platform. `release/products.toml` owns each platform's version and build number independently. The validator requires the build to advance beyond the observed legacy build 204.
- Expo prebuild verified the main iOS application identifier, the `.Share` target identifier, and the production incoming-share entitlement in both targets. The main iOS application uses its default Keychain access group. The production Android prebuild generated the expected application ID and an unsigned Release configuration.
- Store installation replacement, import of existing released mobile history, and access to legacy Keychain credentials require dedicated verification. No evaluated website published an Apple App Site Association document for this target. Associated Domains remain pending.

## Diagnostics

Each mobile Engine has the shared bounded observability controller. Settings
> Diagnostic logs toggles upload and shares up to 64 already-redacted JSON
records from the current process. The toggle defaults on and a closed
preference survives restart. Release builds accept the ingestion-only token
for the exact selected PostHog Cloud endpoint. Without deployment metadata,
local log collection remains available and the UI reports upload as
unavailable.

The first Android native run reproduced a process crash after Setup. The
Android seccomp sandbox rejected syscall 452 while Go tried to change log
directory permissions. On Android the local logger sets `0700` permissions
on the already opened directory descriptor after checking that it points to
the expected directory.

## Local verification

- The Engine dependency boundary, mobile binding race suite, local-only
  commit E2E, diagnostic process E2E, and Expo bundle verification passed.
  The diagnostic processes verified defaults, disabling across restart,
  re-enabling, and redacted local JSON Lines export.
- The iOS 26.3 Simulator and Android 16 emulator passed the live setup,
  history and search, and devices and diagnostics flows with Maestro 2.10.0.
  The device page assertions cover its local identity and join controls.
- The production Android AAB passed bundletool validation for `arm64-v8a`
  and `x86_64`. Its manifest reports `app.uniclipboard.android`, version
  `2.5.0` and build `205`. A disposable self-signed key verified the local
  signing and verification path.
- The first concurrent native runs shared an output directory. Separate
  platform and process identifiers now isolate the receipts. The iOS retry
  also uses the native Search tab selector. The first Android Release rebuild
  after the seccomp fix failed in Android lint while the production build
  shared its generated dependency directories. The sequential rebuild passed.
- `TestRustFixtureMigrationResumeAndSourceIndependence` failed to open the
  default file cache in both the M05 checkout and its unmodified M04 base
  (`9caa0a575`). The failure is tracked separately from the M05 checks.

## Candidate and upload workflows

Native CI has separate Engine, Expo bundle and platform-specific live UI
jobs. The live suite runs on explicitly selected disposable simulators. Its
receipts include the artifact hash, flow hashes, JUnit and screenshots.
The first [hosted Android job](https://github.com/UniClipboard/UniClipboard/actions/runs/45530292519/job/138923618540)
stopped before compilation because the Ubuntu runner had no Android SDK
command-line tools on its PATH. The Android acceptance and Store candidate
workflows install the required SDK packages explicitly.

Two [hosted](https://github.com/UniClipboard/UniClipboard/actions/runs/45530764536/job/138925182817)
[Android runs](https://github.com/UniClipboard/UniClipboard/actions/runs/45531613917/job/138928043313)
built the APK and lost the ADB transport during the first Maestro launch.
The Android job now builds before starting the emulator, retains emulator
logs, and retries the suite once only after the recorded transport failure
and a stable connection. Each attempt writes a separate receipt.

The first [hosted iOS run](https://github.com/UniClipboard/UniClipboard/actions/runs/45530292519/job/138923618609)
passed setup on iPhone Duo and failed its clipboard history assertion. The
same local live suite passed on an iPhone 17 Pro simulator with iOS 27.0.
CI now creates that simulator explicitly. The suite checks the pasteboard
fixture before starting the history flow. The
[next iOS job](https://github.com/UniClipboard/UniClipboard/actions/runs/45534358566/job/138937188768)
stopped during a lost release-asset connection. Both Maestro downloads now
use bounded retries and retain the SHA-256 check.

The iOS workflows use the [GitHub-hosted Xcode 27 image](https://github.com/actions/runner-images/blob/main/images/macos/xcode-27-arm64-Readme.md).

The `mobile-ios` environment supplies the Apple Distribution PKCS#12 file,
its password, and distinct App Store profiles for the app and share target.
The `mobile-android` environment supplies the Android upload keystore and
its associated passwords and alias. The workflows produce versioned candidate
receipts after signing. They upload to the respective store only when the
manual `publish` input is enabled. The optional upload steps use the iOS
App Store Connect API key or Android Play service account in the corresponding
environment. A production Android upload also enables the GitHub checksum
receipt step. See [mobile release construction](../architecture/releases.md#mobile-candidates).

Production signing, the hosted jobs, and Store upload were not executed in
this local run. App Store Connect processing and Store availability require
their own evidence. The M04 physical-device gate and 0031 decision archival
remain open.
