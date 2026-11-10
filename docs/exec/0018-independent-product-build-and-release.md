# Implementation plan: Independent product builds and releases

- Status: CLI native builds and package acceptance verified remotely; npm publication authorization pending. Desktop and mobile phases remain open.
- Date: 2026-11-10
- Decision: [0018 independent product releases](../decisions/proposed/0018-independent-product-build-and-release.md).
- Source baseline after rebase: `edad4381b8a080d583c7a558c517b2d671c8fdf3`, branch `cli-release-channels`.
- Delivery requirement: Four products, each with its own build and release workflows, plus a shared exact-build promotion workflow.
- Native interaction acceptance remains user-owned unless separately authorized.

## Current delivery

The CLI vertical slice is implemented. Five native target builds and npm candidate assembly passed [remote verification](0018-cli-remote-verification.md). It includes channel-free npm build versions, native executable identity and alias checks, package-set integrity checks, direct stable publication, exact-build promotion and durable candidate recovery. Local process acceptance is separate from public OIDC/provenance and remote native runner acceptance. The initial metadata remains a local seed, not a public release assignment.

See the [implemented release architecture](../architecture/releases.md), [release contract](../contracts/release-contract.md) and [local delivery receipt](0018-cli-implementation-receipt.md). Desktop signing, immutable CLI consumption and R2/FlareRelease publication are still open. Android/iOS native applications are still prerequisites. This delivery does not close decision 0018.

## 1. Verified starting point

| Area | Current evidence | Implementation implication |
|---|---|---|
| Shared CI | `.github/workflows/pr-checks.yml` | Retain existing quality/headless checks; add product-owned entries. |
| CLI/service | `app/service/cmd/unic`, `app/service/cmd/unicd` | Reuse the existing UI-free pair and process E2E. |
| Desktop | `app/desktop/cmd/unic-desktop` | Keep native integration separate from the service. |
| Packaging | `tools/package/main.go` | Existing native archives and hashes lack independent product versions, release orchestration and external immutable CLI consumption. |
| Android | `app/android/README.md`, `AGENTS.md` | No Gradle/native project exists yet. A runnable Android build is a prerequisite, not a YAML-only success. |
| iOS | `app/ios/README.md`, `AGENTS.md` | No Xcode/native project exists yet. A runnable iOS build is a prerequisite. |
| Shared identity | Engine and local service contracts | Separate app versions from protocol support and actual tested component combinations. |
| Existing local changes | Desktop UI/Kit/demo work is uncommitted | Preserve it; do not absorb it into release-infrastructure changes. |

The current checkout name does not establish the repository's default branch or signing/store configuration.
Inspect those live settings only when implementing or operating the corresponding stage.

## 2. Required workflow inventory

| File under `.github/workflows/` | Triggers | Responsibility |
|---|---|---|
| `build-android.yml` | Relevant PR/default-branch push, manual, reusable call | Build/test Android and its Engine binding; return candidate identities. |
| `release-android.yml` | Manual | Validate Android version/build identity, call Android build, verify/sign/stage/publish selected destinations. |
| `build-ios.yml` | Relevant PR/default-branch push, manual, reusable call | Verify simulator target and construct explicitly requested device candidates. |
| `release-ios.yml` | Manual | Validate iOS native identity, call iOS build, select/upload exact distribution build and record processing/availability. |
| `build-cli.yml` | Relevant PR/default-branch push, manual, reusable call | Build/test the UI-free pair for the declared OS/architecture matrix. |
| `release-cli.yml` | Manual | Call CLI build, validate exact candidates, publish immutable npm platform/main packages and the selected dist-tag. |
| `build-desktop.yml` | Relevant PR/default-branch push, manual, reusable call | Build native desktop targets and verify the locked CLI pair and packaging layout. |
| `release-desktop.yml` | Manual | Call Desktop build, finish required signing/notarization, publish `desktop/v...` and available target pointers. |
| `promote.yml` | Manual | Select product/version/build number and targets; CLI updates npm dist-tags, Desktop/mobile reuse exact assets or store identities. |

Reusable implementation helpers are allowed. They are not substitutes for any of these eight entries.
Release workflows use `workflow_call` to await their own builds at the resolved workflow/source revision.
Do not use a repository-global latest artifact, an asynchronous tag-trigger cascade, or a successful unrelated workflow as candidate selection.

### Confirmed channel requirements

