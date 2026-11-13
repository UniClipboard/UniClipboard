---
paths:
  - .gitattributes
  - release
  - tools/package
  - packaging
  - e2e/packaging
  - app/desktop/internal/appupdates
  - app/desktop/e2e/updateprobe
  - internal/localconfig
  - app/desktop/cmd/unic-desktop
  - tools/release
  - internal/releaseinfo
  - app/service/cmd/unic
  - internal/localpaths
  - internal/v1migration
  - e2e/release
  - .github/workflows/build-cli.yml
  - .github/workflows/build-desktop.yml
  - .github/workflows/package-acceptance.yml
  - .github/workflows/release-cli.yml
  - .github/workflows/promote.yml
verified: dc779bfbd9b014979cd71d8954478cfe5dea7fa5
---
# Product builds and CLI releases

## Current implementation

`release/products.toml` owns product versions and build numbers. The implemented release product is CLI. Desktop selects `2.0.0 (1)` for the first v2 candidate. Android and iOS records reserve independent metadata. These records do not establish release support.

`go run ./tools/package -product cli` builds `unic` and `unicd` for the current target. The versioned archive contains those programs and the `uniclip` alias. Unix archives preserve the relative symlink. Windows archives provide `uniclip.cmd`. Development packaging through `make package-headless` and `make package-desktop` remains available and unsigned.

All runtime programs handle `--version` and `--version --json` before initialization. Release builds embed version, build number and source SHA. Development builds report `dev` unless packaging supplies metadata.

CLI assembly uses the existing npm main/platform-package model. `@uniclipboard/cli` pins every optional platform package to the exact `<version>-build.<number>` registry version. Both npm CLI command names use one launcher. The package also exposes `unicd` through a dedicated service entrypoint. The launcher executes the real CLI beside its sibling service. Alpha selects the npm `alpha` dist-tag. Stable selects `latest`. Channel names do not enter package versions.

The declared CLI matrix covers macOS arm64/amd64, Linux arm64/amd64 and Windows arm64/amd64. The Windows arm64 target passed native hosted executable acceptance in distribution run 43408274394. Each target runs its own executable identity acceptance. Linux amd64 also runs production headless and packaged release/upgrade process acceptance. Remote native acceptance passed for all five targets in run 42777109814. The [remote verification receipt](../exec/0018-cli-remote-verification.md) records exact source, artifacts and the observed headless retry.

## Desktop staging and package definitions

`go run ./tools/package -product desktop -target <os>-<arch> -cli-manifest <file> -output <directory>` delegates production staging to `tools/package/desktop.py`.
The metadata must lock the exact CLI manifest digest, version and build. The stager checks the selected npm package SHA-256/SHA-512, native acceptance fields and both executable hashes before building Desktop.
It copies accepted CLI/service files unchanged. It rejects moving source, changed inputs and existing output directories.
`--development` permits a dirty construction candidate and records that state; it does not establish a release or a separate installation identity.
Use isolated homes for all construction candidates. Version-`dev` executables use the separate development identity and default data directory.

The format constructor supplies DEB, RPM, pacman packages, NSIS setup EXE, portable ZIP, macOS DMG/application archive and AppImage/update archive entrypoints.
Only tested construction/lifecycle states count as acceptance; see the [package delivery receipt](../exec/0018-v2-package-channel-receipt.md).
Linux uses the pinned mygo GTK3 native-view path and a tray indicator library. These recipes do not inherit v1's GTK4/Wails/WebKitGTK dependency set.
AppImage currently uses host GTK3 libraries; self-contained runtime delivery remains open.

Desktop packages keep `unic-desktop`, `unic` and `unicd` together in a private directory. Linux exposes only `uniclipboard` as the graphical launcher, preserving independently installed CLI commands.
macOS bundles retain `app.uniclipboard.desktop`. The Windows installer retains the v1 per-user uninstall registration, checks running owned executables before replacement, and preserves user data.
Actual v1 installer replacement and native GUI behavior remain unverified.

`tools/package/channels.py` renders twelve channel definitions from exact local artifact hashes. Mixed builds, missing inputs, unsafe filenames and modified files fail.
The self-managed Tap retains both CLI Formula and Desktop Cask roles. AUR includes a source `-git` recipe and a generated binary recipe; their provenance remains distinct.
The renderer writes explicit pending installation/publication states. It never submits, uploads, signs a repository or advances a channel.
`tools/package/pacman.py` requires an exact package lifecycle receipt, clean production provenance and an explicit signing key environment before repository signing.
The lifecycle attachment entrypoint verifies the package digest and required observations before updating acceptance metadata.

`UNIC_MACOS_SIGNING_IDENTITY` and `UNIC_WINDOWS_CERT_SHA1` let the CLI builder sign executable files before archive hashing and native acceptance.
Desktop signing first verifies distribution signatures on the locked CLI files and signs only Desktop-owned bytes. Real certificate signing is unverified on this host.
Desktop locks CLI `2.0.0 (1)` from canonical distribution run 43408274394 by manifest SHA-256. `fetch-cli --allow-ci` validates the canonical successful workflow, source, complete target matrix, hashes and macOS signing before acquiring this pre-publication candidate. The default fetch path requires public immutable release assets.
`build-desktop.yml` constructs six native targets from that exact lock. Explicit distribution mode imports an ephemeral Apple keychain, signs the Desktop payload, submits a copied bundle to Apple notarization and staples the accepted ticket before final archive hashing. Credentials stay in the ephemeral keychain. Cleanup runs after failure as well as success.
`package-acceptance.yml` runs six-target nonvisual package and signed-update process acceptance, plus disposable-container DEB lifecycle checks. Exact observed results are recorded in the package receipt. These build workflows do not publish.

