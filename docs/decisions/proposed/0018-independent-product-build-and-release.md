# 0018: Independent product builds and releases

- Status: Accepted design; implementation pending.
- Date: 2026-11-10
- Requirement: Android, iOS, CLI and Desktop each have a dedicated build workflow and a dedicated release workflow.
- Execution plan: [0018 implementation plan](../../exec/0018-independent-product-build-and-release.md).
- Existing boundaries: [repository overview](../../architecture/overview.md), [headless architecture](../../architecture/headless.md), and [local service contract](../../contracts/local-service-contract.md).
- Intended landing pages after implementation: `docs/architecture/releases.md` and `docs/contracts/release-contract.md`.

## 1. Context and scope

The monorepo shares Engine and protocol code, but its products have different users, delivery channels and release schedules.
A shared repository does not require a shared product version.
CLI and Desktop already have separate entry points. Desktop packages currently reuse CLI/service binaries built in the same invocation.
Android and iOS currently have documentation placeholders, not native application projects.
The repository has PR checks and a local archive builder. It does not yet implement this release system.

This decision defines product identity, version ownership, build/release responsibilities and artifact provenance.
It does not implement native clients or change the desktop integration behavior.
Implementation progress and acceptance receipts belong in the execution plan.

## 2. Four independently versioned products

| Product ID | Payload | Example version | Example release tag |
|---|---|---|---|
| `android` | Android application, embedded Engine binding, APK/AAB | `0.8.0` | `android/v0.8.0` |
| `ios` | iOS application, embedded Engine binding, device archive/export | `0.6.0` | `ios/v0.6.0` |
| `cli` | `unic` and `unicd` | `1.2.0` | `cli/v1.2.0` |
| `desktop` | `unic-desktop`, bundled CLI and service, platform packaging | `1.5.0` | `desktop/v1.5.0` |

Versions above are examples, not initial release assignments.
Each product owns its version, changelog, release tags, publication state and stable/beta channel pointers.
A product release does not automatically bump or release another product.
`unic` and `unicd` share the CLI product version and ship as one tested pair.

Windows, macOS and Linux are target variants of CLI and Desktop.
Their OS, architecture and package format do not create additional product version sequences.
A platform-only desktop patch uses the next Desktop version with an explicitly selected target set.
Other targets may keep their previously available version until a later release.
A failed target is not silently removed from the selected release target set.

Engine remains shared source inside this monorepo, not a fifth application release line.
Every application manifest records its Engine/binding source identity.
A separately published SDK can define its own release contract when an actual SDK consumer requires it.
Product versions, local IPC versions, network protocol versions and database formats remain distinct concepts.
Independent product version numbers do not prove interoperability.
The current pre-release project rule remains in force: no legacy compatibility layer is introduced, and protocol version stays 1.
Before supporting independently installed public versions, implementation must define and test the supported IPC/network capability combinations.

## 3. Version metadata and native build identities

Use one checked-in metadata source, `release/products.toml`, with four product records.
Each record owns its product version. Android and iOS also own separate positive build counters.
Desktop records an immutable CLI dependency lock by release version and manifest digest.
Native project files and Go version variables consume or generate values from this source; they are not independently edited version authorities.
Generated output manifests describe a build and never become a second editable source of product versions.

A release-preparation change selects the product version and reserves any native build counter before choosing the immutable source commit.
Counters may contain gaps. A reserved build identity is never reassigned to different bytes.
Counters advance independently for Android and iOS, across their respective channels.
GitHub workflow `run_number` and rerun attempt numbers are provenance fields, not native version authorities.
A preflight checks existing tags, manifests and store build identities before creating a new candidate.

