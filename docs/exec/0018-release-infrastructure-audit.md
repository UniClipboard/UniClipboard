# 0018: Existing release infrastructure audit

- Date: 2026-11-10 (Asia/Shanghai).
- Scope: Read-only source inspection and GitHub Actions configuration-name inventory, followed by local design documentation updates.
- Parent: [0018 execution plan](0018-independent-product-build-and-release.md).
- Monorepo planning HEAD: `620dc2148`.
- Verified remote UniClipboard main: `885546c7cf6f6f0d2076104efd1d90d9f73d9744`; local `origin/main` matched this revision. Local `main` differed and was not used as the source authority.
- Verified remote FlareRelease main: `19a7e496d01d2588821f574d9eca86dc7e7c2eb7`.
- Verified remote UniClip main: `f8a0c8fe9b45f0d5fa5b9483f48829ceee2bf282`. Its workflows describe the old mobile repository, not implemented mobile applications in this monorepo.
- Scope correction from the user: CLI is distributed and managed by npm, requires no FlareRelease registration type, and GitCode implementation is deferred. Mirror findings below describe existing infrastructure only.

## 1. Available configuration names

GitHub returned HTTP 403 for organization-wide secret and variable enumeration because the current CLI token lacks the required organization scope.
Repository-scoped organization-secret enumeration succeeded for both UniClipboard and UniClip. It establishes which organization secrets are shared with each repository.
No secret values were read. Variable responses were filtered to names; no variable values are retained here.
Configuration existence does not establish credential validity, target-account access or signing suitability.

| Purpose | Names | Verified scope and reuse boundary |
|---|---|---|
| FlareRelease registration and channel API | `FLARE_RELEASE_ACCESS_CLIENT_ID`, `FLARE_RELEASE_ACCESS_CLIENT_SECRET` | Organization secrets shared with both repositories. Reuse the dedicated Access service-token pair. |
| GitCode release writes | `GITCODE_RELEASE_TOKEN` | Organization secret shared with both repositories. Target authorization still needs destination acceptance. |
| Release R2 upload | `CLOUDFLARE_ACCOUNT_ID`, `CLOUDFLARE_API_TOKEN` | Repository secrets present in both repositories. Existing workflows use Wrangler, not build-cache S3 credentials. |
| Public release base | `UPDATE_SERVER_URL` | UniClipboard repository variable present. Existing workflow uses it to construct artifact URLs. |
| GitCode destination | `GITCODE_OWNER`, `GITCODE_REPO` | Repository variables present in both repositories. Do not assume the old mobile destination equals the desktop destination. |
| Mirror SSH verification | `MIRROR_SSH_HOST`, `MIRROR_SSH_KNOWN_HOSTS` | Repository variables present in both repositories. Optional `MIRROR_SSH_PORT` was not listed; the desktop Action defaults to port 22. |
| Current desktop mirror transport | `GITCODE_MIRROR_ACTION_SSH_KEY`, `GITCODE_MIRROR_ACTION_SSH_USER` | Secret and variable respectively in UniClipboard's `mirror` environment. Old `MIRROR_SSH_KEY` also exists but is not the desktop Action key. |
| macOS signing/notarization candidates | `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | UniClipboard repository secrets present and referenced by the old build workflow. Validate certificate identity and export format for the new packaging. |
| Artifact signatures | `MINISIGN_RELEASE_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | UniClipboard repository secrets present. Choose the signature contract for the new clients; key existence does not make a Tauri updater contract applicable to mygo. |
| Windows signing test | `SIGNPATH_API_TOKEN` | Present only in the inspected `signpath-test` environment. No `signpath-production` environment was listed. |
| iOS App Store Connect | `ASC_API_ISSUER_ID`, `ASC_API_KEY_ID`, `ASC_API_KEY_P8` | Present in UniClip repository secrets; absent from UniClipboard's inspected repository/shared-organization names. Provision an authorized scope before monorepo iOS release. |
| iOS signing candidates | `DIST_CERT_P12_BASE64`, `DIST_CERT_PASSWORD`, `DEV_CERT_P12_BASE64`, `DEV_CERT_PASSWORD` | Present in UniClip repository secrets. Confirm signing/provisioning and grant the new repository only the required distribution material. |

Both repositories have a `mirror` environment whose deployment branch policy allows only branch `main`.
Release tags and the current v2 planning branch do not satisfy that policy.
Preserve the protection and design a trusted invocation; do not inline-call the mirror job from a tag and assume its environment will accept it.
UniClipboard had no shared organization variables in the repository-scoped query.

