# Product build and release contract

## Scope

Each product owns `version` and a positive `build_number` in `release/products.toml`.
A build identity is `(product, version, build_number)`. Channels are alpha and stable.
CLI packages belong to npm. Desktop/mobile file publication belongs to R2 and FlareRelease.
GitCode is outside this implementation.

## CLI candidate and npm identity

The npm destination version is `<version>-build.<build_number>`; neither channel name is encoded.
Alpha uses npm `alpha`; stable uses npm `latest`. Promotion changes tags on existing versions without rebuilding, repacking or re-uploading.
The main package is `@uniclipboard/cli`. Platform packages use `@uniclipboard/cli-<node-os>-<node-arch>`.
Every platform dependency is pinned exactly. Both `unic` and `uniclip` invoke the same launcher; `unicd` remains beside the real CLI executable and has its own npm command entrypoint.

Candidate manifests record product/build/source identity, target set, executable hashes, package SHA-512 integrity, source cleanliness and acceptance results.
A candidate set is complete only when all declared targets are present and agree on identity.
Retries compare immutable package integrity; package existence alone is insufficient.
Publish platform packages first and the main package last. Read back each version and integrity before advancing its channel.
Promotion verifies the complete existing package set before changing any tag. Do not overwrite a newer channel selection.
Partial external failures are recorded per package and may be retried with the same manifest. npm tag updates across packages are not an atomic transaction.

## Publication authority

Public publication requires a clean source SHA and required native/process acceptance.
CLI release and promotion use npm OIDC trusted publishing. Workflow identities must be authorized per package, with dist-tag management enabled for promotion.
Local orchestration acceptance uses only a loopback npm registry fixture; fixture acceptance cannot establish npm OIDC/provenance or public availability.
A GitHub build record stores the immutable manifest. It is not CLI channel authority.

Trusted publisher configuration follows the [npm trusted publisher contract](https://docs.npmjs.com/trusted-publishers/).
Configure `release-cli.yml` for publication and dist-tag management. Configure `promote.yml` for dist-tag management on every main/platform package.
The workflows pin npm 11.21.2 because dist-tag OIDC requires npm 11.21+ or 12.2+.

## CLI manifest and receipt fields

The checked-in product table has `schema = 1`. Product versions are canonical numeric `major.minor.patch` values. Build numbers are integers from 1 to 999999999. Product target arrays must not contain duplicates. CLI target names are `darwin-arm64`, `darwin-amd64`, `linux-arm64`, `linux-amd64` and `windows-amd64`.

Each native `manifest-<os>-<arch>.json` contains `product`, `product_version`, `build_number`, `metadata_sha256`, `source_head`, `source_dirty`, `source_sha256`, `source_unchanged`, `os`, `arch`, `go`, `binaries_sha256`, `archives_sha256` and `aliases`. Native smoke adds `acceptance.version_smoke`, `acceptance.alias_smoke` and `acceptance.native`, all true on success. Assembly requires these observations for every declared target.

The assembled `release-manifest.json` contains:

| Field | Value |
|---|---|
| `schema`, `product` | `1`, `cli`. |
| `version`, `build_number`, `npm_version` | Product identity and exact npm encoding. |
| `source_sha`, `source_clean`, `metadata_sha256` | Source identity, cleanliness and original metadata digest. |
| `targets` | Complete target names. |
| `target_manifests` | Native manifests keyed by target. |
| `packages` | Platform packages first; main package last. |

Every package records `name`, `version`, `file`, `sha256` and SHA-512 `integrity`. Platform packages also record `target` and `binaries_sha256`. Digests cover the final packed file. Existing public integrity values must match exactly. The main package's optional dependencies must equal the complete exact platform set.

A publication receipt records `product`, `version`, `build_number`, `manifest_sha256`, `channel`, `registry`, `fixture`, `operation`, `status` and `packages`. Overall status is `running`, `available` or `failed`. Package status is `pending` or `available`; a pending package in a failed receipt is not proof of public availability. A failed receipt also records `error`. A fixture receipt must never finalize a public build record.

Durable record staging creates or verifies the exact Git tag, then uploads immutable assets without replacement. A complete record can resume release without rebuilding. A source/metadata mismatch fails. A partially staged record fails before npm mutation and requires recovery from the original candidate artifacts. Promotion needs the accepted manifest and existing npm versions; it does not need to reconstruct the package files.

## CLI v1 upgrade notice and migration

Interactive CLI invocations display a v1 incompatibility warning on stderr only if the known v1 data directory exists. Detection checks the default directory and the active UC_PROFILE directory without reading their contents, credentials or database. Missing directories, non-directory paths and fresh installations remain silent. Noninteractive stderr and commands with `--json` omit the warning. Custom/portable sources outside these known locations are selected explicitly with `--source`. The warning does not require acknowledgement and does not start migration. Normal stdout and command exit codes remain unchanged.

`unic migrate` and `unic migrate --help` show an offline migration guide without opening a profile, credentials or the service. The guide instructs users to back up v1 data, quit v1 and stop the target service, run `unic migrate --dry-run --json`, then run `unic migrate --json`. Users may specify `--source` and `--target`. All devices must use v2 and establish new pairings. Settings, identities and trust do not migrate.

Every CLI archive and npm platform package includes `unic-migrate` beside `unic`. The `unic migrate` command invokes this exact sibling helper and preserves its exit status, including 3 for partial migration. The importer keeps its existing read-only source, exclusive target, resumable import and reopened-content verification rules. Explicit passphrases travel through stdin, never command arguments or environment variables.

## Desktop and mobile gates

Desktop locks an existing exact CLI build and manifest. A stable Desktop requires that build to be stable in npm.
The accepted CLI files are reused unchanged. Signing/notarization precedes final distribution hashes.
Desktop/mobile must check the deployed FlareRelease build-identity contract before registration or channel mutation.
A numbered FlareRelease build uses separate version/build fields and explicit updater selection for same-platform formats.
A missing native project, lock, signing prerequisite or required acceptance fails clearly. No placeholder artifact satisfies release.
Mobile native applications and Store publication are not implemented by the CLI vertical slice.