Android maps the product version to `versionName` and its reserved counter to `versionCode`.
Android requires a higher `versionCode` for successive updates; its user-facing version is separate.
See [Android versioning](https://developer.android.com/studio/publish/versioning).

iOS records the product release version, native marketing version and native build number separately.
A prerelease suffix belongs in the release identity; native marketing/build fields must satisfy Apple's formats.
The mapping must be deterministic and validated before archive creation.
A stable candidate and a beta release with different product identities are not interchangeable uploads.
A new upload gets a new reserved build identity. Reusing an already uploaded build for review/promotion selects that exact build rather than uploading replacement bytes.
See [Apple build selection](https://developer.apple.com/help/app-store-connect/manage-builds/choose-a-build-to-submit/).

Tags have product prefixes. A published tag never moves to another source commit.
The release workflow is the only tag-creation owner; tag pushes do not trigger a second publication path.
GitHub Releases remain in this repository, each associated with its product tag and product-scoped notes.
See [GitHub releases](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases).

## 4. Eight first-class workflow entry points

All names below are target files under `.github/workflows/`; they do not currently exist.

| Product | Build workflow | Release workflow |
|---|---|---|
| Android | `build-android.yml` | `release-android.yml` |
| iOS | `build-ios.yml` | `release-ios.yml` |
| CLI | `build-cli.yml` | `release-cli.yml` |
| Desktop | `build-desktop.yml` | `release-desktop.yml` |

Do not replace these entry points with one generic workflow selected by a product flag.
Shared scripts or reusable jobs may remove proven duplication while preserving these independent entries.
Existing PR checks remain shared quality checks; they do not replace any product build or release workflow.

### Build workflows

Each build workflow supports product-relevant PR/default-branch builds, `workflow_dispatch` and `workflow_call`.
The repository's actual default branch must be verified during implementation; the current checkout name is not its authority.
Each run resolves an immutable source SHA and an explicit target matrix before building.

Build workflows compile, perform applicable E2E checks, package candidates and produce an artifact manifest.
Ordinary PR builds use no distribution secrets and create no release tags, store uploads or channel changes.
A trusted release caller may request distribution signing/export as part of candidate construction.
Signing credentials are limited to that product and trusted invocation; a fork PR cannot select this mode.
A public-release candidate must come from a clean checkout of the selected SHA.
Development builds may record dirty source identity, but cannot be promoted to public releases.

### Release workflows

Each release workflow supports `workflow_dispatch` with source ref, product version, channel and selected targets/destinations.
Version and build identity must match the checked-in metadata at the resolved source SHA.
A release calls its own build workflow through `workflow_call` at the same immutable revision.
It consumes the returned exact run/artifact identities, rather than searching for the latest successful build.
GitHub supports named inputs and secrets for reusable workflows; see [workflow reuse](https://docs.github.com/en/actions/how-tos/reuse-automations/reuse-workflows).

The release workflow validates manifests, staged payloads, signing/notarization and destination-specific requirements.
It creates the product tag and GitHub Release only after all selected candidate targets pass their required gates.
It then records each destination result and promotes only verified available channel targets.
An unsigned verification build is not silently treated as a distribution-signed candidate.
A public release does not rebuild accepted payloads during upload or channel promotion.
Checksums identify the final distributable bytes after signing/notarization/export has completed.

Serialize publication by product, including all of that product's channels and native build reservations.
Do not cancel an in-progress release when another release of the same product is requested.
Different products may release concurrently.
Resuming the same product/version/build identity reuses matching artifacts and receipts.
A conflicting source SHA or payload digest stops publication rather than overwriting it.

## 5. Artifact and target ownership

| Product | Candidate/verification outputs | Distribution outputs |
|---|---|---|
| Android | Native build outputs, binding identity, device/allowed automated acceptance receipt | Signed APK for direct distribution; signed AAB for Play distribution |
| iOS | Simulator verification output plus separately identified device archive/export | Distribution-signed device payload and App Store Connect/TestFlight receipts; IPA only where the distribution method allows it |
| CLI | UI-free `unic`/`unicd` pair and production headless E2E receipt | Versioned OS/architecture archives and checksums |
| Desktop | Native GUI, bundled CLI identity, packaging/integration acceptance receipt | Signed Windows installer, signed/notarized macOS package, and declared Linux package/archive formats |

The implementation selects and checks in required target matrices before enabling public releases.
CLI target candidates are Windows/macOS/Linux, each with amd64/arm64.
Desktop target candidates cover Windows/macOS/Linux with only toolchain- and runtime-verified architectures enabled.
Mobile ABI, deployment-target and native SDK settings belong to their real application projects; this decision does not invent them.
A target declaration is not a claim of existing native support.
An unimplemented application or unsupported declared target fails clearly, rather than uploading a placeholder artifact or reporting a successful release.

Names include product, product version, OS and architecture where applicable, for example:

```text
unic-cli-1.2.0-linux-arm64.tar.gz
unic-cli-1.2.0-windows-amd64.zip
unic-desktop-1.5.0-macos-arm64.dmg
unic-android-0.8.0-42.apk
unic-android-0.8.0-42.aab
unic-ios-0.6.0-17-device.xcarchive.zip
release-manifest.json
SHA256SUMS
```

A CI artifact is a temporary transfer object. Public release payloads and their manifests must have durable immutable storage.
A cache is never a release authority. A repository-wide "latest release" is never a product download selector.

## 6. Desktop bundles lock an existing CLI release

Desktop and CLI versions may differ, for example Desktop `1.5.0` with CLI `1.2.0`.
The Desktop release locks a published CLI release and its manifest digest.
For each selected target it fetches the exact CLI archive, verifies its digest and reuses the `unic`/`unicd` bytes.
It must not rebuild current CLI source and label it as the locked CLI version.
If the required CLI version is not published, release CLI first; Desktop fails preflight until that dependency is available.
This is an explicit dependency, not a cross-product auto-release chain.

Desktop acceptance verifies the actual installer layout, service ownership and installed GUI-to-daemon IPC behavior with the locked pair.
`unic-desktop` has its own product version. Bundled `unic`/`unicd` report their CLI version.
The final bundle manifest records every component's version, source commit and payload hash.
Installers prevent duplicate daemon ownership when standalone CLI and Desktop installations coexist.
Replacing a user-installed service requires an explicit ownership/version policy; uninstall must not remove another installation's service or user data.
The packaging implementation must define that policy before distribution acceptance.

## 7. Manifest and compatibility records

Each immutable candidate manifest includes:

- Product ID, product version, channel intent, and Android/iOS native version/build mapping where applicable.
- Source SHA, complete build-input identity, clean-source assertion, workflow revision, run ID and attempt.
- OS, architecture/ABI, toolchain/SDK versions, selected required targets and destinations.
- Final asset names, sizes, SHA-256 hashes, durable artifact identity and signature/notarization result.
- Component versions/source identities, including bundled CLI and embedded Engine/bindings.
- Local IPC/network protocol identifiers and the compatibility/integration acceptance receipt.
- Applicable automated/native acceptance evidence; unrun checks are explicit and cannot satisfy a required gate.

A separate publication receipt references that immutable manifest and records external destination IDs and observed states.
Uploading or promoting a release does not rewrite the immutable build manifest.
Native/store identities, signed installer identity and installed-file identity may differ; record the relevant stage rather than assuming one hash covers them all.

## 8. Channels and partial publication

Resolve updates by `(product, channel, OS, architecture/ABI, destination)`.
Only relevant selectors are used for a product; iOS store delivery is not exposed as a generic raw-binary download.
Each pointer references an immutable available release manifest and destination identity.
A platform-only patch updates only its selected platform pointers.
Other platform versions and all other products keep their existing pointers.

Track stages separately: source selected, built, verified, signed/exported, staged, uploaded, destination available, and channel promoted.
A queued workflow is not a build. A tag is not a successful upload. An upload is not Store/TestFlight availability.
Every selected target/destination has its own receipt, including failure, pending external processing and uncertain outcomes.
Public availability can be partial; it is not reported as a single all-platform success.
A failed or uncertain target never advances its pointer.
Channel updates compare the expected prior pointer to avoid an older job overwriting a newer release.

A rollback changes a pointer only to a compatible previously verified release.
It does not reverse installed data changes or downgrade installed mobile applications.
Native build-number rules still apply; repair installed clients through a valid forward update when downgrade is unavailable.

## 9. Alternatives and tradeoffs

- A repository-wide version/tag is simpler, but couples unrelated client schedules and hides which product changed.
- Independent versions for every OS add release sequences without a separate product boundary. Per-target availability solves platform-only delivery without that extra version ownership.
- One universal build/release entry reduces filenames, but hides product ownership and contradicts the requested independent workflows.
- Rebuilding a bundled CLI from Desktop HEAD simplifies assembly, but breaks the identity of an independently published CLI version.
- Sharing immutable CLI artifacts adds a dependency preflight and compatibility check. It preserves component provenance and the UI-free CLI boundary.

## 10. Documentation boundary

This is a target design. It does not claim that workflow files, native applications, version injection, store delivery or channel infrastructure exist.
After implementation, move current behavior and exact schemas into the intended architecture/contract pages before archiving this decision.