- Use only `alpha` (internal development testing) and `stable` (formal release), including CLI.
- Identify each build by product, version and build number. Multiple builds may share one version. Channel names never enter the version.
- Allow release workflows to publish directly to stable. Alpha is not a mandatory predecessor.
- Promotion selects one exact alpha build. File destinations receive the original final bytes after download and verification; managed stores reuse their existing build identity.
- Stable Desktop publication, through either path, requires its exact bundled CLI build to be available in stable for every selected target.
- Both channels share application/package identity, installation location, service identity and default data directory. Channel switching replaces the installation and preserves data.
- Record channel selection outside immutable payload bytes. Do not implicitly downgrade a newer installed alpha build when selecting stable.
- Keep recommendations in decision section 11 distinguishable from confirmed requirements until resolved.
- CLI uses npm for package and channel management. Do not add CLI registration or channels to FlareRelease.
- GitCode implementation is deferred and is not a completion gate for this task.

## 3. Smallest complete implementation sequence

### P0. Freeze identities and failure inventory

- [ ] Complete the [existing-infrastructure integration gates](0018-release-infrastructure-audit.md), including Desktop/mobile FlareRelease changes and npm OIDC authorization. Mirror implementation is deferred.
- [ ] Verify actual default branch, supported native toolchains, product IDs, bundle/application IDs and intended destinations.
- [ ] Define the initial public target matrices and required gates. Do not infer runtime support from cross-compilation.
- [ ] Define the `release/products.toml` schema and single-source native/Go version injection.
- [ ] Include CLI/Desktop build numbers and finalize allocation, candidate ordering, tag format and native mappings using decision section 11.
- [ ] Define immutable candidate-manifest and publication-receipt schemas.
- [ ] Select durable build storage and separate alpha/stable records; define channel-switch persistence and no-newer-build behavior.
- [ ] Define channel-free npm version/build-number encoding and alpha/stable dist-tag mapping; verify publish and dist-tag OIDC permissions for the actual workflow identities.
- [ ] Define Desktop CLI lock resolution and installation ownership when standalone CLI already exists.
- [ ] Map each failure mode in section 5 to process-level acceptance before implementing orchestration.

Exit artifact: reviewed metadata examples, manifest/receipt examples and a target/gate matrix with no fake release success states.

### P1. Complete CLI build-to-release vertical slice

- [x] Extend existing packaging to identify only the CLI product when requested and include its version in names and executable metadata.
- [x] Add `build-cli.yml` with declared OS/architecture targets, dependency checks and production headless process acceptance.
- [x] Add `release-cli.yml` to resolve one source SHA, call the build, validate matching artifacts and stage a product-scoped release.
- [x] Include version and build number in CLI metadata, archives and immutable tags; publish fixtures to both alpha and direct stable paths.
- [x] Reuse main's npm main/platform-package pattern with exact dependencies, registry integrity, provenance and the new binary names.
- [x] Keep `uniclip` as an alias of `unic`: a relative symlink in Unix archives, an equivalent Windows launcher, and both npm bin names pointing to the same launcher. Verify actual extraction/install and argument/exit-code forwarding.
- [x] Add the CLI vertical slice of `promote.yml`: verify the exact existing npm build and platform set, update stable dist-tags, read back integrity and retain alpha. Do not re-upload tarballs or call FlareRelease.
- [x] Exercise retries, manifest verification and destination read-back against a disposable local publication fixture.
- [ ] Enable real publication only after its exact candidates and destination configuration have passed the required checks.

Exit artifact: CLI candidate manifest, binary/asset hashes, replay command and a publication receipt that distinguishes staged from publicly available.
The existing working CLI/Desktop paths remain usable during later phases.

### P2. Complete Desktop with an immutable CLI dependency

- [ ] Add the checked-in CLI version/build-number/manifest and npm identity/integrity lock; verify each fetched platform package before extraction.
- [ ] Add `build-desktop.yml` and `release-desktop.yml` with Windows, macOS and Linux target ownership.
- [ ] Package the GUI with the exact locked CLI/service pair. Do not rebuild or relabel that pair.
- [ ] Implement installation/service ownership rules, version reporting and the installed-layout integration test.
- [ ] Add platform signing/notarization/export steps before final hashes and staged publication.
- [ ] Extend promotion to Desktop without changing its package or bundled pair. Gate both stable paths on the exact CLI build's stable availability.
- [ ] Verify shared alpha/stable installation identity and data preservation through installation-process E2E; keep native interaction acceptance separate.
- [ ] Record native interaction/installation acceptance separately; do not launch GUI or a VM without authorization.

Exit artifact: per-target installer/package identities, component hashes proving CLI reuse, integration receipts and explicit native acceptance status.

### P3. Complete the Android product

