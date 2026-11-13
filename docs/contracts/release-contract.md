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
Promotion verifies the complete existing package set before changing any tag. Do not overwrite a newer channel selection. Known v1 alpha/beta/rc tags can advance to a strictly higher product version. Unknown or same-base legacy tags fail before mutation.
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

The checked-in product table has `schema = 1`. Release TOML files must use LF checkout bytes on every platform, enforced by `.gitattributes`. Metadata SHA-256 covers those exact bytes. Product versions are canonical numeric `major.minor.patch` values. Build numbers are integers from 1 to 999999999. Product target arrays must not contain duplicates. CLI target names are `darwin-arm64`, `darwin-amd64`, `linux-arm64`, `linux-amd64`, `windows-amd64` and `windows-arm64`.

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

Every CLI archive and npm platform package includes the migration importer within `unic`. The `unic migrate` command runs offline in-process and returns 3 for partial migration. The importer keeps its existing read-only source, exclusive target, resumable import and reopened-content verification rules. Explicit passphrases travel through stdin, never command arguments or environment variables.

## Desktop installation identity and update ownership

These requirements are accepted. Desktop staging and format construction are partially implemented; complete native installer and release acceptance remain pending.

The product name is `UniClipboard`. Preserve existing channel package identifiers, including the `uniclipboard` base name and channel-specific identifiers. The Desktop application identifier remains `app.uniclipboard.desktop`.
Within one distribution channel, v2 replaces the installed v1 program. Official parallel v1/v2 Desktop installation is not a supported mode.
Alpha and stable share one installation identity. Channel switching replaces program files and preserves v2 user data; it does not authorize automatic downgrade.
Development builds use a separate installation identity and data directory.

Executable names are `unic-desktop`, `unic` and `unicd`; `uniclip` remains the CLI alias.
Desktop and CLI have independent product versions. Each product increments its build number within a version. Desktop consumes one exact accepted CLI build and manifest.
Every native package format must correctly order increasing product versions and increasing builds. Exact format encodings require tool validation before candidate construction.

Desktop keeps its bundled CLI and service in an application-private directory. It must not overwrite independently installed CLI files or public commands.
Package-manager installations delegate updates to the owning package manager. Accepted direct-download formats use the application update path supported by that format.
An installer must respect other package managers' file ownership; retaining an application identifier does not authorize cross-manager removal.

v2 uses data directories separate from v1. Program replacement preserves v1 data for explicit history migration and must not silently reuse v1 settings, device identities or trust.
Uninstall removes only the installation's owned program files and preserves user data by default.
Release acceptance must verify v1-to-v2 replacement, alpha/stable replacement, independent CLI coexistence, development isolation and data preservation with actual package lifecycle operations.

## Desktop and mobile gates

Desktop locks an existing exact CLI build and manifest. A stable Desktop requires that build to be stable in npm.
The accepted CLI files are reused unchanged. Signing/notarization precedes final distribution hashes.
Desktop/mobile must check the deployed FlareRelease build-identity contract before registration or channel mutation.
A numbered FlareRelease build uses separate version/build fields and explicit updater selection for same-platform formats.
A missing native project, lock, signing prerequisite or required acceptance fails clearly. No placeholder artifact satisfies release.
Mobile native applications and Store publication are not implemented by the CLI vertical slice.

## Desktop staging and format evidence

Desktop metadata locks `cli_version`, `cli_build_number` and `cli_manifest_sha256` to one accepted CLI manifest. A seed or empty digest cannot satisfy a production build.
The selected target must exist in both Desktop metadata and the CLI manifest. Staging checks native CLI acceptance, package integrity and executable hashes; it must not rebuild or re-sign those CLI bytes.
CLI native signatures, when requested through `UNIC_MACOS_SIGNING_IDENTITY` or `UNIC_WINDOWS_CERT_SHA1`, are applied before binary/archive hashes and native smoke acceptance.
Desktop signing verifies those locked signatures first and updates only Desktop-owned file hashes.

A stage manifest records product/version/build/target/source identity, metadata digest, source file hashes, development status, private payload path, file/alias hashes, the CLI lock and explicit acceptance results.
Each format directory contains `artifacts.json` with final file names, SHA-256 values, sizes, source/CLI identities, stage manifest digest and acceptance/signature state.
Constructing unsigned files must not set release signing or public availability to passed.
A package lifecycle receipt binds exact artifact hashes, target, format and required installation/reinstallation/upgrade/removal/data-preservation/file-ownership/service-shutdown observations.
Attaching a receipt requires a matching target/format and package hash. A DEB receipt cannot accept an RPM or pacman package.

DEB/RPM/pacman use product version plus native package revision for build number. Windows setup uses `major.minor.patch.build`; each component must fit 0..65535 and build number must be positive.
Scoop and Chocolatey use the same four numeric components. Homebrew Cask records `version,build`; Nix/Snap use a build-bearing version string. Native upgrade ordering remains an acceptance requirement for every manager.

