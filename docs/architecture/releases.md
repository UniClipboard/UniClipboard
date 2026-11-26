---
paths:
  - .gitattributes
  - release
  - tools/package
  - packaging
  - e2e/packaging
  - app/desktop/internal/appicons
  - app/desktop/boardui/assets
  - app/desktop/internal/appupdates
  - internal/desktopupdate
  - app/desktop/e2e/updateprobe
  - internal/localconfig
  - app/desktop/cmd/unic-desktop
  - tools/release
  - .github/release-notes
  - internal/releaseinfo
  - app/service/cmd/unic
  - internal/localpaths
  - internal/v1migration
  - e2e/release
  - .github/workflows/build-cli.yml
  - .github/workflows/build-desktop.yml
  - .github/workflows/build-desktop-test.yml
  - .github/workflows/package-acceptance.yml
  - .github/workflows/pr-checks.yml
  - .github/workflows/release-desktop.yml
  - .github/workflows/record-desktop-acceptance.yml
  - .github/workflows/prepare-legacy-desktop-alpha.yml
  - .github/workflows/native-desktop-acceptance.yml
  - app/desktop/e2e/run.py
  - .github/workflows/sandbox-acceptance.yml
  - .github/workflows/release-cli.yml
  - .github/workflows/dispatch-package-releases.yml
  - .github/actions/package-release
  - .github/workflows/release-aur.yml
  - .github/workflows/release-chocolatey.yml
  - .github/workflows/release-copr.yml
  - .github/workflows/release-flatpak.yml
  - .github/workflows/release-homebrew-cask.yml
  - .github/workflows/release-homebrew-tap.yml
  - .github/workflows/release-nix.yml
  - .github/workflows/release-pacman.yml
  - .github/workflows/release-scoop.yml
  - .github/workflows/release-snap.yml
  - .github/workflows/release-winget.yml
  - .github/workflows/promote.yml
  - .github/workflows/check-expo.yml
  - .github/workflows/release-android.yml
  - .github/workflows/release-ios.yml
  - .github/workflows/mirror-gitcode.yml
  - .github/workflows/prepare-release.yml
  - .github/workflows/release-on-merge.yml
  - .github/workflows/release-checks.yml
  - .github/workflows/repo-docs-check.yml
  - .github/workflows/check-nix.yml
  - .github/prompts/prepare-release-changelog.codex.md
  - tools/ci
  - app/expo/app.config.js
  - app/expo/plugins/withUnsignedAndroidRelease.js
verified: 89cc0fa79bc387107148c1780dc7684217cacfe7
---
# Product builds and CLI releases

## Current implementation

`release/products.toml` owns the standalone product metadata and declares the joint build targets and Desktop update key. CLI and Desktop have separate build and delivery controllers. A joint workflow coordinates both products. Android and iOS have independent candidate workflows and metadata.