- [ ] Establish the native Android application and binding build under its existing application boundary.
- [ ] Consume the Android metadata record for `versionName`/`versionCode`; validate reserved identity before building.
- [ ] Add `build-android.yml` and `release-android.yml` with separate direct-download and Play destinations.
- [ ] Verify signed APK/AAB identities, upgrade behavior and selected ABI content through product E2E.
- [ ] Record upload, processing and availability independently for each selected destination.
- [ ] Extend promotion to Android destinations using identical signed direct-download assets or the existing store build identity as applicable.

Exit artifact: buildable application, signed candidate manifest, acceptance receipt and destination IDs/states.
An empty project or a placeholder artifact cannot satisfy this phase.

### P4. Complete the iOS product

- [ ] Establish the native iOS application and binding build under its existing application boundary.
- [ ] Consume the iOS metadata record, validate native marketing/build mapping and reserve the build identity.
- [ ] Add `build-ios.yml` and `release-ios.yml`; distinguish simulator validation from distribution-device output.
- [ ] Verify signed archive/export identity, application metadata and product E2E.
- [ ] Record the exact App Store Connect build and TestFlight/App Store state; selecting a build for review does not imply availability.
- [ ] Extend promotion to select the exact uploaded alpha build for formal submission without a duplicate upload or rebuild.

Exit artifact: buildable application, device-candidate manifest, acceptance receipt and independently observed store states.

### P5. Complete channels and operational closure

- [ ] Implement product/channel/target/destination pointers with expected-prior-value checks.
- [ ] Verify platform-only patches leave unselected targets and other products unchanged.
- [ ] Verify retries and forward recovery after partial external publication.
- [ ] Verify shared application/data identity across channel switches, including newer installed alpha builds and unavailable stable updates.
- [ ] Write implemented behavior into `docs/architecture/releases.md` and exact schemas into `docs/contracts/release-contract.md`.
- [ ] Update product READMEs/AGENTS, perform the documentation lifecycle checks, and archive 0018 only after its implementation/acceptance is complete.

## 4. Change routing

| Changed source | Build/verification obligations | Release effect |
|---|---|---|
| `engine/`, shared `internal/`, `plugin/`, root Go module/lock inputs | All consuming products; preserve CLI UI-dependency checks | No automatic product bump or publication |
| `app/service/` | CLI build and Desktop IPC/component compatibility verification | Only an explicit CLI release request publishes CLI |
| `app/desktop/` | Desktop build/verification | No mobile or standalone CLI release |
| `app/android/` | Android build/verification | No iOS/Desktop/CLI release |
| `app/ios/` | iOS build/verification | No Android/Desktop/CLI release |
| Shared future mobile binding implementation | Both consuming mobile products | No implicit mobile release |
| Release scripts/workflows/metadata | Affected product build and orchestration acceptance | No publication merely because CI ran |
| Documentation only | Documentation validation | No product publication |

Use admission/routing jobs so required checks complete meaningfully when a product is unaffected.
Do not leave required checks indefinitely pending through inappropriate whole-workflow path filtering.
Before native projects exist, affected mobile rows report an explicit prerequisite status and cannot satisfy a mobile release gate.

## 5. Failure inventory and process acceptance

