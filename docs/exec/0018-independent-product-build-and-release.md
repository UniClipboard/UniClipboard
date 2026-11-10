# Implementation plan: Independent product builds and releases

- Status: Planned; no workflow or release-system implementation in this documentation change.
- Date: 2026-11-10
- Decision: [0018 independent product releases](../decisions/proposed/0018-independent-product-build-and-release.md).
- Source baseline: `fd77245cb00fd9efb7a77688ba45e0f67d0d1d8e`, branch `v2`.
- Delivery requirement: Four products, each with its own build workflow and release workflow.
- Native interaction acceptance remains user-owned unless separately authorized.

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
| `release-cli.yml` | Manual | Call CLI build, validate exact candidates, publish `cli/v...` and immutable assets. |
| `build-desktop.yml` | Relevant PR/default-branch push, manual, reusable call | Build native desktop targets and verify the locked CLI pair and packaging layout. |
| `release-desktop.yml` | Manual | Call Desktop build, finish required signing/notarization, publish `desktop/v...` and available target pointers. |

Reusable implementation helpers are allowed. They are not substitutes for any of these eight entries.
Release workflows use `workflow_call` to await their own builds at the resolved workflow/source revision.
Do not use a repository-global latest artifact, an asynchronous tag-trigger cascade, or a successful unrelated workflow as candidate selection.

## 3. Smallest complete implementation sequence

### P0. Freeze identities and failure inventory

- [ ] Verify actual default branch, supported native toolchains, product IDs, bundle/application IDs and intended destinations.
- [ ] Define the initial public target matrices and required gates. Do not infer runtime support from cross-compilation.
- [ ] Define the `release/products.toml` schema and single-source native/Go version injection.
- [ ] Define immutable candidate-manifest and publication-receipt schemas.
- [ ] Define Desktop CLI lock resolution and installation ownership when standalone CLI already exists.
- [ ] Map each failure mode in section 5 to process-level acceptance before implementing orchestration.

Exit artifact: reviewed metadata examples, manifest/receipt examples and a target/gate matrix with no fake release success states.

### P1. Complete CLI build-to-release vertical slice

- [ ] Extend existing packaging to identify only the CLI product when requested and include its version in names and executable metadata.
- [ ] Add `build-cli.yml` with declared OS/architecture targets, dependency checks and production headless process acceptance.
- [ ] Add `release-cli.yml` to resolve one source SHA, call the build, validate matching artifacts and stage a product-scoped release.
- [ ] Exercise retries, manifest verification and destination read-back against a disposable local publication fixture.
- [ ] Enable real publication only after its exact candidates and destination configuration have passed the required checks.

Exit artifact: CLI candidate manifest, binary/asset hashes, replay command and a publication receipt that distinguishes staged from publicly available.
The existing working CLI/Desktop paths remain usable during later phases.

### P2. Complete Desktop with an immutable CLI dependency

- [ ] Add the checked-in CLI version/manifest lock and verify each fetched target archive before extraction.
- [ ] Add `build-desktop.yml` and `release-desktop.yml` with Windows, macOS and Linux target ownership.
- [ ] Package the GUI with the exact locked CLI/service pair. Do not rebuild or relabel that pair.
- [ ] Implement installation/service ownership rules, version reporting and the installed-layout integration test.
- [ ] Add platform signing/notarization/export steps before final hashes and staged publication.
- [ ] Record native interaction/installation acceptance separately; do not launch GUI or a VM without authorization.

Exit artifact: per-target installer/package identities, component hashes proving CLI reuse, integration receipts and explicit native acceptance status.

### P3. Complete the Android product

- [ ] Establish the native Android application and binding build under its existing application boundary.
- [ ] Consume the Android metadata record for `versionName`/`versionCode`; validate reserved identity before building.
- [ ] Add `build-android.yml` and `release-android.yml` with separate direct-download and Play destinations.
- [ ] Verify signed APK/AAB identities, upgrade behavior and selected ABI content through product E2E.
- [ ] Record upload, processing and availability independently for each selected destination.

Exit artifact: buildable application, signed candidate manifest, acceptance receipt and destination IDs/states.
An empty project or a placeholder artifact cannot satisfy this phase.

### P4. Complete the iOS product

- [ ] Establish the native iOS application and binding build under its existing application boundary.
- [ ] Consume the iOS metadata record, validate native marketing/build mapping and reserve the build identity.
- [ ] Add `build-ios.yml` and `release-ios.yml`; distinguish simulator validation from distribution-device output.
- [ ] Verify signed archive/export identity, application metadata and product E2E.
- [ ] Record the exact App Store Connect build and TestFlight/App Store state; selecting a build for review does not imply availability.

Exit artifact: buildable application, device-candidate manifest, acceptance receipt and independently observed store states.

### P5. Complete channels and operational closure

- [ ] Implement product/channel/target/destination pointers with expected-prior-value checks.
- [ ] Verify platform-only patches leave unselected targets and other products unchanged.
- [ ] Verify retries and forward recovery after partial external publication.
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
Validation: run `go run ./tools/docscheck` and `git diff --check` after registering these pages.