Use the [product manifest](../../release/products.toml) for the build numbers configured at this revision. Use [npm dist-tags](https://www.npmjs.com/package/@uniclipboard/cli) for CLI channel availability and [GitHub Releases](https://github.com/UniClipboard/UniClipboard/releases) for public Desktop preview installers. A checked-in build number is a build input. A joint release can allocate a higher number.

### Mobile candidates

**Mobile build and acceptance** runs the mobile binding and Engine boundary
checks, transitively native Release builds, and the live Engine Maestro suite.
It saves native receipts separately from the Expo bundle verification.

The **Android release** workflow builds a signed `arm64-v8a` production APK
(zipaligned and signed with `apksigner` using APK Signature Scheme v3 key
rotation from the legacy Expo `debug.keystore` to `ANDROID_UPLOAD_KEYSTORE`
with `--rotation-min-sdk-version 28` and rollback disabled on the legacy key,
validated with `aapt2`, `apksigner verify`, and `apksigner lineage`) and
optionally builds a dual-ABI (`arm64-v8a,x86_64`) Google Play AAB validated
with `bundletool`. Android v3 Settings → About includes an in-app updater that
checks `/v1/android/{stable,alpha}.json`, verifies downloaded APK SHA-256
digests natively, and launches the system installer via `FileProvider`. When
`publish` is enabled, a separate `Production` delivery job stages an immutable
GitHub Release (`android/v<version>-build.<build>`, prerelease for `alpha`),
uploads the APK and `candidate.json` to R2 with create-only conditional writes
under `artifacts/android/<version>/build.<build>/`, registers the numbered
build in FlareRelease, optionally promotes `android/alpha` or `android/stable`
with public `/v1/android/<channel>.json` read-back, and publishes the GitHub
Release. An optional `publish_play` flag uploads the signed AAB to Google Play
(`internal`, `alpha`, or `production`). The **iOS Store candidate** workflow
runs on `macos-26` (Xcode 26.6), fetches and validates the active App Store
provisioning profiles for the main app, `.Share`, and `.Keyboard` targets via
App Store Connect API (or optional static profile secrets), checks build-number
availability on App Store Connect before archiving when `publish` is enabled,
exports the signed IPA and dSYMs, and optionally uploads the IPA to App Store
Connect for TestFlight testing. TestFlight availability, submission for review,
and Store publication have their own acceptance states.

The production variant retains `app.uniclipboard.android` and
`app.uniclipboard.UniClipboard` from the preceding mobile product. The
v3 alpha candidate starts at `3.0.0 (205)` above the legacy floor `204`. `internal/mobilemigration` and
`bindings/mobile` implement read-only discovery and automatic offline import of
the preceding mobile Expo SQLite history (`uniclipboard.db` plus iOS App Group
`payloads/` and Android `clipboards/` attachments), and the production iOS app
declares `group.app.uniclipboard.UniClipboard` alongside its
`.incoming-share` group. Real Store in-place update behavior, Android upload
certificate lineage, and iOS production provisioning still require verification
before rollout.

### Prepare Release and automatic merge dispatch

Run **Prepare Release** (`.github/workflows/prepare-release.yml`) with `platform` (`desktop`, `cli`, `mobile`, `android`, or `ios`), `channel` (`alpha` or `stable`), optional canonical semver `version` (defaults to the current product version in `release/products.toml`), and optional `base_branch` (defaults to `main`). `tools/release/prepare_release.py` computes the next unused build number and previous published comparison tag, updates `release/products.toml` (preserving LF bytes and unrelated sections), initializes bilingual Markdown changelog skeletons under `release/changelogs/<platform>/v<version>-build.<number>.md` (and `.zh.md`), and for `desktop` initializes `release/desktop-notes.json`.

The `changelog` job uses `openai/codex-action@v1` with the explicit model `gpt-5.6-luna`, the `CODEX_API_ENDPOINT` and `CODEX_API_KEY` secrets, and `.github/prompts/prepare-release-changelog.codex.md` to generate user-facing English and Chinese changelogs (plus `release/desktop-notes.json` for `desktop`). It validates that Codex only touched the expected changelog files, replaced all placeholders, and did not introduce `<!--` markers into `release/desktop-notes.json`. The `publish` job pushes branch `release/<platform>-<channel>-v<version>-build.<number>` and opens a release PR against `base_branch`.

When a release PR from the repository is merged into the default branch, **Release on PR Merge** (`.github/workflows/release-on-merge.yml`) validates the branch name against `release/products.toml` and the finalized changelog files on the merge commit, dispatches the matching release workflow(s), and deletes the merged release branch:

- `desktop` -> `release-cli.yml` (`channel`, `version`, `expected_source=<merge_commit_sha>` — joint CLI + Desktop release)
- `cli` -> `release-cli.yml` (`channel`, `expected_source=<merge_commit_sha>` — standalone CLI release)
- `mobile` -> `release-android.yml` (`publish=true`, `track=internal` for `alpha` or `production` for `stable`) and `release-ios.yml` (`publish=true`)
- `android` -> `release-android.yml` (`publish=true`, `track=internal|production`)
- `ios` -> `release-ios.yml` (`publish=true`)

### Joint CLI and Desktop delivery

Run **Release CLI and Desktop** from the selected release source. Supply
`channel` and `version`. The entry uses `release-cli.yml` to retain the npm
trusted publishing identity. Leave `version` empty to release the CLI
independently. When `expected_source` is supplied (for example, the release PR
merge commit dispatched by `release-on-merge.yml` on `main`), the `plan` job
verifies that `expected_source` is a 40-character commit SHA that is identical
to or an ancestor of the dispatched branch HEAD, then propagates that exact
`source_sha` across planning, CLI/Desktop builds, legacy upgrade verification,
and Desktop publication/promotion (`desktop_release.py acquire --source-sha`
independently re-verifies ancestry against the workflow run HEAD). This keeps
`workflow_run.head_branch` on the default branch for npm OIDC and package
channel dispatch while preventing concurrent merges to `main` from breaking or
altering an in-flight release.

Alpha records the source SHA, selected CLI and Desktop build number in a
create-only Git tag under `joint-release/alpha/`. The planner allocates new
numbers above existing product tags, draft GitHub releases, previous
reservations and, for CLI, every published npm platform version. The configured
number is a floor for that version. A retry restores the saved plan. The plan
supplies metadata to the build jobs without editing the checkout.

The planner checks the newest eligible published CLI against all six Desktop
targets. It pins the exact compatible CLI manifest or reserves a new CLI build
from the selected source when shared source hashes differ. Recovery verifies
the pinned digest or the original source and generated metadata. Signed
Desktop construction must succeed before the workflow publishes the CLI to
npm. Desktop preparation checks the CLI lock, package integrity and shared
source again before starting native jobs. Each target also checks those inputs
during staging.
The Windows installer lifecycle fixture uses the same lock as the production
installer and changes only the fixture build number.
After npm publication, the workflow publishes the Desktop candidate, stages the
legacy upgrade after all twelve native receipts pass, and promotes Desktop
using the channel revision from the staging receipt. The verified Alpha
selection is stored under `joint-release/published/`.

After native acceptance, run **Record Desktop native acceptance** with the
exact Alpha build identity and the three platform receipts. The workflow
checks the catalogue digest and every package hash, GUI result and lifecycle
result. It stores the verified receipts under `joint-release/accepted/`.
The workflow can also register a previously published build that still
matches the public Alpha feed and its six updater hashes.

Stable chooses the highest accepted Desktop build for the requested version.
It pins that selection for the run and checks the public origin receipt,
GitHub attachments, native receipts and exact published npm integrity before
promotion. It fails when there is no accepted candidate. The workflow promotes
the existing npm versions to `latest` and uses the original Desktop catalogue,
release notes and signed assets to advance the Stable feed. It verifies both
channels after changing them. Desktop promotion uses a channel revision and
rejects a downgrade.

The same Actions run displays CLI construction and publication, Desktop
construction and publication, legacy acceptance and Desktop promotion.
Its artifacts hold the exact Desktop CLI lock and the publication receipts.
Re-run failed jobs to reuse the successful build jobs and their artifacts.
Recovering a CLI candidate skips CLI construction and still starts Desktop
construction. The final joint result requires successful CLI publication and
successful Desktop channel verification.

### Package-manager distribution

After a successful joint release on the default branch, `dispatch-package-releases.yml` dispatches eleven independent workflows. The workflows use the completed run ID and attempt, the verified promotion and npm receipts, and the exact GitHub release asset hashes. Standalone CLI publication stays in the npm path.

Alpha runs prepare definitions and build preview packages. COPR submits to its separate Alpha project. Stable runs can publish or submit after rechecking the current Stable selection and native acceptance. Each workflow can be retried independently against the same joint run. Registry review and public package availability have separate outcomes.

The publication destinations are the project Homebrew Tap, Homebrew Cask, Scoop Extras, Chocolatey, WinGet, the project-owned AUR source package, the signed x86_64 pacman repository, COPR and the Snap Store. Nix and Flatpak publish versioned first-party assets through GitHub Releases. See the [delivery runbook](../exec/0018-desktop-release-runbook.md#package-manager-distribution) for credentials, retry inputs and first-party installation.

### GitCode release mirroring

GitHub Releases and R2 remain authoritative. `.github/workflows/mirror-gitcode.yml` mirrors the FlareRelease-registered artifacts of a published Desktop (`desktop/v<version>-build.<build>`, six updater archives) or Android (`android/v<version>-build.<build>`, production APK) release to GitCode and registers each verified copy with FlareRelease (`PUT /api/mirrors`) so mainland China downloads can redirect to it.

`tools/release/gitcode_mirror.py prepare` validates that the GitHub Release is published (not draft), verifies the immutable `release-manifest.json` (Desktop) or `candidate.json` (Android) metadata asset and cross-checks artifact sizes and SHA-256 digests against the GitHub Release assets without downloading the installers onto the runner. Because `mkdir700/gitcode-release-mirror` (pinned at `5055ef17b7302e27ca53cbe5f6e44722f7e55f6b`, `v1.0.0`) forbids `/` in GitCode tags, the workflow maps `desktop/v<ver>-build.<n>` and `android/v<ver>-build.<n>` to `desktop-v<ver>-build.<n>` and `android-v<ver>-build.<n>`.

The Action runs inside the main-only `mirror` environment over SSH to the Shanghai relay host, which fetches each artifact from R2 (`https://release.uniclipboard.app/artifacts/<product>/<ver>/build.<n>/<file>`), verifies its SHA-256, uploads or reuses identical GitCode assets without overwriting, and reads them back anonymously. `tools/release/gitcode_mirror.py register` then runs on the GitHub runner so FlareRelease Access credentials never leave GitHub, sending `product`, `version`, `buildNumber`, `filename`, `provider: "gitcode"`, `downloadUrl`, `size`, `sha256` and `source` only for receipt entries with status `mirrored` or `reused`, exact digest/size match, and a plain HTTPS GitCode/AtomGit URL. Joint Desktop releases dispatch `mirror-gitcode.yml` on the default branch from `dispatch-package-releases.yml`; standalone `release-desktop.yml` and `release-android.yml` publications dispatch it on the default branch after GitHub Release publication; human-published releases re-dispatch via `redispatch-from-release`. Live end-to-end mirroring for numbered builds requires FlareRelease's `PUT /api/mirrors` endpoint to accept `buildNumber` and the Shanghai host allow-list (`/etc/gitcode-release-mirror/config.json`) to admit `.update.tar.gz` (and `.apk` for Android). See the [delivery runbook](../exec/0018-desktop-release-runbook.md#gitcode-release-mirroring).

`go run ./tools/package -product cli` builds `unic` and `unicd` for the current target. The versioned archive contains those programs and the `uniclip` alias. Unix archives preserve the relative symlink. Windows archives provide `uniclip.cmd`. Development packaging through `make package-headless` and `make package-desktop` remains available and unsigned.

All runtime programs handle `--version` and `--version --json` before initialization. Release builds embed version, build number and source SHA. Development builds report `dev` unless packaging supplies metadata.

CLI assembly uses the existing npm main/platform-package model. `@uniclipboard/cli` pins every optional platform package to the exact `<version>-build.<number>` registry version. Both npm CLI command names use one launcher. The package also exposes `unicd` through a dedicated service entrypoint. The launcher executes the real CLI beside its sibling service. Alpha selects the npm `alpha` dist-tag. Stable selects `latest`. Channel names do not enter package versions.

The declared CLI matrix covers macOS arm64/amd64, Linux arm64/amd64 and Windows arm64/amd64. The Windows arm64 target passed native hosted executable acceptance in distribution run 43408274394. Each target runs its own executable identity acceptance. Linux amd64 also runs production headless and packaged release/upgrade process acceptance. Remote native acceptance passed for all five targets in run 42777109814. The [remote verification receipt](../exec/0018-cli-remote-verification.md) records exact source, artifacts and the observed headless retry.

## Desktop staging and package definitions

### Single-target device-test packages

The registered `Build CLI` dispatch accepts `test_target` and `test_format`. Select one of the six native OS/architecture targets; `none` preserves the normal CLI path. `default` selects macOS DMG, Windows setup EXE or Linux DEB. Explicit formats use the existing Desktop format constructor. Target/format mismatches and combinations with distribution, desktop-only, sandbox or native-window requests fail before the native runner starts.

`build-desktop-test.yml` runs one selected native package job, not the six-target release matrix. `tools/package/desktop_test.py` builds the current checkout's CLI, runs native executable/alias smoke, assembles a single-target npm candidate in scratch space, then locks that exact manifest into a development Desktop stage. The existing stager checks native identity and private CLI/service configuration IPC. Only the selected format and its input/hash receipts enter the downloadable `desktop-test-<target>` artifact, retained for seven days.

Checked-in versions, CLI release locks and updater keys remain unchanged. Temporary metadata disables automatic updates, and child build processes do not receive production signing or ingestion configuration. The downloadable file uses a `test-<source>-` prefix; its manifest preserves actual unsigned/not-notarized states. The workflow has read-only repository permissions and no release credentials. It does not publish npm, create a GitHub Release, upload R2 objects or change a channel. These packages retain the existing installation identity; `--development` does not make an installation parallel-safe. `TESTING.md` requires a dedicated machine/account for installer tests; a custom path does not isolate an existing Windows registration. Unpacked archives may use separate folders. Every test launch uses an explicit test `UNIC_HOME`.

The bundle contains one native package, `test-package.json`, `artifacts.json`, exact CLI/stage manifests, `TESTING.md` and a complete `SHA256SUMS`. `desktop_test.py verify` rejects modified bytes, symlinks and extra/missing files. Native version/IPC acceptance is separate from installation and GUI acceptance. See the [single-target receipt and replay](../exec/0018-single-target-test-package.md).

### Production staging

`go run ./tools/package -product desktop -target <os>-<arch> -cli-manifest <file> -output <directory>` delegates production staging to `tools/package/desktop.py`.
The metadata must lock the exact CLI manifest digest, version and build. The stager checks the selected npm package SHA-256/SHA-512, native acceptance fields and both executable hashes before building Desktop.
It copies accepted CLI/service files unchanged. It rejects moving source, changed inputs and existing output directories.
`--development` permits a dirty construction candidate and records that state; it does not establish a release or a separate installation identity.
Use isolated homes for all construction candidates. Version-`dev` executables use the separate development identity and default data directory.

Desktop packaging uses the platform assets in `app/desktop/internal/appicons`. macOS declares the multi-resolution ICNS through `CFBundleIconFile`. Windows executables embed the multi-resolution ICO and PE `VERSIONINFO` metadata (`Unic` description/product name and numeric version from `release/products.toml`) through architecture-specific `.syso` resources, and the NSIS installer uses the same ICO. Linux native packages and the source AUR recipe install scalable SVG and 48- and 512-pixel PNG icons under the hicolor theme. AppImage carries the 512-pixel PNG at its root. The stage manifest digests each staged icon. See the [asset guide](../../app/desktop/internal/appicons/README.md) for regeneration.

Desktop uses `Unic` in its application UI and launcher metadata. macOS localizes the existing `UniClipboard.app` basename to `Unic`. Linux changes the desktop entry `Name` field and keeps its existing filename, executable and icon keys. Windows NSIS writes the `Unic` uninstall display name to the existing registry key and replaces the previous Start menu entry after it creates the `Unic` shortcut. Windows in-app updates replace the private payload. Running the installer refreshes its Windows shell registration and Start menu entry.

Every Desktop stage also retains the root AGPL license and `licenses/BOARDUI-MIT.txt`, `licenses/Inter-OFL.txt` and `licenses/REMIX-LICENSE.txt`. macOS stores them in the application Resources directory; Linux and Windows store them with the private payload. The normal immutable stage file manifest hashes these notice bytes, and the format constructors preserve them. A local unsigned macOS application-archive replay verifies the actual archive and native identity/private-service smoke. It does not install or open a GUI.

The format constructor supplies DEB, RPM, pacman packages, NSIS setup EXE, portable ZIP, macOS DMG/application archive and AppImage/update archive entrypoints.
macOS DMG construction (`tools/package/macos_dmg.py`) emits a `ULFO` (LZFSE) disk image with `UniClipboard.app` copied via `ditto`, an `Applications` symlink, a volume icon (`app/desktop/internal/appicons/icon.icns`), the checked-in text-free BoardUI Retina background (`packaging/macos/dmg-background.tiff`, regenerated via `tools/package/macos-dmg-assets.py`), and a programmatically generated `.DS_Store` without Finder GUI/AppleScript automation.
The writable staging image is hidden from Finder browsing. The constructor detaches it by the device identifier returned at attachment and reports the native error if detachment fails.
Only tested construction/lifecycle states count as acceptance; see the [package delivery receipt](../exec/0018-v2-package-channel-receipt.md).
Linux uses the pinned mygo GTK3 native-view path and a tray indicator library. These recipes do not inherit v1's GTK4/Wails/WebKitGTK dependency set.
AppImage currently uses host GTK3 libraries; self-contained runtime delivery remains open.

Desktop packages keep `unic-desktop`, `unic` and `unicd` together in a private directory. Linux exposes only `uniclipboard` as the graphical launcher, preserving independently installed CLI commands.
`unic-desktop` supplies the launcher, tray, GUI and quick-panel roles from one executable. Each running role has its own OS process. Internal `--role` arguments do not add a distributable executable or change the immutable CLI lock. The macOS `CFBundleExecutable`, Windows Start-menu and Scoop shortcuts, and Linux desktop entry keep using the launcher without a role argument. Login startup also enters this launcher and reads the saved desktop mode.
The tray owns login registration and application updates. Its application name and installation identity remain stable across roles and profiles; profile-scoped activation uses Desktop IPC rather than a different native application name. The macOS bundle declares `LSUIElement` so the launcher, tray, quick panel and bundled service start without a Dock icon. The GUI switches to regular activation as it opens its window and returns to accessory activation during graceful quit. Explicit application quit and updater relaunch must stop the owned GUI and quick-panel processes before retiring the tray; if an earlier version left a moved-aside or replaced view role holding the profile endpoint, `tray` classifies it as `ErrStaleBuild` and retires only that OS-verified previous-build process before relaunching or shutting down. The Windows NSIS installer (`packaging/windows/installer.nsi`) uses MUI2 with automatic English, Simplified Chinese and Traditional Chinese language selection. Before installing or uninstalling interactively, it checks managed executables in the target install directory and prompts the user to stop those path-scoped running processes (including leftover `.old-<pid>` updater executables) and continue (retrying or canceling if locks remain), while silent (`/S`) and legacy v1 update paths remain non-interactive. Interactive installation finishes with a checked-by-default option to launch `unic-desktop.exe`; silent installs skip GUI pages and legacy v1 updates retain their explicit restart path. Native process and login acceptance remain separate from package construction.
macOS bundles retain `app.uniclipboard.desktop`. The Windows installer retains the v1 per-user uninstall registration, checks or stops running owned executables in `$INSTDIR` before replacement or removal, and preserves user data.
Actual v1 installer replacement and GUI behavior of a formally installed package remain unverified. Source-built Windows ARM64 native role/lifecycle acceptance is recorded separately in the [process receipt](../exec/desktop-process-lifecycle.md).

`tools/package/channels.py` renders twelve channel definitions from exact local artifact hashes. Mixed builds, missing inputs, unsafe filenames and modified files fail.
The self-managed Tap retains both CLI Formula and Desktop Cask roles. AUR includes a source `-git` recipe and a generated binary recipe; their provenance remains distinct.
The renderer records definition hashes and pending publication states. Separate channel workflows consume its output and own submission, signing and publication.
`tools/package/pacman.py` requires an exact package lifecycle receipt, clean production provenance and an explicit signing key environment before repository signing.
The lifecycle attachment entrypoint verifies the package digest and required observations before updating acceptance metadata.

`UNIC_MACOS_SIGNING_IDENTITY` and `UNIC_WINDOWS_CERT_SHA1` let the CLI builder sign executable files before archive hashing and native acceptance.
Desktop signing first verifies distribution signatures on the locked CLI files and signs only Desktop-owned bytes. Real certificate signing is unverified on this host.
The Desktop lock selects CLI `2.0.0 (2)` from canonical distribution run [45030337326](https://github.com/UniClipboard/UniClipboard/actions/runs/45030337326) by manifest SHA-256 `19a091d691c8d1ad5ea7bdfb12be39a2e6a2235756a919b7c9f415c4719dd8ec`. Both macOS targets have distribution signatures. All six targets passed the shared configuration, API and service-lifecycle source checks against Desktop. The [previous lock rejection receipt](../exec/baselines/desktop-cli-lock-20261120/receipt.json) records why the earlier candidate was replaced. `fetch-cli --allow-ci` verifies the canonical workflow, clean source, complete target matrix, package digests and macOS signing before acquiring the candidate. The default fetch path requires public immutable release assets.
`build-desktop.yml` verifies the exact CLI lock against the selected source before it starts the six native build jobs. A target failure cancels queued and running targets. The workflow requires all six targets to succeed. Explicit distribution mode imports an ephemeral Apple keychain, signs the Desktop payload, submits a copied bundle to Apple notarization and staples the accepted ticket before final archive hashing, and signs, notarizes and staples the `ULFO` DMG container (`--notarize-container`). Credentials stay in the ephemeral keychain. Cleanup runs after failure as well as success.
Native package smoke keeps version inspection state-free, then explicitly initializes its isolated CLI profile with `unic migrate --initialize-empty --json` before shared-configuration IPC. Discovery HOME, XDG and Windows profile paths remain private. Portable and explicit data directories initialize independently; there is no gate bypass or direct receipt write. Disposable DEB lifecycle acceptance initializes before its first business command and verifies that the receipt survives upgrade and removal.

The primary PR Desktop suite uses the same Ubuntu 24.04 and native GTK 3/indicator runtime baseline as the native Linux acceptance workflow. It installs display/recording tools without the webview-only WebKitGTK stack. The complete canonical suite is partitioned into three disjoint round-robin shards on independent hosted runners. All shards must pass, fail-fast is disabled, and each keeps the thirty-minute job budget. The canonical runner records a separate evidence artifact for each shard; unselected cases remain NOT_RUN in that shard.

`package-acceptance.yml` runs six-target nonvisual package and signed-update process acceptance, plus disposable-container DEB lifecycle checks. Exact observed results are recorded in the package receipt. These build workflows do not publish.

### Native application updates

The Desktop host uses the pinned mygo native updater plugin. `internal/desktopupdate` converts the numbered FlareRelease feed into mygo manifests. The app version includes `-build.<number>` so later builds of one product version compare numerically. The adapter validates the target, archive format, bounded size, SHA-256, raw Ed25519 signature over the archive digest and the signed embedded `release-identity.json`. mygo owns extraction and installation. On macOS, Desktop asks unicd to select the release, fetch the archive and verify its complete signed identity. Desktop stages and revalidates the transferred file with its embedded public key before stopping the owned service. The tray updater keeps scheduling, preferences, user confirmation and restart coordination. Other platforms retrieve and verify the archive in the desktop updater.

`desktop.update_channel` accepts `stable` (default) or `alpha` and takes effect on the next check. Updater preferences use the v2 data directory. `release/products.toml` selects the durable `update_public_key`; an empty key disables in-app updates. The production private key must use mygo's raw Ed25519 format, independent of existing Minisign/Tauri keys. `go run ./tools/release/update-sign` generates a private key directory or signs archives from `UNIC_UPDATE_PRIVATE_KEY` without logging the private key.

Each update check selects the release notes using the current `desktop.language`. The adapter sends the Chinese section to mygo for Chinese when the feed includes a translation. Other languages and missing Chinese translations use the English section.

The adapter sends the installed identity as `from=<version>-build.<number>` to the numbered Desktop feed. FlareRelease can return the selected build and up to four intervening, published builds from the selected channel in newest-first order. The adapter accepts single-build notes from the feed when there is no cumulative history.

The host enables automatic/native-menu updates only for direct installations. System Linux packages, Snap, Flatpak, AppImage, Homebrew-owned bundles, Scoop and portable archives remain externally managed. NSIS records winget/Chocolatey ownership from `/MANAGED=`. Manual reinstalls preserve that receipt. Windows and Linux updates use mygo's owned-file list and preserve unowned data. AppImage self-update remains unsupported by the pinned SDK. Public feed/key provisioning remains a deployment prerequisite.

The registered CLI build entry can also call `native-desktop-acceptance.yml` to run the canonical history/search/copy native-window case on all six targets. Its screenshots, continuous recording, source/binary hashes and cleanup receipt remain distinct from production installer and real-system clipboard acceptance. Windows arm64 records the pinned Go toolchain's unavailable race detector explicitly.

### Desktop delivery controller

`tools/release/desktop_release.py` acquires six-target candidates only from a successful canonical repository build, checks package/source provenance and assembles the complete 22-artifact catalogue. Production assembly requires native executable identity, macOS code signatures, Apple notarization and native updater signatures. Windows payloads and installers may be unsigned Authenticode for both alpha and stable. Their `signed`, `payload_signed` and `container_signed` fields remain false; release pages explicitly declare unsigned Windows. Test certificates never establish production signing. Raw Ed25519 updater signatures remain mandatory for every updater archive. The publication command also verifies npm availability of the locked CLI set.

The controller uses the maintained boto3 SDK with create-only conditional R2 writes and byte-for-byte read-back. It checks all existing objects before the first write. Four workers handle independent object checks, missing object uploads and public origin read-backs. It reuses objects whose bytes passed preflight. The catalogue and successful publication receipt remain immutable objects. It checks the deployed FlareRelease contract, authenticates with Cloudflare Access service credentials and registers the six updater archives through the existing API. The Production environment pins the R2 endpoint. When dedicated S3 credentials are absent, the controller verifies the existing Cloudflare token and derives the SigV4 key in memory for the configured account’s default-jurisdiction endpoint. The read-only preflight compares the full bytes of a known v1 object with its accepted hash and checks both numbered contracts. The current 512 MiB registration limit applies to this updater set. All other formats remain downloadable immutable objects. Registration does not change a channel.

Promotion consumes the immutable availability receipt, requires an explicit channel revision and verifies public numbered identity/updater hashes after the API's compare-and-swap mutation. Stable requires one native acceptance receipt for each of macOS, Linux and Windows. Each receipt binds the catalogue SHA-256 and records passing GUI and lifecycle checks against the exact hash of every artifact for that platform. Stable also verifies that all seven packages in the locked CLI manifest have matching npm integrity and that each npm `latest` tag selects the locked version. `release-desktop.yml` defines separate manual read-only preflight, publication and promotion operations. The joint workflow calls publication and promotion after their product gates. `release-cli.yml` can consume an accepted canonical run without rebuilding the locked CLI bytes.

### GitHub Desktop Release pages

`tools/release/github_desktop.py` adapts the v1 Release template to independent numbered Desktop builds. The template is `.github/release-notes/desktop.md.tmpl`. It renders changelog/source links, all supported native installer downloads, exact locked standalone CLI downloads, verification instructions and optional GitHub-generated new contributors. English and Chinese changelog strings remain available in `desktop-notes.json`. Mobile download claims remain outside this Desktop page.

The tag is `desktop/v<version>-build.<number>` and points to the accepted candidate source, independently of the CLI tag. Draft staging attaches the 22 native files, six raw updater signatures, the immutable catalogue, bilingual notes, reproducible page context and `SHA256SUMS.txt` (32 attachments). Existing bytes and tag identity must match. Missing draft assets can resume. Extra or changed assets fail. Every new upload is downloaded and hashed before accepting staging. Later staging and publication compare GitHub's SHA-256 metadata with the accepted catalogue and page files. An asset without a digest still requires full-byte read-back. Published releases cannot repair missing assets or downgrade from stable to alpha.

The existing delivery workflow supports `github-draft`, `publish` and `promote`. It stages or verifies the page before origin/channel mutation. Alpha pages become public prereleases only after origin publication read-back. Stable publication leaves a draft until verified stable promotion, then marks the page as the latest Desktop release. The page context restores the original comparison tag/contributors across workflows. Local `preview --development` produces reviewable files without creating a page; remote operations retain all production signing gates.

The CLI and Desktop controllers search the paginated GitHub release list by exact tag to find both drafts and published releases.

`e2e/packaging/github_release.py` drives the real GitHub CLI and release subprocess against disposable loopback HTTP endpoints. Its report and checksums establish process/transport behavior only. It does not create a real draft or public GitHub Release.

`sandbox-acceptance.yml` runs actual Nix, Flatpak and strict Snap construction/execution on both Linux architectures. The harness overrides source acquisition with the exact verified pre-publication DEB; generated runtime/build steps remain unchanged. Missing runtimes, namespace failures or confinement failures fail the job. Nix sets only the Desktop ELF interpreter and supplies GTK libraries through the existing wrapper; it does not add RUNPATH to the prebuilt Go binary. The locked private CLI pair remains unchanged. The process harness verifies real private CLI/service IPC and bounded shutdown inside each environment. These nonvisual process checks do not establish clipboard, file/network, refresh or sandbox GUI acceptance.

## Desktop alpha upgrade from 1.6.2 and 1.6.2-alpha.2

FlareRelease can offer the selected 2.0 alpha build to Desktop
`1.6.2` and `1.6.2-alpha.2` when the old client requests the Alpha channel.
Stable requests retain the stable selection. The offer uses separate
Minisign signatures and six exact GitHub prerelease assets. The numbered native
feed continues to use raw Ed25519 signatures.

The signed macOS bundle retains the old `Contents/MacOS/gui-go` launch path
and accepts the old restart flags through that path. The Windows installer
accepts the old updater's passive invocation for an existing
`1.6.2` or `1.6.2-alpha.2` installation. It waits for the old files to become writable
before replacement and relaunches without passing the old flags. On Linux, the
legacy updater replaces an AppImage in place. The package manager continues
to own DEB and RPM upgrades.

Run **Prepare legacy Desktop alpha** after publishing and registering the
candidate. It binds the offer to the authenticated registration digest.
It downloads the exact release assets, matches the GitHub catalogue to the
origin publication receipt, and signs the files with the pinned v1 signer.
The workflow tests both source versions on all six targets. The native jobs
verify the old and new signatures, replace an isolated installation, verify
the restart entry and explicitly migrate synthetic clipboard history.

Enable the workflow's optional `stage` input to verify the source version,
target, candidate hash, and registration digest in all twelve upgrade receipts.
The staging job verifies the ready origin registration and writes the offer
using a create-only R2 operation at
`_control/desktop-legacy-alpha/<manifestDigest>.json` in the release bucket.
Its receipt records the offer hash and the observed alpha channel revision.
The numbered alpha promotion activates that offer. Staging leaves the
current offer available. Roll back the numbered alpha selection to restore
the previous offer, or remove the selected offer object to stop new offers.

## v1 upgrade notice and migration

The CLI checks known v1 paths only when stderr is a terminal and `--json` is absent. It performs directory stat calls; it does not open credentials or a database. A default v1 directory or the active `UC_PROFILE` directory triggers the warning. Missing paths and regular files remain silent. Custom and portable profiles outside these locations require explicit `--source` selection.

The warning explains v1 incompatibility and points to migration help, preflight and import commands. It does not request acknowledgement or migrate automatically. `unic migrate` and `unic migrate --help` print a read-only guide before opening any profile.

CLI archives and npm platform packages include the migration importer within `unic`. Explicit migration runs in-process without starting the service. The importer keeps source data unchanged, verifies reopened target content and resumes by record identity. Partial migration returns 3. The importer fills unset display name, theme, anonymous usage-event, and tag presentation settings. All peers must use v2 and pair again with new device identities and trust relationships. See the [preference contract](../contracts/history-contract.md#v1-preference-preservation).

The importer encodes Windows drive paths as absolute SQLite file URIs (`file:///C:/...`), retaining read-only/query-only access. A drive letter must not become the URI authority. The [unsigned Windows receipt](../exec/0018-unsigned-windows-acceptance.md) records the native replacement and frozen-history acceptance boundary.

The packaged-process E2E uses a temporary home, pseudoterminals and fixed encrypted Rust fixtures. It checks fresh-install silence, directory detection, JSON output, offline help, read-only preflight, partial results, source hashes and idempotent repair/retry. It never reads a personal v1 profile or system credential.

## Build, publication and promotion

Git attributes require LF checkout bytes for release TOML metadata on every platform. Metadata digests cover the original file bytes. The real Git checkout regression enables `core.autocrlf=true` and verifies that checkout preserves the canonical metadata bytes before building candidates.

`build-cli.yml` runs on scoped CLI/core/release-tooling PRs and main pushes, as well as unfiltered manual requests and reusable calls. PR candidates use the actual PR head rather than a synthetic merge SHA. Explicit distribution builds import the existing Apple certificate into a temporary keychain, sign CLI executables before hashing and remove the keychain after construction. Its callers provide an exact source SHA. It checks the metadata digest, source identity, required target set and native executable receipts before `npm pack`. CI builds do not publish.

### PR check trigger boundaries

Every PR-triggered workflow declares explicit path filters so unrelated changes (such as documentation updates or release-preparation PRs touching only `release/**`) do not trigger heavy cross-platform builds:

| Workflow | File | Trigger scope |
|---|---|---|
| PR checks | `.github/workflows/pr-checks.yml` | Root Go module code, Desktop E2E harness/cases, Go lint/build config and `tools/ci/**`. Its `changes` job (`tools/ci/pr_changes.py`) gates `lint`, `test`, `headless` and `desktop-e2e` independently; `merge_group` and `workflow_dispatch` run all jobs. |
| Build CLI | `.github/workflows/build-cli.yml` | Headless CLI/service/core Go code, `e2e/headless/**` and CLI release/packaging scripts. `workflow_dispatch` and `workflow_call` stay unfiltered. |
| Package process acceptance | `.github/workflows/package-acceptance.yml` | Desktop packaging, installers, updaters, app icons/license assets, Desktop release-note template and Desktop packaging/release workflows (excluding `release/**` and mobile/prepare-release scripts). |
| Mobile build and acceptance | `.github/workflows/check-expo.yml` | `app/expo/**`, `bindings/**` and core Go packages transitively imported by `bindings/` (excluding desktop/service-only `internal/*` packages and `release/products.toml`). |
| Nix flake check | `.github/workflows/check-nix.yml` | `flake.nix`, `nix/**`, `go.mod`, `go.sum`, `packaging/linux/**`, Desktop icons/assets and workflow changes. |
| Docs Check (`docs-site`) | `.github/workflows/docs-check.yml` | `docs-site/**` and workflow changes. |
| Repo docs check | `.github/workflows/repo-docs-check.yml` | Repository Markdown docs (`docs/**`, `AGENTS.md`, `README*.md`, `CONTRIBUTING.md`, `SECURITY.md`) and `tools/docscheck/**`, running `go run ./tools/docscheck`. |
| Release checks | `.github/workflows/release-checks.yml` | `release/**`, `prepare-release` / `release-on-merge` / mobile release / `mirror-gitcode` workflows, prompt/templates and `tools/release/{prepare_release,mobile,android_release,gitcode_mirror,ios_profiles}.py`. Runs fast metadata/notes/branch validation (`tools/ci/validate_release.py`) plus `prepare_release_test.py`, `mobile_delivery_test.py` and `gitcode_mirror_test.py`. |

`tools/ci/pr_changes_test.py` verifies both the `pr-checks.yml` job classifier and the workflow `paths:` trigger matrix against representative PR file sets.

`release-cli.yml` supports alpha and direct stable publication. When `candidate_run_id` is supplied, the publish job acquires the canonical candidate directly. Otherwise the workflow checks for a durable record for the selected version/build and restores its exact package files when it exists. It awaits the reusable build when no record exists. It stages the manifest and packages in a draft GitHub release before publishing to npm. The product tag is `cli/v<version>-build.<number>` and must reference the accepted source SHA. Existing assets are compared and never overwritten. Incomplete staging fails before npm publication, and recovery requires the original candidate artifacts.

Public npm operations require clean source, GitHub Actions OIDC and npm with dist-tag OIDC support. Every package must authorize the exact release and promotion workflow identities. Packages may require an initial owner-controlled bootstrap before trusted publishing can be configured. Configuration is a deployment prerequisite, not proof supplied by local acceptance.

Publication validates every local archive and every existing registry version before the first mutation. Up to three platform npm mutations (`publish` or `dist-tag`) run concurrently, while all platform packages poll registry read-back concurrently so asynchronous registry processing does not block remaining uploads. The main package runs only after every platform's integrity and tag pass registry read-back. A tag write is required only if the tag differs from the selected build. Platform worker results are recorded in manifest order. A durable record becomes public only after a real availability receipt passes validation.

`promote.yml` currently offers CLI only. It downloads the requested durable manifest, verifies all existing npm package integrity values and moves the exact package set to `latest`. It does not rebuild, pack or upload. It retains alpha. Both mutating workflows share one CLI concurrency group.

Registry reads request cache revalidation. The controller polls for each package
version and each required tag update with a separate 15-minute deadline.
A visible package version with conflicting integrity fails immediately.

Retries preserve exact identity and reject integrity conflicts. An older build cannot replace a newer selected build. Known v1 alpha/beta/rc tags may advance only to a strictly higher product version. Unknown or same-base legacy tags require review. npm changes across packages are not atomic; a partial failure has a failed receipt and can be retried with the same candidate. External writers outside the workflow concurrency group remain an operational constraint because npm does not expose a compare-and-swap tag API.

### Alpha release timing and caches

Once the signed CLI candidate is locked, run Release CLI with `candidate_run_id` alongside the canonical Desktop distribution build. Desktop GitHub draft staging can start when the Desktop build succeeds. Desktop origin publication waits for the exact locked npm set. Legacy Alpha upgrade acceptance consumes that registered build before its Alpha channel promotion. Start timing at the first release workflow dispatch, including the CLI distribution build when it is part of that release. Stop after both the public npm Alpha tag and the public Desktop Alpha feed pass read-back. Include runner queue time, retries and required upgrade acceptance in the P95 target of under 20 minutes.

CLI assembly, publication and promotion share the pinned npm download cache. Desktop delivery uses the pip cache keyed by its requirements. Linux Desktop packaging restores downloads per target and tool lock, reusing entries for unchanged SHA-256 values. The installer checks every download against the current lock, including cache hits, and replaces corrupt entries with verified bytes. Publication still uses the accepted immutable candidates and destination read-back.

## Invariants

- Product version and build number identify immutable bytes. Channel selection is external metadata.
- A CLI candidate contains the complete declared target set before publication.
- Required packages have exact dependencies and verified SHA-256/SHA-512 identities.
- Missing, conflicting or incomplete candidates fail before a main package channel advances.
- CLI never registers with FlareRelease.
- Promotion never reconstructs accepted packages.
- Production availability requires destination read-back. A local fixture is separate evidence.
- PostHog linker values and release credentials retain the existing host/token validation.

## Code map

| Path | Responsibility |
|---|---|
| `release/products.toml` | Product metadata and CLI target matrix. |
| `internal/releaseinfo` | Early executable build identity output. |
| `app/service/cmd/unic/upgrade.go` | Known v1 directory checks and terminal notice. |
| `app/service/cmd/unic/migrate.go` | Offline help and in-process history import. |
| `internal/localpaths` | Shared v1 source location without directory creation. |
| `tools/package` | Go CLI builds/aliases, exact Desktop staging, native formats, channel definitions and repository construction. |
| `tools/release/release.py` | Assembly, native smoke, npm publish/promote and durable build records. |
| `tools/release/gitcode_mirror.py` | Published release validation, R2 mirror source assembly and FlareRelease `PUT /api/mirrors` registration for Desktop and Android. |
| `.github/workflows/build-cli.yml` | Native CLI target construction and acceptance. |
| `.github/workflows/release-cli.yml` | Candidate recovery, reusable build and npm publication. |
| `.github/workflows/mirror-gitcode.yml` | Main-only SSH relay mirror to GitCode and FlareRelease mirror registration for Desktop and Android. |
| `.github/workflows/promote.yml` | Exact npm build promotion. |
| `e2e/release` | Real Go/npm processes, disposable registry and GitHub command fixture. |

## Verification

Use Node 22.23.4 or a supported newer version, Python 3.11+ and the Go version from `go.mod`.

```sh
python3 e2e/release/run.py
cd .cache/release-e2e
shasum -a 256 -c SHA256SUMS
```

The report records source hashes, process observations and assertions. Acceptance keeps no installed package or binary archive. The GitHub command fixture verifies orchestration order and exact-byte recovery; it does not verify real GitHub API behavior.

## Gaps

Stable advancement requires the exact Desktop acceptance and publication receipts described above. Each distribution format has its own availability and acceptance evidence. Native sandbox GUI, package refresh and real-OS clipboard, file and network acceptance remain separate checks. GitCode mirror workflow, tooling and offline process tests are implemented; live end-to-end mirroring for numbered builds still requires FlareRelease `PUT /api/mirrors` `buildNumber` support and Shanghai relay host allow-list updates for `.update.tar.gz` and `.apk`.

Mobile candidate builds, simulator runs, production signing, Store upload and rollout each have their own acceptance gate. The [mobile release preparation receipt](../exec/0031-mobile-m5-receipt.md) records candidate builds and simulator observations. The [iPhone receipt](../exec/0031-mobile-m2-receipt.md#ios-physical-device-acceptance) records the physical-device scenarios tested so far.

Exact interface requirements are in the [release contract](../contracts/release-contract.md). Process ownership remains in the [headless architecture](headless.md).

## Accepted CLI acquisition

`tools/release/cli_candidate.py` acquires the exact `cli-npm-candidate` artifact from a successful canonical Build CLI run in this repository. It checks workflow identity, source SHA, product identity, the complete declared matrix, native receipts and package SHA-256/SHA-512 before changing a Desktop lock. `--require-macos-signing` rejects unsigned macOS component builds.
The optional `cli_artifact_run_id` records the immutable acquisition source for pre-publication construction. It does not establish npm availability or satisfy the stable Desktop gate.