| Scenario | Required result |
|---|---|
| A product version changes | Other product versions and pointers remain unchanged. |
| A shared Engine change lands | Affected products are verified; no tag or upload appears automatically. |
| Dirty or moving release source | Release rejects it; source SHA and workflow revision are fixed before construction. |
| Wrong workflow/run/target artifact | Manifest verification rejects it before publication. |
| Modified, missing or differently signed payload | Hash/signature checks reject it; no stable pointer advances. |
| Desktop lock references unpublished or mismatched CLI | Fail preflight; do not substitute a newly built CLI. |
| Desktop installer runs beside standalone CLI | One service owns a state directory; uninstall preserves unrelated service ownership and user data. |
| Two releases reserve the same native identity | Serialize/reject conflict; a different build never reuses that reservation. |
| Android/iOS retry after an uncertain upload | Query the exact store identity; do not blindly re-upload or claim availability. |
| One selected platform or destination fails | Keep its prior pointer; report partial state; do not silently shrink the selected matrix. |
| Existing product tag points elsewhere | Fail; do not force-move or delete the published tag. |
| A stale job promotes after a newer release | Expected-prior-pointer check prevents regression. |
| Candidate retry with identical identity | Resume from matching immutable payloads and receipts. |
| Two builds share a product version | Version plus build number selects one exact manifest; a version-only promotion request is rejected. |
| Direct stable release has no alpha record | Permit it after the same required stable checks. |
| Promotion source is missing, expired or modified | Fail without rebuilding, replacing assets or advancing stable pointers. |
| Alpha promotion changes signature, package bytes or bundled CLI | Reject the altered candidate; promotion must preserve the accepted build. |
| Desktop bundles an alpha-only CLI build | Block both direct stable release and promotion until that exact CLI build is stable on the selected targets. |
| Channels are switched on an existing installation | Preserve application identity and data; do not create a second channel daemon or implicitly downgrade. |
| Same build appears in both channels | Reuse immutable build identity and payload hashes; retain separate channel receipts. |
| Store promotion is submitted but still under review | Keep availability pending; do not claim stable distribution is complete. |
| FlareRelease still has a product/version-only key or rejects alpha-to-stable | Block Desktop/mobile activation; CLI remains owned by npm. |
| One channel rolls back a build still used by another channel | Keep the other channel's downloads available; distinguish pointer movement from global withdrawal. |
| R2 has the same key with different bytes | Reject the upload; do not overwrite an accepted build. |
| CLI promote selects a different npm package set or integrity | Reject it before changing stable dist-tags; never rebuild or republish the selected package. |
| An installation exposes the old command name | `uniclip` invokes the same `unic` implementation with identical arguments and exit codes; moving the installation does not break the alias. |
| npm publish/dist-tag trust does not cover the invoked workflow | Fail explicitly; do not introduce a token fallback or assume old workflow trust covers the new workflow. |
| GitCode mirroring is requested during this implementation | Report it as deferred; do not extend or activate mirror infrastructure. |
| A product release is listed on the shared GitHub Releases index | Other products' records, versions and channel pointers remain independent. |
| Native project/SDK/signing prerequisite absent | Report the actual missing prerequisite; do not publish a stub. |
| Attempted channel rollback | Verify compatibility and native update constraints; distinguish pointer rollback from installed-client recovery. |

Use real build/package processes and a local fake publication/store endpoint for orchestration faults.
A fake endpoint proves orchestration only; it does not prove real signing, Store upload, native behavior or public availability.
Do not add unit tests after implementation. Use the existing process E2E approach and product-native E2E at their real seams.

## 6. Acceptance artifact requirements

Each acceptance run retains a replayable artifact with exact commands/exit codes, product/version/build identity,
source/build-input hashes, toolchain/SDK/workflow identity, selected target matrix, final payload checksums,
assertion results and per-destination receipts. Use only disposable fixture identities in local orchestration tests.
A release additionally reads back its real tag SHA, uploaded asset hashes and destination identity/state.
Never report a dispatched workflow, a draft release, an uploaded build or an externally processing build as a completed public release.

## 7. Documentation delivery receipt

This change writes the accepted decision and this implementation plan, and registers 0018 in the two decision indexes.
It creates no workflow files, version metadata, native projects, signing configuration, release tags, Store uploads or update channels.
No code build, native acceptance, hosted workflow or publication is claimed by this documentation task.
The channel discussion adds the confirmed requirements above and leaves allocation, ordering, storage and switch-policy recommendations explicit in the decision.
Validation: run `go run ./tools/docscheck` and `git diff --check` after registering these pages.

## 8. Archive command alias receipt

The existing archive builder now includes `uniclip` as an alias of `unic` in both headless and desktop distributions.
Unix tar archives preserve a relative symbolic link. Windows ZIP archives contain `uniclip.cmd`, which forwards arguments to the sibling `unic.exe` and returns its exit code.
Package manifests record alias targets separately from executable hashes and include the packaging source in source identity.
See the [current headless architecture](../architecture/headless.md).

The pre-change production packaging check failed because `uniclip` was absent.
Post-change process acceptance passed on macOS arm64 for both headless and desktop archives: extraction to a directory containing spaces, relative-link preservation, relocation, identical help/error output, and exit codes 0, 1 and 2.
Windows amd64 cross-packaging and ZIP/PE/hash/launcher inspection passed. Native Windows execution and Linux native execution were NOT RUN.
No GUI or service was launched. No unit tests were added.

Local repeatable evidence is retained in `.cache/packaging-alias/`: `before/report.json`, `after/report.json`, `replay.py`, `failure-inventory.md`, `README.md` and `SHA256SUMS`.
The reports bind the tested source, binary/archive hashes and exact process commands. Generated binaries and archives were removed after acceptance.
Replay from the repository root:

```sh
GOCACHE=/private/tmp/unic-0018-docs-go-cache python3 .cache/packaging-alias/replay.py --root "$PWD" --artifacts /private/tmp/unic-alias-replay --desktop --windows
```

`go vet ./tools/package` and `git diff --check` passed.
The required headless architecture `-bump` was attempted and rejected because its source paths are uncommitted; its original marker remains unchanged.
The npm dual-command launcher remains part of future P1 implementation. This archive change does not implement npm publication or the wider release system.