### Native application updates

The Desktop host uses the pinned mygo native updater plugin. `appupdates.Source` converts the numbered FlareRelease feed into mygo manifests. The app version includes `-build.<number>` so later builds of one product version compare numerically. The adapter validates the target, archive format, bounded size, SHA-256, raw Ed25519 signature over the archive digest and the signed embedded `release-identity.json`. mygo owns extraction and installation.

`desktop.update_channel` accepts `stable` (default) or `alpha` and takes effect on the next check. Updater preferences use the v2 data directory. `release/products.toml` selects the durable `update_public_key`; an empty key disables in-app updates. The production private key must use mygo's raw Ed25519 format, independent of existing Minisign/Tauri keys. `go run ./tools/release/update-sign` generates a private key directory or signs archives from `UNIC_UPDATE_PRIVATE_KEY` without logging the private key.

The host enables automatic/native-menu updates only for direct installations. System Linux packages, Snap, Flatpak, AppImage, Homebrew-owned bundles, Scoop and portable archives remain externally managed. NSIS records winget/Chocolatey ownership from `/MANAGED=`. Manual reinstalls preserve that receipt. Windows and Linux updates use mygo's owned-file list and preserve unowned data. AppImage self-update remains unsupported by the pinned SDK. Public feed/key provisioning and complete release orchestration remain prerequisites.

## v1 upgrade notice and migration

The CLI checks known v1 paths only when stderr is a terminal and `--json` is absent. It performs directory stat calls; it does not open credentials or a database. A default v1 directory or the active `UC_PROFILE` directory triggers the warning. Missing paths and regular files remain silent. Custom and portable profiles outside these locations require explicit `--source` selection.

The warning explains v1 incompatibility and points to migration help, preflight and import commands. It does not request acknowledgement or migrate automatically. `unic migrate` and `unic migrate --help` print a read-only guide before opening any profile.

CLI archives and npm platform packages include the migration importer within `unic`. Explicit migration runs in-process without starting the service. The importer keeps source data unchanged, verifies reopened target content and resumes by record identity. Partial migration returns 3. All peers must use v2 and pair again; settings, identities and trust are not imported.

The packaged-process E2E uses a temporary home, pseudoterminals and fixed encrypted Rust fixtures. It checks fresh-install silence, directory detection, JSON output, offline help, read-only preflight, partial results, source hashes and idempotent repair/retry. It never reads a personal v1 profile or system credential.

## Build, publication and promotion

Git attributes require LF checkout bytes for release TOML metadata on every platform. Metadata digests cover the original file bytes. The real Git checkout regression enables `core.autocrlf=true` and verifies that checkout preserves the canonical metadata bytes before building candidates.

`build-cli.yml` runs for PRs, main/v2 pushes, manual requests and reusable calls. PR candidates use the actual PR head rather than a synthetic merge SHA. Explicit distribution builds import the existing Apple certificate into a temporary keychain, sign CLI executables before hashing and remove the keychain after construction. Its callers provide an exact source SHA. It checks the metadata digest, source identity, required target set and native executable receipts before `npm pack`. CI builds do not publish.

`release-cli.yml` supports alpha and direct stable publication. It first checks for a durable record for the selected version/build. A complete existing record restores its exact package files. Otherwise it awaits the reusable build. It stages the manifest and packages in a draft GitHub release before publishing to npm. The product tag is `cli/v<version>-build.<number>` and must reference the accepted source SHA. Existing assets are compared and never overwritten. Incomplete staging fails before npm publication; recovering that case requires the original candidate artifacts.

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

The selected next CLI version is `2.0.0 (1)`, encoded as `2.0.0-build.1` on npm. Alpha selects the `alpha` dist-tag. Desktop selects `2.0.0 (1)`; Android and iOS retain independent local candidate seeds. The v1-to-v2 npm tag transition is implemented. Historical remote five-target build and package assembly acceptance passed. The new six-target matrix has not been accepted remotely. Workflow authorization remains a publication prerequisite. Public OIDC/provenance and real GitHub release publication remain unverified.

Desktop staging can consume an immutable CLI lock, but the checked-in lock remains unresolved. Signed-release acceptance, notarization, R2 uploads and FlareRelease registration are not complete. Its stable CLI gate remains a required future implementation. Android and iOS share an Expo UI shell under `app/expo`; it has no Engine integration and does not establish native release support. Their workflows, signing, Store acceptance and promotion are not implemented. GitCode is deferred. Do not archive decision 0018 while these phases remain open.

Exact interface requirements are in the [release contract](../contracts/release-contract.md). Process ownership remains in the [headless architecture](headless.md).

## Accepted CLI acquisition

`tools/release/cli_candidate.py` acquires the exact `cli-npm-candidate` artifact from a successful canonical Build CLI run in this repository. It checks workflow identity, source SHA, product identity, the complete declared matrix, native receipts and package SHA-256/SHA-512 before changing a Desktop lock. `--require-macos-signing` rejects unsigned macOS component builds.
The optional `cli_artifact_run_id` records the immutable acquisition source for pre-publication construction. It does not establish npm availability or satisfy the stable Desktop gate.
