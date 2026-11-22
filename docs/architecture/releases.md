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
  - .github/workflows/native-desktop-acceptance.yml
  - app/desktop/e2e/run.py
  - .github/workflows/sandbox-acceptance.yml
  - .github/workflows/release-cli.yml
  - .github/workflows/promote.yml
verified: 513b19ec8aff35c046ddb8295d438192ece097c7
---
# Product builds and CLI releases

## Current implementation

`release/products.toml` owns product versions and build numbers. CLI and Desktop have separate build and delivery controllers. Public delivery is a separate accepted operation. Desktop selects `2.0.0 (1)` for the first v2 candidate. Android and iOS records reserve independent metadata. These records do not establish release support.

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

Desktop packaging uses the platform assets in `app/desktop/internal/appicons`. macOS declares the multi-resolution ICNS through `CFBundleIconFile`. Windows executables embed the multi-resolution ICO through architecture-specific resources, and the NSIS installer uses the same ICO. Linux native packages and the source AUR recipe install scalable SVG and 48- and 512-pixel PNG icons under the hicolor theme. AppImage carries the 512-pixel PNG at its root. The stage manifest digests each staged icon. See the [asset guide](../../app/desktop/internal/appicons/README.md) for regeneration.

Every Desktop stage also retains the root AGPL license and `licenses/BOARDUI-MIT.txt`, `licenses/Inter-OFL.txt` and `licenses/REMIX-LICENSE.txt`. macOS stores them in the application Resources directory; Linux and Windows store them with the private payload. The normal immutable stage file manifest hashes these notice bytes, and the format constructors preserve them. A local unsigned macOS application-archive replay verifies the actual archive and native identity/private-service smoke. It does not install or open a GUI.

The format constructor supplies DEB, RPM, pacman packages, NSIS setup EXE, portable ZIP, macOS DMG/application archive and AppImage/update archive entrypoints.
Only tested construction/lifecycle states count as acceptance; see the [package delivery receipt](../exec/0018-v2-package-channel-receipt.md).
Linux uses the pinned mygo GTK3 native-view path and a tray indicator library. These recipes do not inherit v1's GTK4/Wails/WebKitGTK dependency set.
AppImage currently uses host GTK3 libraries; self-contained runtime delivery remains open.

Desktop packages keep `unic-desktop`, `unic` and `unicd` together in a private directory. Linux exposes only `uniclipboard` as the graphical launcher, preserving independently installed CLI commands.
`unic-desktop` supplies the launcher, tray, GUI and quick-panel roles from one executable. Each running role has its own OS process. Internal `--role` arguments do not add a distributable executable or change the immutable CLI lock. The macOS `CFBundleExecutable`, Windows Start-menu and Scoop shortcuts, and Linux desktop entry keep using the launcher without a role argument. Login startup also enters this launcher and reads the saved desktop mode.
The tray owns login registration and application updates. Its application name and installation identity remain stable across roles and profiles; profile-scoped activation uses Desktop IPC rather than a different native application name. macOS tray and quick-panel processes use accessory activation; only the GUI uses regular activation. Explicit application quit and updater relaunch must stop the owned GUI and quick-panel processes before retiring the tray. The Windows installer checks the shared `unic-desktop.exe` image before replacement, so a running GUI, quick panel or tray blocks replacement. Native process and login acceptance remain separate from package construction.
macOS bundles retain `app.uniclipboard.desktop`. The Windows installer retains the v1 per-user uninstall registration, checks running owned executables before replacement, and preserves user data.
Actual v1 installer replacement and GUI behavior of a formally installed package remain unverified. Source-built Windows ARM64 native role/lifecycle acceptance is recorded separately in the [process receipt](../exec/desktop-process-lifecycle.md).

`tools/package/channels.py` renders twelve channel definitions from exact local artifact hashes. Mixed builds, missing inputs, unsafe filenames and modified files fail.
The self-managed Tap retains both CLI Formula and Desktop Cask roles. AUR includes a source `-git` recipe and a generated binary recipe; their provenance remains distinct.
The renderer writes explicit pending installation/publication states. It never submits, uploads, signs a repository or advances a channel.
`tools/package/pacman.py` requires an exact package lifecycle receipt, clean production provenance and an explicit signing key environment before repository signing.
The lifecycle attachment entrypoint verifies the package digest and required observations before updating acceptance metadata.

