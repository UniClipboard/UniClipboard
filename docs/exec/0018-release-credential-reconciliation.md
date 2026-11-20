# Release credential reconciliation

- Date: 2026-11-20.
- Scope: read-only source, GitHub secret/variable names and existing Actions evidence; documentation correction only.
- Working source: `f7c998ac4e8413fa16c13389d1c8905d388638f8` on `feat/desktop-test-package`.
- Fixed v1 source: `c6483d73c7a0234be68571fdb5fa3d38a17c4871`; current inspected `origin/v2`: `5158490dbf3f29413315d89edc7252af90f73ccf`.
- Correction: repository/environment secret inventories did not establish organization-secret absence. The earlier blanket missing-release-credentials conclusion was incorrect.

## Existing v1 delivery and observed success

The fixed [v1 release workflow](https://github.com/UniClipboard/UniClipboard/blob/c6483d73c7a0234be68571fdb5fa3d38a17c4871/.github/workflows/release.yml#L443-L475) uses repository `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` with `wrangler r2 object put --remote`. It uploads to `uniclipboard-releases/artifacts/v<version>/<filename>`. It does not use `R2_RELEASE_*` S3 aliases.

The next step consumes the organization-shared `FLARE_RELEASE_ACCESS_CLIENT_ID` and `FLARE_RELEASE_ACCESS_CLIENT_SECRET`. It submits the prepared registration JSON to `https://release-admin.uniclipboard.app/api/releases/register` with the Access service headers and requires a `ready` response.

Existing v1.6.2 run [43187269797](https://github.com/UniClipboard/UniClipboard/actions/runs/43187269797) used that source and succeeded. Its `Create GitHub Release` job `131091765937` records successful R2 upload at 2026-11-12 09:29:41 UTC and successful FlareRelease registration at 09:29:43 UTC. These are actual historical write/registration observations, not a current credential probe or v2-contract acceptance.

A current unauthenticated [v1 public-download read-back](baselines/0018-unsigned-windows/v1-public-download.json) also passed: `artifacts/v1.6.2/SHA256SUMS.txt?source=origin` returned HTTP 200 and 2877 bytes, matching the GitHub asset SHA-256 `07e5161104b7ae76508dbfb4df8d02198e874b06d1a945fe287af8eb840baf17`. This establishes current public delivery of that legacy object. It does not verify authenticated S3 access, the current uploader token, the numbered v2 contract, or whether the server honored a cache-bypass query.

## Current name-only inventory

The repository-scoped `GET /repos/UniClipboard/UniClipboard/actions/organization-secrets` succeeded. It lists six organization secrets shared with this repository, including both Access names. It does not require organization-wide enumeration. The organization-wide 403 reported by the parent audit is a visibility limit, not evidence of missing secrets. No organization permission change is needed to establish this repository's shared names.

| Status | Item | Evidence and precise boundary |
| --- | --- | --- |
| Configured | `CLOUDFLARE_API_TOKEN`, `CLOUDFLARE_ACCOUNT_ID` | Present in repository secret metadata now; referenced by v1's successful R2 step. Current remote validity, exact permissions and token class were not probed. |
| Configured | `FLARE_RELEASE_ACCESS_CLIENT_ID`, `FLARE_RELEASE_ACCESS_CLIENT_SECRET` | Both names are currently shared with this repository through organization secrets; referenced by the successful v1 registration. No duplicate repository/Production secret is required just because those narrower inventories omit them. |
| Known non-secret source setting | FlareRelease admin base | v1 uses the fixed `https://release-admin.uniclipboard.app` host. v2 may reuse this base after read-only numbered-contract verification. No credential creation is needed to recover a URL already present in source. |
| Not verified | Existing token's S3 compatibility, token identifier and bucket jurisdiction | The earlier Wrangler success establishes historical object-write use only. It does not establish current SigV4 object-read access, token-verification permission or the bucket's S3 endpoint. |
| Not verified | Current Access authorization and numbered admin/public contracts | Current shared-name visibility and the successful historical `ready` response do not verify the deployed `product-version-build-v1` contract today. Earlier unauthenticated public 403s do not establish absence, DNS/TLS failure or an account suspension. |
| Missing current v2 wiring | `R2_RELEASE_ACCESS_KEY_ID`, `R2_RELEASE_SECRET_ACCESS_KEY` | These exact names were absent from repository, repository-shared organization and Production environment inventories. The current workflow references them, but this is an S3 credential-input gap, not proof that a new Cloudflare account/token must be created. |
| Missing current v2 wiring | `R2_RELEASE_ENDPOINT`, `FLARE_RELEASE_ADMIN_URL` | Absent from repository, repository-shared organization and Production variable inventories. The account ID and known admin host are potential existing inputs for deriving/defaulting these settings. Bucket jurisdiction and deployed contract still require verification. |
| Missing current updater configuration | `products.desktop.update_public_key` and a shared/configured `UNIC_UPDATE_PRIVATE_KEY` | The public field is empty and that private-key name is absent from the three inspected Actions scopes. This establishes missing configuration for this repository, not absence of a key in every external vault or unshared organization scope. Existing Minisign/Tauri keys are not raw mygo Ed25519 configuration. |

Production currently has no secret or variable names. Repository-shared organization variables are empty. These complete scoped queries support the missing-wiring rows above. They do not justify a global claim about unrelated accounts, vaults or unshared organization configuration. The corrected [machine-readable audit](baselines/0018-unsigned-windows/credentials-and-routes.json) keeps names and evidence only.

## Concrete v1/v2 controller differences

| Boundary | v1 implementation | Current v2 requirement |
| --- | --- | --- |
| Authentication | Wrangler uses a Cloudflare bearer API token and account ID. | boto3 uses SigV4 with S3 access-key ID/secret and an R2 endpoint. The bearer string cannot simply be assigned to `AWS_SECRET_ACCESS_KEY` unchanged. |
| Object identity | `artifacts/v<version>/<filename>`. | `artifacts/desktop/<version>/build.<number>/<filename>`, binding product/build/source and exact accepted bytes. Preserve this namespace and immutable candidate manifest. |
| Existing-object preflight | The inspected upload loop has no all-object byte comparison. | `matches()` downloads existing objects and checks complete SHA-256 and length before any upload. Identical retries reuse existing bytes; conflicts fail. |
| Atomic creation | The inspected Wrangler command passes no create-only condition. | `PutObject(IfNoneMatch="*")` must enforce creation at commit time. A preceding HEAD/GET followed by an unconditional PUT is not atomic and is not an acceptable substitute. |
| Race handling | No explicit conditional-conflict handling in that loop. | After 412, re-download and compare bytes. A racing identical object may resume; different bytes fail without overwrite. |
| Post-write read-back | The inspected upload step does not compare full downloaded bytes. | Each artifact and catalogue is fully read back from S3 and from its public origin URL before availability is recorded. The publication receipt is also create-only and fully read back through S3. |
| Registration | The historical prepared body reached `ready`. | Check authenticated `/api/contract` and public `/contract` first; require numbered product/build support and exact CLI npm availability. Register the six signed updater archives; retain all 22 native files in the object catalogue. |
| Channel changes | Legacy publication semantics are not reused as v2 promotion authority. | Registration does not promote. Promotion separately checks the availability receipt, expected revision and public numbered updater hashes; stable also retains native/lifecycle and stable CLI gates. |

The [R2 conditional-write contract](https://developers.cloudflare.com/r2/api/s3/conditional-operations/) documents atomic S3 `If-None-Match` checks. The [Cloudflare REST upload reference](https://developers.cloudflare.com/api/resources/r2/subresources/buckets/subresources/objects/methods/upload/) describes a 300 MB upload limit and does not document create-only headers in that endpoint's parameter contract. This does not prove that every REST implementation rejects conditional headers; it means an equivalent atomic guarantee has not been established here. Do not replace the tested S3 path with the v1 unconditional loop.

## Safe reuse candidate, without replacing the upload backend

The [R2 authentication reference](https://developers.cloudflare.com/r2/api/tokens/#get-s3-api-credentials-from-an-api-token) documents deriving S3 credentials from an eligible API token: the identifier is the access-key ID and the SHA-256 of its value is the secret access key. This supports investigating reuse of existing R2-authorized token material; it does not establish that the current token has the needed R2/S3 permissions.

1. Keep the existing boto3 conditional upload, conflict checks and full-byte read-back code.
2. In an explicitly approved trusted Actions context, reuse the existing Cloudflare token/account inputs without exporting their values. Select the correct user/account [read-only verification endpoint](https://developers.cloudflare.com/api/resources/accounts/subresources/tokens/methods/verify/) for its token class. Establish the token identifier, status and R2 permission/resource scope; a generic active status alone is insufficient.
3. If compatible, derive SigV4 material only inside the runner process and initialize the existing S3 client. Do not log, put on command arguments, persist, or upload the original/derived credentials. Do not create tokens or expand permissions merely to try this route. This bridge is not implemented by the current `storage()` function/workflow.
4. Confirm bucket jurisdiction and perform an authenticated, full-byte GET of an already published known object, comparing the accepted release digest. Verify the authenticated numbered FlareRelease contract using the existing organization-shared Access pair. These probes remain NOT RUN in this correction.
5. Only if this route is unsuitable, review provision of a narrowly scoped S3 credential pair. Do not rotate existing keys or change organization permissions as a shortcut. A different storage transport must first prove the same atomic creation/conflict/read-back behavior through isolated process E2E.

No token verification, credential derivation, authenticated Cloudflare/Access probe, new workflow dispatch, production upload, registration, deployment, channel mutation, secret creation or permission change ran during this correction. Only GitHub metadata and existing execution evidence were read. Signing/updater/publication acceptance remains separate; correcting the credential inventory does not make v2 ready to publish.
