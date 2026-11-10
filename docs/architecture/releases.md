---
paths:
  - release
  - tools/package
  - tools/release
  - internal/releaseinfo
  - e2e/release
  - .github/workflows/build-cli.yml
  - .github/workflows/release-cli.yml
  - .github/workflows/promote.yml
verified: 84decc42acae489bb4c4a608c71f2346d4a76148
---
# Product builds and CLI releases

## Current implementation

`release/products.toml` owns product versions and build numbers. The implemented release product is CLI. Desktop, Android and iOS records reserve independent metadata; they do not establish release support.

`go run ./tools/package -product cli` builds `unic` and `unicd` for the current target. The versioned archive contains the pair and the `uniclip` alias. Unix archives preserve the relative symlink. Windows archives provide `uniclip.cmd`. Development packaging through `make package-headless` and `make package-desktop` remains available and unsigned.

All three program entrypoints handle `--version` and `--version --json` before initialization. Release builds embed version, build number and source SHA. Development builds report `dev` unless packaging supplies metadata.

CLI assembly uses the existing npm main/platform-package model. `@uniclipboard/cli` pins every optional platform package to the exact `<version>-build.<number>` registry version. Both npm CLI command names use one launcher. The package also exposes `unicd` through a dedicated service entrypoint. The launcher executes the real CLI beside its sibling service. Alpha selects the npm `alpha` dist-tag. Stable selects `latest`. Channel names do not enter package versions.

The declared CLI matrix covers macOS arm64/amd64, Linux arm64/amd64 and Windows amd64. Each target runs its own executable identity acceptance. Linux amd64 also runs production headless process acceptance. Local acceptance covers only the current host; workflow definitions do not establish remote runner success.

## Build, publication and promotion

`build-cli.yml` runs for PRs, main/v2 pushes, manual requests and reusable calls. Its callers provide an exact source SHA. It checks the metadata digest, source identity, required target set and native executable receipts before `npm pack`. CI builds do not publish.

`release-cli.yml` supports alpha and direct stable publication. It first checks for a durable record for the selected version/build. A complete existing record restores its exact package files. Otherwise it awaits the reusable build. It stages the manifest and packages in a draft GitHub release before publishing to npm. The product tag is `cli/v<version>-build.<number>` and must reference the accepted source SHA. Existing assets are compared and never overwritten. Incomplete staging fails before npm publication; recovering that case requires the original candidate artifacts.

Public npm operations require clean source, GitHub Actions OIDC and npm with dist-tag OIDC support. Every package must authorize the exact release and promotion workflow identities. Packages may require an initial owner-controlled bootstrap before trusted publishing can be configured. Configuration is a deployment prerequisite, not proof supplied by local acceptance.

Publication validates every local archive and every existing registry version before the first mutation. It publishes platform packages first and the main package last. Each mutation has registry read-back. A durable record becomes public only after a real availability receipt passes validation.

`promote.yml` currently offers CLI only. It downloads the requested durable manifest, verifies all existing npm package integrity values and moves the exact package set to `latest`. It does not rebuild, pack or upload. It retains alpha. Both mutating workflows share one CLI concurrency group.

Retries preserve exact identity and reject integrity conflicts. An older build cannot replace a newer selected build. An unrecognized legacy npm channel version requires review. npm changes across packages are not atomic; a partial failure has a failed receipt and can be retried with the same candidate. External writers outside the workflow concurrency group remain an operational constraint because npm does not expose a compare-and-swap tag API.

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

The initial `0.1.0 (1)` entries are local candidate seeds. Public version allocation and the npm legacy-tag transition require review before publication. Public OIDC/provenance, remote native runners and real GitHub publication have not been accepted locally.

Desktop does not yet consume the immutable CLI lock or provide signed installers, R2 uploads or FlareRelease registration. Its stable CLI gate remains a required future implementation. Android and iOS lack native application projects. Their workflows, signing, Store acceptance and promotion are not implemented. GitCode is deferred. Do not archive decision 0018 while these phases remain open.

Exact interface requirements are in the [release contract](../contracts/release-contract.md). Process ownership remains in the [headless architecture](headless.md).