`UNIC_MACOS_SIGNING_IDENTITY` and `UNIC_WINDOWS_CERT_SHA1` let the CLI builder sign executable files before archive hashing and native acceptance.
Desktop signing first verifies distribution signatures on the locked CLI files and signs only Desktop-owned bytes. Real certificate signing is unverified on this host.
The Desktop lock selects CLI `2.0.0 (2)` from canonical distribution run [45030337326](https://github.com/UniClipboard/UniClipboard/actions/runs/45030337326) by manifest SHA-256 `19a091d691c8d1ad5ea7bdfb12be39a2e6a2235756a919b7c9f415c4719dd8ec`. Both macOS targets have distribution signatures. All six targets passed the shared configuration, API and service-lifecycle source checks against Desktop. The [previous lock rejection receipt](../exec/baselines/desktop-cli-lock-20261120/receipt.json) records why the earlier candidate was replaced. `fetch-cli --allow-ci` verifies the canonical workflow, clean source, complete target matrix, package digests and macOS signing before acquiring the candidate. The default fetch path requires public immutable release assets.
`build-desktop.yml` constructs six native targets from that exact lock. Explicit distribution mode imports an ephemeral Apple keychain, signs the Desktop payload, submits a copied bundle to Apple notarization and staples the accepted ticket before final archive hashing. Credentials stay in the ephemeral keychain. Cleanup runs after failure as well as success.
Native package smoke keeps version inspection state-free, then explicitly initializes its isolated CLI profile with `unic migrate --initialize-empty --json` before shared-configuration IPC. Discovery HOME, XDG and Windows profile paths remain private. Portable and explicit data directories initialize independently; there is no gate bypass or direct receipt write. Disposable DEB lifecycle acceptance initializes before its first business command and verifies that the receipt survives upgrade and removal.

The primary PR Desktop suite uses the same Ubuntu 24.04 and native GTK 3/indicator runtime baseline as the native Linux acceptance workflow. It installs display/recording tools without the webview-only WebKitGTK stack. The complete canonical suite is partitioned into three disjoint round-robin shards on independent hosted runners. All shards must pass, fail-fast is disabled, and each keeps the thirty-minute job budget. The canonical runner records a separate evidence artifact for each shard; unselected cases remain NOT_RUN in that shard.

`package-acceptance.yml` runs six-target nonvisual package and signed-update process acceptance, plus disposable-container DEB lifecycle checks. Exact observed results are recorded in the package receipt. These build workflows do not publish.

### Native application updates

The Desktop host uses the pinned mygo native updater plugin. `appupdates.Source` converts the numbered FlareRelease feed into mygo manifests. The app version includes `-build.<number>` so later builds of one product version compare numerically. The adapter validates the target, archive format, bounded size, SHA-256, raw Ed25519 signature over the archive digest and the signed embedded `release-identity.json`. mygo owns extraction and installation.

`desktop.update_channel` accepts `stable` (default) or `alpha` and takes effect on the next check. Updater preferences use the v2 data directory. `release/products.toml` selects the durable `update_public_key`; an empty key disables in-app updates. The production private key must use mygo's raw Ed25519 format, independent of existing Minisign/Tauri keys. `go run ./tools/release/update-sign` generates a private key directory or signs archives from `UNIC_UPDATE_PRIVATE_KEY` without logging the private key.

The host enables automatic/native-menu updates only for direct installations. System Linux packages, Snap, Flatpak, AppImage, Homebrew-owned bundles, Scoop and portable archives remain externally managed. NSIS records winget/Chocolatey ownership from `/MANAGED=`. Manual reinstalls preserve that receipt. Windows and Linux updates use mygo's owned-file list and preserve unowned data. AppImage self-update remains unsupported by the pinned SDK. Public feed/key provisioning remains a deployment prerequisite.

The registered CLI build entry can also call `native-desktop-acceptance.yml` to run the canonical history/search/copy native-window case on all six targets. Its screenshots, continuous recording, source/binary hashes and cleanup receipt remain distinct from production installer and real-system clipboard acceptance. Windows arm64 records the pinned Go toolchain's unavailable race detector explicitly.

### Desktop delivery controller

`tools/release/desktop_release.py` acquires six-target candidates only from a successful canonical repository build, checks package/source provenance and assembles the complete 22-artifact catalogue. Production assembly requires native executable identity, macOS code signatures, Apple notarization and native updater signatures. Windows payloads and installers may be unsigned Authenticode for both alpha and stable. Their `signed`, `payload_signed` and `container_signed` fields remain false; release pages explicitly declare unsigned Windows. Test certificates never establish production signing. Raw Ed25519 updater signatures remain mandatory for every updater archive. The publication command also verifies npm availability of the locked CLI set.

The controller uses the maintained boto3 SDK with create-only conditional R2 writes and byte-for-byte read-back. The catalogue and successful publication receipt remain immutable objects. It checks the deployed FlareRelease contract, authenticates with Cloudflare Access service credentials and registers the six updater archives through the existing API. The Production environment pins the R2 endpoint. When dedicated S3 credentials are absent, the controller verifies the existing Cloudflare token and derives the SigV4 key in memory for the configured account’s default-jurisdiction endpoint. The read-only preflight compares the full bytes of a known v1 object with its accepted hash and checks both numbered contracts. The current 512 MiB registration limit applies to this updater set. All other formats remain downloadable immutable objects. Registration does not change a channel.

Promotion consumes the immutable availability receipt, requires an explicit channel revision and verifies public numbered identity/updater hashes after the API's compare-and-swap mutation. Stable requires one native acceptance receipt for each of macOS, Linux and Windows. Each receipt binds the catalogue SHA-256 and records passing GUI and lifecycle checks against the exact hash of every artifact for that platform. Stable also verifies that all seven packages in the locked CLI manifest have matching npm integrity and that each npm `latest` tag selects the locked version. `release-desktop.yml` defines separate manual read-only preflight, publication and promotion operations. It has not been run for public delivery. `release-cli.yml` can consume an accepted canonical run without rebuilding the locked CLI bytes.

### GitHub Desktop Release pages

`tools/release/github_desktop.py` adapts the v1 Release template to independent numbered Desktop builds. The template is `.github/release-notes/desktop.md.tmpl`. It renders changelog/source links, all supported native installer downloads, exact locked standalone CLI downloads, verification instructions and optional GitHub-generated new contributors. English and Chinese changelog strings remain available in `desktop-notes.json`. Mobile download claims remain outside this Desktop page.

The tag is `desktop/v<version>-build.<number>` and points to the accepted candidate source, independently of the CLI tag. Draft staging attaches the 22 native files, six raw updater signatures, the immutable catalogue, bilingual notes, reproducible page context and `SHA256SUMS.txt` (32 attachments). Existing bytes and tag identity must match; missing draft assets can resume, while extra/changed assets fail. Every uploaded file is downloaded and hashed before accepting staging. Published releases cannot repair missing assets or downgrade from stable to alpha.

The existing delivery workflow supports `github-draft`, `publish` and `promote`. It stages or verifies the page before origin/channel mutation. Alpha pages become public prereleases only after origin publication read-back. Stable publication leaves a draft until verified stable promotion, then marks the page as the latest Desktop release. The page context restores the original comparison tag/contributors across workflows. Local `preview --development` produces reviewable files without creating a page; remote operations retain all production signing gates.

`e2e/packaging/github_release.py` drives the real GitHub CLI and release subprocess against disposable loopback HTTP endpoints. Its report and checksums establish process/transport behavior only. It does not create a real draft or public GitHub Release.

`sandbox-acceptance.yml` runs actual Nix, Flatpak and strict Snap construction/execution on both Linux architectures. The harness overrides source acquisition with the exact verified pre-publication DEB; generated runtime/build steps remain unchanged. Missing runtimes, namespace failures or confinement failures fail the job. Nix sets only the Desktop ELF interpreter and supplies GTK libraries through the existing wrapper; it does not add RUNPATH to the prebuilt Go binary. The locked private CLI pair remains unchanged. The process harness verifies real private CLI/service IPC and bounded shutdown inside each environment. These nonvisual process checks do not establish clipboard, file/network, refresh or sandbox GUI acceptance.

## v1 upgrade notice and migration

The CLI checks known v1 paths only when stderr is a terminal and `--json` is absent. It performs directory stat calls; it does not open credentials or a database. A default v1 directory or the active `UC_PROFILE` directory triggers the warning. Missing paths and regular files remain silent. Custom and portable profiles outside these locations require explicit `--source` selection.

The warning explains v1 incompatibility and points to migration help, preflight and import commands. It does not request acknowledgement or migrate automatically. `unic migrate` and `unic migrate --help` print a read-only guide before opening any profile.

CLI archives and npm platform packages include the migration importer within `unic`. Explicit migration runs in-process without starting the service. The importer keeps source data unchanged, verifies reopened target content and resumes by record identity. Partial migration returns 3. All peers must use v2 and pair again; settings, identities and trust are not imported.

The importer encodes Windows drive paths as absolute SQLite file URIs (`file:///C:/...`), retaining read-only/query-only access. A drive letter must not become the URI authority. The [unsigned Windows receipt](../exec/0018-unsigned-windows-acceptance.md) records the native replacement and frozen-history acceptance boundary.

The packaged-process E2E uses a temporary home, pseudoterminals and fixed encrypted Rust fixtures. It checks fresh-install silence, directory detection, JSON output, offline help, read-only preflight, partial results, source hashes and idempotent repair/retry. It never reads a personal v1 profile or system credential.

## Build, publication and promotion

Git attributes require LF checkout bytes for release TOML metadata on every platform. Metadata digests cover the original file bytes. The real Git checkout regression enables `core.autocrlf=true` and verifies that checkout preserves the canonical metadata bytes before building candidates.

`build-cli.yml` runs for PRs, main/v2 pushes, manual requests and reusable calls. PR candidates use the actual PR head rather than a synthetic merge SHA. Explicit distribution builds import the existing Apple certificate into a temporary keychain, sign CLI executables before hashing and remove the keychain after construction. Its callers provide an exact source SHA. It checks the metadata digest, source identity, required target set and native executable receipts before `npm pack`. CI builds do not publish.

`release-cli.yml` supports alpha and direct stable publication. It first checks for a durable record for the selected version/build. A complete existing record restores its exact package files. Otherwise it awaits the reusable build. It stages the manifest and packages in a draft GitHub release before publishing to npm. The product tag is `cli/v<version>-build.<number>` and must reference the accepted source SHA. Both delivery controllers find draft and published pages by exact tag in the paginated authenticated release list. Existing assets are compared and never overwritten. Registry read-back waits up to three minutes for a successful upload or tag change to become visible and rejects a different published integrity immediately. Incomplete staging fails before npm publication; recovering that case requires the original candidate artifacts.

Public npm operations require clean source, GitHub Actions OIDC and npm with dist-tag OIDC support. Every package must authorize the exact release and promotion workflow identities. Packages may require an initial owner-controlled bootstrap before trusted publishing can be configured. Configuration is a deployment prerequisite, not proof supplied by local acceptance.

Publication validates every local archive and every existing registry version before the first mutation. It publishes platform packages first and the main package last. Each mutation has registry read-back. A durable record becomes public only after a real availability receipt passes validation.

`promote.yml` currently offers CLI only. It downloads the requested durable manifest, verifies all existing npm package integrity values and moves the exact package set to `latest`. It does not rebuild, pack or upload. It retains alpha. Both mutating workflows share one CLI concurrency group.

Retries preserve exact identity and reject integrity conflicts. An older build cannot replace a newer selected build. Known v1 alpha/beta/rc tags may advance only to a strictly higher product version. Unknown or same-base legacy tags require review. npm changes across packages are not atomic; a partial failure has a failed receipt and can be retried with the same candidate. External writers outside the workflow concurrency group remain an operational constraint because npm does not expose a compare-and-swap tag API.

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
| `.github/workflows/build-cli.yml` | Native CLI target construction and acceptance. |
| `.github/workflows/release-cli.yml` | Candidate recovery, reusable build and npm publication. |
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

The selected next CLI version is `2.0.0 (2)`, encoded as `2.0.0-build.2` on npm. Alpha selects the `alpha` dist-tag. Desktop selects `2.0.0 (1)`; Android and iOS retain independent local candidate seeds. The v1-to-v2 npm tag transition is implemented. Historical remote five-target build and package assembly acceptance passed. The six-target CLI matrix has native hosted acceptance. CLI build 2 is selected to include the current shared configuration/API; the execution receipt records its final accepted lock. Workflow authorization remains a publication prerequisite. Public OIDC/provenance and real GitHub release publication remain unverified.

Desktop staging checks the locked private service configuration/API in addition to executable hashes. macOS signing/notarization, native Windows installer lifecycle, and two-architecture Nix/Flatpak/strict Snap process/IPC acceptance have passed as recorded in the execution receipt. Production delivery requires the durable raw updater key and the numbered public routes. R2/FlareRelease transport and the locked CLI npm availability gate are implemented but have not performed production delivery. Native sandbox GUI, package refresh and real-OS clipboard/file/network acceptance remain open. Android and iOS share an Expo UI shell under `app/expo`; it has no Engine integration and does not establish native release support. Their workflows, signing, Store acceptance and promotion are not implemented. GitCode is deferred. Do not archive decision 0018 while these phases remain open.

Exact interface requirements are in the [release contract](../contracts/release-contract.md). Process ownership remains in the [headless architecture](headless.md).

## Accepted CLI acquisition

`tools/release/cli_candidate.py` acquires the exact `cli-npm-candidate` artifact from a successful canonical Build CLI run in this repository. It checks workflow identity, source SHA, product identity, the complete declared matrix, native receipts and package SHA-256/SHA-512 before changing a Desktop lock. `--require-macos-signing` rejects unsigned macOS component builds.
The optional `cli_artifact_run_id` records the immutable acquisition source for pre-publication construction. It does not establish npm availability or satisfy the stable Desktop gate.
