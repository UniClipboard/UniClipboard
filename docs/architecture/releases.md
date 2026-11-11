---
paths:
  - .gitattributes
  - release
  - tools/package
  - tools/release
  - internal/releaseinfo
  - app/service/cmd/unic
  - internal/localpaths
  - internal/v1migration
  - e2e/release
  - .github/workflows/build-cli.yml
  - .github/workflows/release-cli.yml
  - .github/workflows/promote.yml
verified: 5e6d74552d5f5197d6f6435ba4a4d4e8a728043e
---
# Product builds and CLI releases

## Current implementation

`release/products.toml` owns product versions and build numbers. The implemented release product is CLI. Desktop, Android and iOS records reserve independent metadata; they do not establish release support.

`go run ./tools/package -product cli` builds `unic` and `unicd` for the current target. The versioned archive contains those programs and the `uniclip` alias. Unix archives preserve the relative symlink. Windows archives provide `uniclip.cmd`. Development packaging through `make package-headless` and `make package-desktop` remains available and unsigned.

All runtime programs handle `--version` and `--version --json` before initialization. Release builds embed version, build number and source SHA. Development builds report `dev` unless packaging supplies metadata.

CLI assembly uses the existing npm main/platform-package model. `@uniclipboard/cli` pins every optional platform package to the exact `<version>-build.<number>` registry version. Both npm CLI command names use one launcher. The package also exposes `unicd` through a dedicated service entrypoint. The launcher executes the real CLI beside its sibling service. Alpha selects the npm `alpha` dist-tag. Stable selects `latest`. Channel names do not enter package versions.

The declared CLI matrix covers macOS arm64/amd64, Linux arm64/amd64 and Windows amd64. Each target runs its own executable identity acceptance. Linux amd64 also runs production headless and packaged release/upgrade process acceptance. Remote native acceptance passed for all five targets in run 42777109814. The [remote verification receipt](../exec/0018-cli-remote-verification.md) records exact source, artifacts and the observed headless retry.

## v1 upgrade notice and migration

The CLI checks known v1 paths only when stderr is a terminal and `--json` is absent. It performs directory stat calls; it does not open credentials or a database. A default v1 directory or the active `UC_PROFILE` directory triggers the warning. Missing paths and regular files remain silent. Custom and portable profiles outside these locations require explicit `--source` selection.

The warning explains v1 incompatibility and points to migration help, preflight and import commands. It does not request acknowledgement or migrate automatically. `unic migrate` and `unic migrate --help` print a read-only guide before opening any profile.

CLI archives and npm platform packages include the migration importer within `unic`. Explicit migration runs in-process without starting the service. The importer keeps source data unchanged, verifies reopened target content and resumes by record identity. Partial migration returns 3. All peers must use v2 and pair again; settings, identities and trust are not imported.

The packaged-process E2E uses a temporary home, pseudoterminals and fixed encrypted Rust fixtures. It checks fresh-install silence, directory detection, JSON output, offline help, read-only preflight, partial results, source hashes and idempotent repair/retry. It never reads a personal v1 profile or system credential.

## Build, publication and promotion

Git attributes require LF checkout bytes for release TOML metadata on every platform. Metadata digests cover the original file bytes. The real Git checkout regression enables `core.autocrlf=true` and verifies that checkout preserves the canonical metadata bytes before building candidates.

`build-cli.yml` runs for PRs, main/v2 pushes, manual requests and reusable calls. Its callers provide an exact source SHA. It checks the metadata digest, source identity, required target set and native executable receipts before `npm pack`. CI builds do not publish.

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
| `tools/package` | Go build, archive aliases and source/binary manifests. |
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

The selected next CLI version is `2.0.0 (1)`, encoded as `2.0.0-build.1` on npm. Alpha selects the `alpha` dist-tag. Other products retain their independent local candidate seeds. The v1-to-v2 npm tag transition is implemented. Remote five-target build and package assembly acceptance passed. Workflow authorization remains a publication prerequisite. Public OIDC/provenance and real GitHub release publication remain unverified.

Desktop does not yet consume the immutable CLI lock or provide signed installers, R2 uploads or FlareRelease registration. Its stable CLI gate remains a required future implementation. Android and iOS lack native application projects. Their workflows, signing, Store acceptance and promotion are not implemented. GitCode is deferred. Do not archive decision 0018 while these phases remain open.

Exact interface requirements are in the [release contract](../contracts/release-contract.md). Process ownership remains in the [headless architecture](headless.md).