The main workflow references `WINDOWS_SIGN_BACKEND`, `SIGNPATH_PRODUCTION_POLICY_SLUG` and `SIGNPATH_PRODUCTION_CERT_THUMBPRINT`, but these were absent from the inspected UniClipboard secret/variable scopes.
The repository has `ALLOW_UNSIGNED_WINDOWS_ALPHA` and `ALLOW_UNSIGNED_WINDOWS_STABLE` variable names; their values were not read.
No production Windows signing capability is established by this inventory.
`MINISIGN_RELEASE_PASSWORD` was also not listed; the existing workflow permits an empty password, so absence alone does not prove signing failure.
`BUILD_CACHE_R2_*` credentials in the old mobile repository belong to build caching and are not substitutes for release-bucket authorization.

## 2. Existing upload, registration and mirror sequence

The inspected [desktop release workflow](https://github.com/UniClipboard/UniClipboard/blob/885546c7cf6f6f0d2076104efd1d90d9f73d9744/.github/workflows/release.yml) performs:

1. Assemble final release assets and registration metadata.
2. Upload files with `wrangler r2 object put` into bucket `uniclipboard-releases`, key `artifacts/v<version>/<filename>`.
3. Call `POST https://release-admin.uniclipboard.app/api/releases/register` with `CF-Access-Client-Id` and `CF-Access-Client-Secret` headers.
4. Require `.data.status == "ready"`. Registration does not advance a channel.
5. Run GitCode mirroring independently from a trusted `main` ref.

The old [mobile publish workflow](https://github.com/UniClipboard/UniClip/blob/f8a0c8fe9b45f0d5fa5b9483f48829ceee2bf282/.github/workflows/publish.yml) uploads Android files under `android/artifacts/<tag>/<filename>` in the same bucket and uses the same Access-protected registration endpoint.
Do not overwrite or relocate those historical objects as part of this design.

The [desktop GitCode workflow](https://github.com/UniClipboard/UniClipboard/blob/885546c7cf6f6f0d2076104efd1d90d9f73d9744/.github/workflows/mirror-desktop-gitcode.yml) pins `mkdir700/gitcode-release-mirror` to `5055ef17b7302e27ca53cbe5f6e44722f7e55f6b`.
It uses SSH to run the configured mirror transport on a mainland host.
The workflow supplies R2 download URLs and SHA-256 values; the transport verifies source bytes, uploads or reuses matching GitCode assets, and anonymously reads them back.
The runner then calls `PUT /api/mirrors` only for successful matching receipt entries.
FlareRelease credentials stay on the runner.
Dispatch, mirror completion and mirror registration are separate states.

Current desktop registration selects updater artifacts only. It does not register every DMG, DEB, RPM or portable archive.
The documented host allow-list admits `.app.tar.gz`, `.AppImage`, `.AppImage.tar.gz`, `.nsis.zip` and `.exe`, with a 300 MiB bound.
The actual host configuration was not inspected in this task. Any future mirror extension would require a separate host-policy check. No extension is part of this task, and CLI distribution is owned by npm.
The mobile mirror workflow currently uses a separate older host entry point. Do not assume the new desktop Action has replaced it.

## 3. Required FlareRelease changes

These findings were checked against remote main source, not the deployed service revision:

| Existing contract | Required change for 0018 |
|---|---|
| `releases_product_version_unique` covers product and version only | Add separate build-number identity across registration, queries, artifacts, mirrors and channel responses. |
| Registration variants are Desktop, Android and iOS; only iOS accepts a separate build number | Add the agreed build-number identity to the existing product variants. Do not add CLI; npm owns it. |
| Promote checks the release's `prerelease` flag against the channel | Move channel membership out of immutable build identity so one accepted alpha build can enter stable unchanged. |
| Public channel routes are Desktop stable/alpha and Android stable/beta; iOS has a store-specific path | Define Desktop/mobile alpha/stable semantics and preserve separate store availability. CLI channels are npm dist-tags. |
| Initial registration checks R2 existence and size with `bucket.head` | Establish end-to-end SHA-256 verification and conditional/immutable upload before accepting build integrity. Size alone is insufficient. |
| Rollback globally withdraws downloads for the replaced release | Separate channel pointer movement from global withdrawal when one build serves multiple channels. |
| Promotion uses `releaseId` and `expectedRevisions` | Resolve exact build identity, preserve concurrent-update checks, and define safe retries after partial channel/public-policy publication. |

Sources: [schema](https://github.com/UniClipboard/FlareRelease/blob/19a7e496d01d2588821f574d9eca86dc7e7c2eb7/src/server/db/schema.ts), [registration](https://github.com/UniClipboard/FlareRelease/blob/19a7e496d01d2588821f574d9eca86dc7e7c2eb7/src/domain/registration.ts), [admin service](https://github.com/UniClipboard/FlareRelease/blob/19a7e496d01d2588821f574d9eca86dc7e7c2eb7/src/server/db/admin-service.ts), [public routes](https://github.com/UniClipboard/FlareRelease/blob/19a7e496d01d2588821f574d9eca86dc7e7c2eb7/src/server/public-app.ts).

## 4. Proposed integration order

1. Resolve the existing service's product IDs and old/new application ownership before connecting new clients. Reusing an old `desktop` identity must not silently redirect legacy clients to an unverified incompatible application.
2. Complete FlareRelease's Desktop/mobile build identity, channel, integrity and rollback contracts. Verify source-to-deployed-service identity before enabling publication. Do not add CLI registration.
3. Define product/version/build-number R2 keys and an upload conflict policy. One candidate key shape is `artifacts/<product>/v<version>/build-<number>/<filename>`; it remains a proposal until public routes and host policy are validated.
4. Complete CLI build → npm platform/main package publication → alpha dist-tag → stable destination dist-tag promotion. Verify direct stable publication separately. CLI does not call FlareRelease or require R2 publication.
5. Complete Desktop/mobile R2 → Ready registration → alpha/stable publication against the updated FlareRelease contract. GitCode implementation remains deferred.
6. Gate stable Desktop on its exact stable CLI dependency. Add native store paths when real mobile applications exist.
7. Keep existing GitCode behavior intact without extending or activating it. It is not an acceptance gate for this task.
8. Verify npm dist-tags/package integrity, Desktop/mobile public channel responses, file hashes and per-destination receipts. A stable R2 channel may reuse the alpha object's exact key; a separate destination upload must preserve bytes.

## 5. Release pages

Existing source references desktop releases in `UniClipboard/UniClipboard` and mobile releases in `UniClipboard/UniClip`.
That old repository split is not the target version model of the new monorepo.
The target has four independent release records: Android, iOS, Desktop and CLI.
GitHub can list their product/build tags on one repository Releases index, with separate notes and assets per record.
FlareRelease can keep one administration UI for Desktop, Android and iOS with product filtering; its current route structure already uses a shared Releases list and release details. CLI versions are managed on npm.
A public download page may aggregate the four products and expose alpha/stable selection, but each download resolves its own product/channel/target availability.
The public page layout remains unconfirmed. Publication identity must not depend on that layout.

## 6. Verification boundary

No workflow was dispatched. No build, signature operation, R2 upload, FlareRelease mutation, GitCode transfer, host-policy edit, deployment or Store submission was performed.
Organization-wide enumeration was unavailable; repository-shared organization names were verified successfully.
Existing credentials were not tested. The FlareRelease deployment revision and mirror host's current state remain unverified.
No legacy release, source branch, external secret or environment was changed.

## 7. Existing npm distribution and remaining mapping

Checked main sources: [npm workflow](https://github.com/UniClipboard/UniClipboard/blob/885546c7cf6f6f0d2076104efd1d90d9f73d9744/.github/workflows/npm-publish.yml), [package assembly](https://github.com/UniClipboard/UniClipboard/blob/885546c7cf6f6f0d2076104efd1d90d9f73d9744/scripts/build-npm-packages.mjs), and [distribution notes](https://github.com/UniClipboard/UniClipboard/blob/885546c7cf6f6f0d2076104efd1d90d9f73d9744/npm/README.md).

The main package is `@uniclipboard/cli`, with five platform packages for darwin arm64/x64, linux arm64/x64 and win32 x64.
The main package pins exact optional dependency versions. Platform packages publish first, main package last.
The old workflow downloads checksum-covered GitHub CLI assets, assembles npm packages and publishes through GitHub Actions OIDC with provenance and no `NPM_TOKEN`.
Its version suffix currently selects alpha/beta/rc; stable maps to `latest`. That suffix-derived behavior must change for the new channel-free version model.
The old Rust binary names and five-target matrix do not establish the new Go product's binary names or six-target native support.

For the target design, recommend alpha → npm `alpha` and stable → npm `latest`.
Promotion operates on existing immutable package versions via dist-tags; it does not re-upload npm tarballs or register CLI with FlareRelease.
`package-name@version` is immutable, so multiple builds of one product version need a defined distinct npm registry version mapping.
Do not assume an added JSON build-number field, or `+build.N` alone, satisfies registry identity and update ordering. Keep the encoding unresolved until registry/installer behavior is verified.
The encoding must not contain alpha/stable, and promotion must not rewrite it.

Official references: [dist-tags](https://docs.npmjs.com/cli/v11/commands/npm-dist-tag/), [immutable package versions](https://docs.npmjs.com/unpublishing-packages-from-the-registry/), [OIDC trusted publishing](https://docs.npmjs.com/trusted-publishers/).
The current npm documentation supports OIDC dist-tag management only when the trusted publisher has the corresponding opt-in permission and the CLI version supports it.
Changing from `npm-publish.yml` to product release/promote workflow identities therefore needs an explicit package trust check.
No registry mutation, package publication, package download or install E2E was performed in this audit.
