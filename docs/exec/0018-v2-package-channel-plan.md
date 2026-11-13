# Implementation plan: v2 package channel parity

- Status: implementation in progress; construction/process checks partially passed. Complete native, sandbox and publication acceptance remains open. See the [implementation receipt](0018-v2-package-channel-receipt.md).
- Date: 2026-11-13.
- v2 baseline: `1a6e8d617a8719b575a0c5bf9a6eec1060d730e5`.
- v1 inventory baseline: `c6483d73c7a0234be68571fdb5fa3d38a17c4871` (`origin/v1`, local reference).
- Parent: [independent product release plan](0018-independent-product-build-and-release.md).
- Existing constraints: [release architecture](../architecture/releases.md), [release contract](../contracts/release-contract.md), [headless architecture](../architecture/headless.md).

## 1. Outcome and scope

Produce working v2 packages and definitions for all twelve v1 distribution channels.
Include channels whose v1 definitions are incomplete or unpublished. A template is not completion.
Retain direct-download formats: macOS DMG and application update archive; Windows setup EXE and portable ZIP; Linux DEB, RPM, AppImage and update archive; CLI archives.
DEB is a package format here. Operating a new APT repository is not part of v1 parity.
Android/iOS delivery remains in the parent plan.

Track five separate results for every channel: package built, package installed and accepted, submission sent, submission approved, destination publicly available.
The packaging milestone requires the first two results. Public rollout additionally requires all applicable destination results.
Missing credentials or external approval must leave publication pending without hiding a packaging failure.

## 2. Verified baseline and prerequisites

| Area | Current v2 state | Required work |
|---|---|---|
| CLI | Five-target candidate, npm assembly, publication and promotion implementation | Reuse existing machinery; independently verify current public state before rollout. |
| Desktop | `tools/package -desktop` creates development archives | Add production bundle layout, immutable CLI consumption, installers and acceptance. |
| CLI lock | Desktop metadata has an empty manifest digest and seed version | Select an actual accepted CLI build; never invent a digest or silently rebuild its files. |
| Desktop matrix | macOS arm64/amd64, Linux amd64, Windows amd64 | Add Linux arm64 and Windows arm64; require native evidence for every target. |
| CLI matrix | macOS arm64/amd64, Linux arm64/amd64, Windows amd64 | Add Windows arm64 before promising a native arm64 Desktop bundle; publish the corresponding npm platform package through existing release gates. |
| Runtime | mygo Desktop, Go CLI/service | Audit actual runtime libraries and OS minimums; remove v1 Wails, Rust daemon and WebKitGTK assumptions. |
| Distribution | CLI/npm implemented; Desktop R2/FlareRelease work remains open | Complete the relevant parent-plan Desktop gates before public activation. |

Do not change product versions as a side effect of adding recipes.
The six-target Desktop matrix is a planned requirement, not current support.
Self-managed pacman parity initially targets x86_64, matching its v1 workflow. Add aarch64 repository publication only after its own signed installation acceptance.

## 3. Packaging design

### Accepted installation policy (2026-11-13)