Channel definitions must come from one product build and one CLI manifest digest, with validated local package hashes and fixed HTTPS destinations.
Rendering retains pending installation, native, submission, approval and availability states. It is not proof that the destination URL exists.
Source AUR builds produce their own source receipt; Nix may patch only Desktop-owned executable paths, preserving locked static CLI/service files.

`UNIC_HOME` overrides portable/default paths. A regular sibling `portable.dat` selects `data/` beside the resolved executable. Other marker types fail.
Default version-`dev` data uses `unic-development/`; released data uses `unic/`. Both remain separate from v1 data. The socket defaults to the chosen data directory and remains subject to platform path-length bounds.

## Numbered Desktop updater and delivery

The in-app updater uses mygo's raw Ed25519 format: a base64 32-byte public key and a base64 64-byte private key. The signature covers the archive SHA-256 digest. Existing Minisign and Tauri key encodings are not interchangeable with this format.

The signed archive contains exactly one regular `release-identity.json` with the Desktop product, product version, build number and native target. The client verifies the declared identity against that signed content before handing the archive to mygo. A correctly signed archive for another build must fail. Same-version builds compare as `<version>-build.<number>` inside the SDK. Feed channels remain external metadata.

The metadata's `update_public_key` enables in-app updates. An empty key disables them. `desktop.update_channel` accepts `stable` and `alpha`, defaults to `stable`, and applies on the next check. Package-manager and portable installations do not start the direct updater. AppImage self-update is unsupported by the pinned SDK. Windows NSIS records `/MANAGED=winget` or `/MANAGED=chocolatey`; reinstall preserves existing ownership.

Apple notarization derives a copied, signed stage. It submits and staples that bundle once before deriving both containers. The original signed stage and locked CLI bytes remain unchanged. Final container hashes include the stapled bundle. A payload signature is distinct from a separately signed installer/container. Artifact records expose both states. A production Windows setup EXE requires its own verified Authenticode signature; signed embedded programs alone are insufficient.

The immutable Desktop catalogue requires every declared target and format. Object keys use `artifacts/desktop/<version>/build.<number>/<filename>`. Uploads use conditional create-only S3 writes and full-byte read-back. Existing bytes must match. A conflict fails without overwriting the key. Publication requires verified npm availability of the exact locked CLI package set.

FlareRelease currently limits one numbered registration to 512 MiB in aggregate. The registration selects the six signed native updater archives. All 22 platform/format combinations remain in the object catalogue. Non-updater download assets are not silently discarded. Registration does not promote a channel. Promotion requires a matching availability receipt and the expected channel revision. It verifies the public numbered identity and exact updater hashes after the mutation. Stable additionally requires package lifecycle and native GUI evidence for every catalogue entry. Missing evidence blocks stable promotion.

Publication and promotion are separate manually selected operations. Preparation and CI construction do not establish production delivery. `--fixture` accepts only loopback services and cannot publish a development candidate to production.

Desktop staging also compares target-selected configuration, local API, lifecycle and server source hashes against the CLI candidate's dependency manifest. A changed shared contract requires a new accepted CLI build before Desktop staging. Product versions alone never establish private-service compatibility.

## Desktop GitHub Release page contract

Use product tag `desktop/v<version>-build.<number>`. Bind its actual Git ref to the accepted source commit. Do not reuse CLI tags or v1's shared `v<version>` tag. GitHub page state is distinct from FlareRelease channel state.

Use the migrated v1 page sections: changelog, comparison/source, installer table, independently locked CLI downloads, verification and optional new contributors. Do not advertise mobile release support or claim every download has the same signature format. Keep the raw Ed25519 updater format defined above; do not import v1 Minisign/Tauri semantics.

Attach exactly the complete 22-artifact Desktop catalogue, six updater `.sig` files, `release-manifest.json`, `desktop-notes.json`, `release-page.json` and `SHA256SUMS.txt`. The checksum list covers every attachment except itself. `release-page.json` binds the catalogue digest, bilingual-note digest, previous comparison tag and generated contributors to support deterministic later promotion. An existing context must match supplied nonempty inputs; omitted optional context is restored from the immutable attachment.

Preflight the tag, page identity and all existing attachment bytes before writes. Reject unexpected, duplicate, changed or missing published attachments. Resume missing draft attachments without replacing existing bytes. Download each new upload and compare its SHA-256. A mismatched upload leaves the page in draft and fails acceptance. Alpha retries cannot downgrade an already stable page.

The workflow stages/verifies the draft before R2 publication or channel promotion. Alpha public pages require a matching successful origin-availability receipt and use `prerelease=true`, `make_latest=false`. Stable public pages additionally require a matching verified `desktop/stable` promotion receipt and use `prerelease=false`, `make_latest=true`. No code path creates a public page directly. Production signature, native-input and exact CLI-lock gates remain mandatory for remote operations.

Local development previews cannot mutate GitHub. Process acceptance may use synthetic catalogues only with an explicit loopback fixture URL and disposable GitHub CLI credentials. Its output must identify the fixture and cannot be used as a production receipt.