The user accepted replacement of v1 by v2 within the same distribution channel.
The user also accepted the product/package identifiers, independent product versions, private bundled CLI ownership, update authority and explicit migration rules in [decision 0025](../decisions/proposed/0025-desktop-package-identity-and-replacement.md).
The normative requirements are in the [release contract](../contracts/release-contract.md#desktop-installation-identity-and-update-ownership).
The user selected Desktop version `2.0.0`, build number `1`. `release/products.toml` records this candidate identity. The CLI lock remains unresolved until an exact accepted CLI build and manifest are selected; matching product version numbers do not establish that lock.
These requirements are accepted but not implemented. Exact native version encodings, platform installation metadata and lifecycle acceptance remain P0 work.

### Implementation structure

Keep `release/products.toml` as product metadata authority.
Extend `tools/package` to stage the production Desktop layout. Keep `tools/release/release.py` responsible for release identity, manifests and destination orchestration.
Store channel-native definitions under `packaging/`; retain `snap/snapcraft.yaml` as the Snap entry.
Use small format-specific scripts under `tools/package/` where native tools are required. Do not build a generic plugin framework or a second version database.

Use the current Go/Python tooling for staging and rendering. Use maintained native packaging tools for container formats: nFPM for DEB/RPM, NSIS for setup EXE, Apple signing/notarization and DMG tools, AppImage tooling, makepkg/repo-add, Snapcraft, flatpak-builder and Nix.
Pin tool versions and downloaded tool hashes when implementing each format. Record the actual tool identity in its receipt.
Do not replace the existing release controller with GoReleaser merely to obtain its format wrappers.

The production Desktop payload contains `unic-desktop`, the exact accepted `unic` and `unicd`, and the supported `uniclip` alias.
Record a file ownership map. Define command exposure so a Desktop installation and an independently installed CLI do not overwrite each other's files.
Use an application-private sibling directory for bundled CLI files; keep public command links under explicit package ownership and preserve independently installed CLI commands.
Do not install a system service merely because a desktop package contains `unicd`.
Preserve the existing profile service ownership rules and user data during uninstall.

Sign native executables before final container assembly where required. Sign/notarize final containers where supported, then calculate final distribution hashes.
Every channel recipe binds to an exact product version, build number, source SHA, CLI lock, artifact URL and digest.
Package formats may have different bytes. They must resolve to the same accepted payload identity unless the receipt explicitly describes a source build.
Never use `latest` URLs, placeholder digests or version-only source selection in a release recipe.

Define native version ordering before constructing the first candidate. Verify increasing product versions and increasing build numbers with each package manager's real comparison rules.
npm keeps `<version>-build.<number>`. DEB/RPM/pacman should use their native package revision field where valid.
Specify Windows installer/display versions and Homebrew/Nix/Snap representations in the release contract after actual tool validation.
Alpha/stable selection remains outside product identity; a package manager's version restrictions do not justify repackaging during promotion.
Construct all promotable format packages at candidate time. Promotion selects those exact bytes and accepted definitions.

Source channels need separate treatment. AUR `-git` builds the Desktop source at a recorded v2 revision and consumes that revision's exact CLI lock; it is a development snapshot, not the stable binary candidate.
Stable pacman packages use a fixed release revision. Snap/Nix/Flatpak recipes must explicitly declare whether they consume accepted binaries or build Desktop source.
If an upstream channel requires source builds, emit a channel-native build receipt with the source, CLI lock, toolchain and output hashes. Do not claim those bytes equal the upstream binary build.
Before channel activation, extend the release contract to describe these derived builds without weakening exact-byte promotion for an already accepted build.

## 4. Channel matrix

| Channel | Planned definitions and artifacts | Required acceptance |
|---|---|---|
| Homebrew self-managed Tap | Desktop Cask for both macOS architectures; inspect existing external Tap before adapting any CLI formula | Download/digest validation, brew audit/style, install, upgrade, uninstall, command ownership and app signature. |
| Official Homebrew Cask | Cask generated from the same accepted DMGs | Official Cask checks and local installation; submission and merge are separate states. |
| Scoop Extras | amd64/arm64 manifests using portable ZIP; verified persist and autoupdate configuration | Install in a path containing spaces, data persistence across upgrade, relocation and uninstall. |
| Chocolatey | nupkg, install/uninstall scripts, amd64/arm64 installer selection | Real choco pack, local-source install/upgrade/uninstall, silent execution and exit codes. |
| WinGet | Version/installer/locale YAML generated from signed setup EXEs | winget manifest validation, local-manifest installation, upgrade and uninstall; public index acceptance separate. |
| AUR | v2 `PKGBUILD` and generated `.SRCINFO`; retain `uniclipboard-git` source-build behavior | Clean makepkg builds on x86_64/aarch64, dependency audit, install/start/upgrade/remove; selected v2 branch and source revision recorded. |
| Self-managed pacman | Stable pkg.tar.zst, detached signatures, signed repository databases and public key | Fresh isolated keyring, signature rejection, install/upgrade/remove and repository metadata read-back. |
| COPR | spec, SRPM and resulting RPM for x86_64/aarch64 | rpmbuild/mock installation, dependencies, ownership, upgrade/removal; exact COPR build and repo state for public rollout. |
| Snap Store | Snapcraft definition and amd64/arm64 snaps | Real snapd strict-confinement install, app/service IPC, clipboard, network, files, refresh/removal and channel identity. |
| Nix / NixOS | package.nix, flake and immutable sources metadata for x86_64-linux/aarch64-linux | nix build, closure/library check, launch through wrappers, profile upgrade/GC, service discovery and declarative autostart. |
| Flatpak / Flathub | Manifest, AppStream metadata, icons, screenshots, repository/bundle output for x86_64/aarch64 | flatpak-builder, metadata validation, real sandbox install/start/upgrade/remove and clipboard/file/network permissions. |
| npm CLI | Existing main/platform packages; add Windows arm64 | Existing real-process release E2E and all native target checks; public OIDC/provenance and registry read-back separately. |

Keep native identifiers stable across alpha/stable. Inspect v1 identifiers before modifying them; data migration remains explicit and read-only at the source.
Retain the public `uniclip` command alias. Do not copy v1 daemon names or WebView requirements into v2 manifests.
Define portable data and socket roots before implementing Scoop. A portable marker with no implemented runtime behavior is a failing case.
Sandbox installs need explicit data/socket paths and service ownership. Do not connect a confined Desktop to an arbitrary host daemon or give blanket filesystem/D-Bus access to mask failures.
Use the current runtime's real requirements when choosing Snap plugs and Flatpak permissions.
Package-managed installations must direct updates to their owning package manager. Only accepted direct-download formats may use application-managed replacement.

## 5. Delivery order

### P0. Freeze identities and failure cases

- [ ] Record v1-to-v2 channel/format/architecture mapping and inspect the live external Tap.
- [ ] Audit mygo and clipboard runtime dependencies on each target; establish minimum OS/runtime baselines.
- [x] Accept product/package identity, same-channel v1 replacement, shared alpha/stable installation identity, isolated development identity, independent product versions, private bundled CLI ownership, update authority and explicit migration rules (2026-11-13).
- [ ] Map accepted identities to platform installation metadata; specify service/socket paths, portable behavior, native version encoding and lifecycle acceptance scenarios.
- [ ] Write failure cases and process E2E scenarios before implementation. Resolve derived source-build rules in the contract.

Exit: reviewed release-contract additions, target matrix and executable acceptance cases. No publication.

### P1. Establish one complete Desktop path

- [x] Implement exact accepted CLI consumption and production Desktop staging; process fixtures passed.
- [ ] Select the real complete production CLI lock; the checked-in lock remains unresolved.
- [x] Complete the available Linux arm64 DEB construction/install/upgrade/uninstall path with private service and data-preservation evidence.
- [ ] Repeat the complete lifecycle on Linux amd64; arm64 acceptance does not satisfy that target.
- [ ] Preserve this working path while extending other formats. Do not create all channel workflows before one package works.

Exit: one reproducible production package and process acceptance receipt. This is a Linux amd64 slice, not a complete six-target release candidate. Public stable still requires the exact locked CLI to be stable in npm.

### P2. Complete direct-download packages and architecture coverage

- [ ] Build and accept the missing Windows arm64 CLI target through the existing candidate flow, then select one complete CLI build for the final six-target Desktop candidate.
- [x] Add macOS bundle, DMG/application archive construction and signing entrypoints; unsigned arm64 construction passed.
- [ ] Complete real signing/notarization and both native macOS targets.
- [x] Add Windows setup/portable construction and portable runtime semantics; actual amd64/arm64 PE and EXE/ZIP cross-construction passed.
- [ ] Accept production Windows signatures and native installer/runtime lifecycle.
- [x] Add Linux format entrypoints; arm64 DEB/RPM/pacman construction passed.
- [ ] Accept AppImage construction/runtime and complete bundled-versus-system dependency acceptance.
- [ ] Complete all six Desktop native targets; record missing native environments as pending rather than reducing the matrix.
- [ ] Complete parent-plan immutable storage and Desktop release registration prerequisites. Detect install format for update routing.

Exit: complete candidate manifest with final hashes, native identity evidence and format lifecycle receipts.

### P3. Add channels that consume accepted packages

- [x] Implement exact-artifact definition rendering for Homebrew Tap, official Cask, Scoop, Chocolatey and WinGet.
- [ ] Run each native manager and complete submission prerequisites.
- [x] Implement fixed-input Nix/COPR definition rendering.
- [ ] Accept real Nix/SRPM builds and installed derived-output provenance.
- [ ] Extend existing npm acceptance for the complete CLI set; preserve current workflows.
- [ ] Render real candidate values and validate installation locally without publishing.

Exit: runnable package/recipe artifacts and accepted installation transactions for every listed target.

### P4. Add source, repository and sandbox channels

- [x] Adapt the source AUR recipe to the v2 tree and exact CLI fetch/staging entrypoints.
- [x] Generate `.SRCINFO` with real makepkg in an offline container.
- [ ] Run clean source builds and native lifecycle with a real production lock.
- [ ] Produce and verify signed stable pacman repository snapshots using disposable acceptance keys.
- [ ] Complete Snap strict-confinement packaging and Flatpak packaging against real runtimes.
- [ ] Verify actual X11/Wayland capability limits, tray, notifications, file access and user-session lifecycle.

Exit: accepted real package-manager and sandbox transactions. Sandbox restrictions must have explicit product capability results.

### P5. Activate publication and close parity

- [ ] Connect channel-specific workflows to an accepted immutable product candidate. CI package construction must not itself publish.
- [ ] Configure destination credentials/signing, least required workflow permissions and upstream submission identities.
- [ ] Validate retries, partial publication, stale jobs and same-identity conflicts with process E2E before public writes.
- [ ] After explicit rollout authorization, submit/publish selected channels and read back exact versions, hashes and states.
- [ ] Add a destination status report covering all twelve channels; retain pending moderation states.
- [ ] Update live architecture/contracts and run documentation lifecycle checks. Keep 0018 open until its other product obligations are complete.

Exit: channel-by-channel closure receipt. No claim that submission, upload or CI success proves public availability.

## 6. Implementation work packages

Execute the following work packages in order. Preserve the accepted Linux path while adding formats.
Paths not present in the baseline are planned additions. They do not describe existing commands or supported behavior.
Each package starts by recording failure cases and expected process observations before production code changes.

| Work package | Phase | Primary changes | Completion evidence |
|---|---|---|---|
| W01: Freeze packaging inputs | P0 | Extend `docs/contracts/release-contract.md`; record platform IDs, native version encodings, library/OS minimums, command ownership, data/socket roots and source-build identity rules | Platform/channel mapping, failure inventory and exact acceptance scenarios; every unverified prerequisite is named. |
| W02: Consume accepted CLI files | P1 | Extend `tools/package/main.go` and existing release tooling to verify a CLI manifest and stage Desktop-private CLI/service files; set the real lock in `release/products.toml` only after verification | Staged file hashes equal locked inputs; missing/mismatched locks fail; independently installed CLI files remain unchanged. |
| W03: Complete Linux DEB | P1 | Add `packaging/linux/` metadata and nFPM definition; extend packaging tooling; add real lifecycle cases under `e2e/release/` | Actual DEB build/install/start/upgrade/remove, owned service lifecycle and preserved fixture data, with replayable receipt. |
| W04: Complete Windows | P2 | Add Windows arm64 to CLI metadata/matrix/npm assembly/native acceptance; add `packaging/windows/` installer definition and portable staging; implement portable roots in existing host path handling | Native amd64/arm64 receipts, signed setup identity when release prerequisites exist, silent lifecycle and ZIP relocation/persistence. |
| W05: Complete macOS | P2 | Add `packaging/macos/` bundle metadata and packaging scripts; integrate signing/notarization and DMG/update archive construction | Both native architectures, bundle identity, CLI/service resolution, signature/notarization results and final hashes. |
| W06: Complete Linux formats | P2 | Extend `packaging/linux/` for RPM and AppImage; validate arm64; stage runtime libraries from the audited dependency list | Both architectures, clean-host dependency checks, AppImage relocation, RPM lifecycle and update-format selection. |
| W07: Wire Desktop build orchestration | P2 | Add `.github/workflows/build-desktop.yml`; extend release tooling manifests and `e2e/release/`; consume the final complete CLI lock | Reusable build at an exact SHA, required six-target set, per-format receipts, immutable candidate assembly; CI does not publish. |
| W08: Generate binary-channel definitions | P3 | Add `packaging/homebrew/`, `packaging/scoop/`, `packaging/chocolatey/`, `packaging/winget/`, `packaging/nix/` and COPR spec; reuse verified candidate metadata | Real package/manifest validation plus each channel's installation/upgrade/removal receipts; no placeholders. |
| W09: Build source and repository channels | P4 | Add `packaging/aur/`, pacman repository scripts and signed repository assembly | Clean source builds, source/CLI-lock provenance, trusted and rejected signature transactions, installed file ownership. |
| W10: Build sandbox channels | P4 | Add `snap/snapcraft.yaml`, `packaging/flathub/` and required host integration in the existing Desktop/service boundary | Real confinement and sandbox capability receipts, isolated IPC/data roots, refresh/upgrade/removal; native GUI checks retain their own status. |
| W11: Implement release and channel operations | P5 | Add `.github/workflows/release-desktop.yml`, extend `.github/workflows/promote.yml`, implement channel-specific submission workflows and destination status receipts | Local process E2E for retries/partial failure, immutable recovery, version ordering and stale selection; no unauthorized public mutation. |
| W12: Roll out and close | P5 | Configure actual destinations, execute authorized submissions/publication, update architecture/contracts and channel status | Exact destination read-back and per-channel status; unresolved moderation/native checks remain visible. |

Commit implementation by work package or by a smaller complete behavior. Include its contract/architecture changes and acceptance evidence pointers in the same scoped change.
Do not automatically commit or push as a result of this planning task.
Run documentation verification after each behavior/documentation change. If verification markers require a clean source commit, follow the repository's documented commit-then-bump sequence when committing is authorized.

### Dependency and release gates

1. W01 precedes production packaging changes. Missing signing credentials do not block identity or process work.
2. W02 and W03 establish the first functioning end-to-end path. Windows arm64 work must not block this Linux slice.
3. W04-W06 complete format/architecture coverage. W07 accepts only the complete final CLI lock and Desktop target set.
4. W08-W10 consume verified inputs. Source builds retain their separate source and output provenance.
5. Build and install acceptance can use local immutable artifact inputs. Public recipe URLs must exist and verify before submission.
6. W11 completes Desktop storage/update registration from the parent plan and destination orchestration before W12.
7. Stable Desktop release requires the locked CLI build to be stable in npm. A missing gate fails explicitly; never substitute another build.

Signing must preserve the accepted CLI byte identity. If a platform requires signatures on bundled CLI executables, complete that signing in the CLI candidate pipeline before accepting its manifest.
Do not re-sign an already locked CLI executable inside Desktop assembly.
An unsigned development package can establish construction or nonvisual lifecycle behavior only. It cannot satisfy signed-release acceptance.

## 7. Failure inventory

| Failure | Required observation |
|---|---|
| Missing/wrong CLI lock, changed binary or undeclared target | Fail candidate construction before a package or channel is accepted. |
| CLI/Desktop command files collide | Installation must reject the conflict or use owned private paths; never overwrite an independent installation. |
| Desktop needs a missing runtime library | Clean native installation exposes the missing dependency; do not accept a developer-machine-only launch. |
| Install, upgrade or uninstall stops an unrelated profile service | Lifecycle acceptance fails; user data and unrelated service ownership remain intact. |
| Same version/new build sorts incorrectly | Real package-manager upgrade tests fail before publication. |
| Portable data stays in a versioned installation directory | Relocation/upgrade test fails; Scoop persistence alone cannot establish runtime support. |
| Nix wrapper prevents sibling daemon resolution or autostart survives only until GC | Real profile/GC and declarative autostart acceptance fails. |
| Snap/Flatpak builds but blocks IPC, clipboard, local network or selected files | Capability acceptance remains failed/pending; no stable-channel activation. |
| Host updater mutates a package-managed installation | Update-ownership E2E fails; route the user to the owning package manager. |
| Signature, repository database or download digest is wrong | Fresh-install verification rejects the candidate. |
| Signing or promotion changes already accepted package bytes | Reject identity mismatch; promotion may not rebuild or re-sign. |
| Destination partially publishes or returns an uncertain response | Query exact identity, preserve per-destination state, resume the same inputs and never blindly replace bytes. |
| A stale workflow attempts to select an older build | Reject using expected destination state where supported; otherwise serialize and read back, documenting external-writer limits. |
| Public manifest contains a placeholder or mutable URL | Preflight fails; do not submit or call it installable. |
| Source-built channel has different output bytes | Require its own provenance/acceptance receipt; do not report upstream binary equality. |
| v1 data/identity is silently reused | Fail acceptance; migration remains explicit and peers must pair again. |

## 8. Verification and retained artifacts

Use real package builders, package managers and installed programs. Use isolated homes/profiles and disposable credentials.
Use the existing `e2e/release` runner for release orchestration and the existing Desktop E2E entry for authorized GUI acceptance.
Do not add unit tests after implementation. Add end-to-end cases at the actual package/install/service seams.
Do not launch a GUI, browser, application or VM for visual acceptance without explicit authorization.
Build-only and nonvisual process checks can proceed independently; record native visual cases as pending until authorized and run.

Each run produces `report.json`, `SHA256SUMS` and a replay entrypoint. Record exact source/CLI lock, tool versions, commands, exit codes, targets, package hashes, installed file ownership, service generation, capability assertions and cleanup result.
Use only fixture clipboard content and temporary data. Retain the small replayable receipt by default; remove temporary binaries, installed packages, private keys and payloads after verification.
Accepted release payloads belong in the durable immutable release store, separate from local acceptance evidence.
Real destination receipts additionally record the submission/build/revision ID, exact remote package identity, timestamp, digest where available and observed state.

### Required lifecycle scenarios

| Scenario | Required assertions |
|---|---|
| Fresh installation | Installed files and executable identities match the manifest; Desktop resolves its private service; no unrelated installation is modified. |
| v1 replacement | The same channel replaces its v1 program registration; v1 fixture data is unchanged; v2 uses its separate data root and does not import automatically. |
| v2 upgrade | A newer accepted build installs through the owner; fixture history/configuration survive; no duplicate owned service remains. |
| Alpha/stable switch | One installation identity remains; the selected accepted bytes are installed; user data survives; unsupported downgrade is rejected explicitly. |
| Standalone CLI coexistence | Test both installation orders and removal orders; public CLI commands and unrelated service/data ownership remain intact. |
| Portable/Scoop upgrade | Paths with spaces, relocation and version-directory replacement preserve the intended data and socket locations. |
| Development isolation | Development launch registration, runtime identity and data root differ from production; neither process replaces the other's data. |
| Update authority | Package-managed install cannot replace its owned files through the app updater; direct-download format selects only its accepted update format. |
| Uninstall | Only owned files, registrations and owned runtime processes are removed; user data remains; temporary test resources are cleaned. |
| Rejected/partial operation | Wrong digest/signature/lock, missing target, duplicate identity or partial destination operation yields a failed receipt and no false completion. |

Use synthetic v1 installation/data fixtures to develop replacement checks. Validate actual v1 installer replacement separately on an isolated authorized target before claiming v1 replacement acceptance.
Use real package-manager version comparisons for both increasing build number and increasing product version.
Do not treat a software-view or test-clipboard case as system clipboard or native installation acceptance.

### Check entrypoints

Existing entrypoints available before implementation:

```sh
python3 e2e/release/run.py
make desktop-e2e-list
make desktop-e2e-doctor
go run ./tools/docscheck
git diff --check
```

The Desktop doctor may report missing native prerequisites; listing or diagnosing cases is not execution acceptance.
Run selected nonvisual process cases independently. Run native cases through the canonical Desktop runner only after the required GUI authorization.
Each new package lifecycle entrypoint must document its exact replay command and disposable target requirements when implemented.

### Completion record

Keep a row per channel/target with `build`, `install`, `native`, `submission`, `approval` and `availability` results and a receipt reference.
Use explicit `pending`, `passed`, `failed` or `not_applicable` results; give a reason for pending/not-applicable entries.
Packaging completion requires passed build and installation results for the declared matrix plus required native capability results.
Public rollout completion additionally requires destination read-back and approval where applicable.
Any missing toolchain, native runner, signature, sandbox capability or external review blocks only its dependent result; continue independent authorized work.

## 9. Planning delivery receipt

The initial planning task recorded accepted requirements. Subsequent implementation adds the construction/renderer/acceptance entrypoints and expands the declared six-target matrix. Observed results and remaining gates are in the [implementation receipt](0018-v2-package-channel-receipt.md).
Desktop remains `2.0.0`, build `1`. The real production CLI lock remains unresolved. No hosted workflow or public publication was performed.
Document validation: `go run ./tools/docscheck` and `git diff --check`; record observed results in the task handoff.

## 10. Tool references

- [nFPM](https://nfpm.goreleaser.com/): maintained package-format tooling; use it for DEB/RPM instead of writing archive internals.
- [Homebrew Cask cookbook](https://docs.brew.sh/Cask-Cookbook): native app, URL, digest and uninstall definitions.
- [WinGet manifest guidance](https://learn.microsoft.com/en-us/windows/package-manager/package/manifest): native manifest generation and validation workflow.
- [Flatpak sandbox permissions](https://docs.flatpak.org/en/latest/sandbox-permissions.html): explicit resource access and portals; permissions require real acceptance.

Recheck channel policies and tool versions during implementation. These references do not establish channel approval or application compatibility.
