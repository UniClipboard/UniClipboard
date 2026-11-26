# Desktop release runbook

Status: delivery implementation prepared; production activation blocked by the prerequisites below. No public release or channel mutation is authorized by the acceptance-CI approval.

## Accepted identities

- Desktop: `2.0.0 (1)`, six native targets.
- CLI: `2.0.0 (2)`, canonical distribution run `45030337326`.
- Locked CLI manifest SHA-256: `19a091d691c8d1ad5ea7bdfb12be39a2e6a2235756a919b7c9f415c4719dd8ec`.
- Complete native construction candidate: run `43421800442`, source `09488aaaa20aaf297d7be5c89271cba5f40786c4`. This run includes all 22 artifacts, real Windows installer lifecycle and macOS signing/notarization. Raw updater-key provisioning still blocks public delivery; Windows Authenticode is optional under the accepted unsigned policy.

Native-window run `43432479654`, source `cb1749765fb824c05babd9ad490aa8b6d929a6bb`, passed the canonical history/search/copy case on all six targets. All downloaded artifact checksums and cleanup receipts verified. Its synthetic directory clipboard does not establish real OS integration.

Sandbox run `43423601595` accepted that candidate on both Linux architectures for Nix, Flatpak and strict Snap, including real private CLI/service IPC and shutdown.

A later source change requires its own accepted candidate. Publication and promotion consume immutable files; they do not rebuild them.

## Prerequisites

1. Record Windows payloads and installers as unsigned when production Authenticode is unavailable. This accepted policy applies to alpha and stable. Keep false signature fields and the unsigned release-page warning. The discovered `signpath-test` environment is test signing only. Never mark it as a production certificate or re-sign accepted artifacts in place. macOS Developer ID/notarization and raw Ed25519 updater signatures remain required.
2. Check approved durable custody for an existing compatible raw Ed25519 updater key before generating anything. Configure one matching durable pair; do not rotate or replace an existing key implicitly. Keep the private key in `UNIC_UPDATE_PRIVATE_KEY` and put the corresponding base64 public key in `products.desktop.update_public_key`. Use `go run ./tools/release/update-sign -keygen <private-directory>` to generate a protected directory. Upload the private key through GitHub's secret interface. Never commit it or substitute Minisign/Tauri key encodings.
3. Set `FLARE_RELEASE_ADMIN_URL` and `R2_RELEASE_ENDPOINT` in the Production environment. Reuse the organization-shared `FLARE_RELEASE_ACCESS_CLIENT_ID` and `FLARE_RELEASE_ACCESS_CLIENT_SECRET`. The controller uses a complete `R2_RELEASE_ACCESS_KEY_ID` / `R2_RELEASE_SECRET_ACCESS_KEY` pair when configured. Otherwise it uses the existing repository `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` to derive S3 credentials in process memory. Token reuse requires the default-jurisdiction S3 endpoint for the configured account.
4. Run `Release Desktop` with `operation=preflight`. It performs a read-only authenticated R2 GET and checks both FlareRelease contracts. A successful read establishes read access. The create-only upload and full-byte verification still run during publication.
5. Publish the exact locked CLI npm candidate through the existing CLI release gates. Configure trusted npm publishing and verify every selected platform package. Matching product version strings are insufficient.
6. Complete native installer, sandbox, update-window and real-OS integration acceptance. The current native-window case uses a directory clipboard. Stable promotion requires the additional package-specific evidence recorded in the package-channel plan.

## Nonpublishing construction and acceptance

After authorizing and pushing the reviewed isolated branch, run the nonpublishing construction entries:

```sh
gh workflow run build-cli.yml --repo UniClipboard/UniClipboard \
  --ref feat/release-unsigned-windows --field distribution=true --field desktop_only=true
```

This registered entry calls the reusable Desktop workflow. It does not publish packages. The dedicated Desktop workflow cannot be dispatched directly until GitHub registers it on the default branch.

After a complete successful Desktop run, select its run ID:

```sh
gh workflow run build-cli.yml --repo UniClipboard/UniClipboard \
  --ref feat/release-unsigned-windows --field sandbox_run_id=<accepted-run-id>
```

The sandbox jobs execute Nix, Flatpak and strict Snap on both Linux architectures. They use exact pre-publication binary inputs. They do not submit to a Store.

Run the six-target source-frozen native-window case separately:

```sh
gh workflow run build-cli.yml --repo UniClipboard/UniClipboard \
  --ref feat/release-unsigned-windows --field native_gui=true
```

This case uses a synthetic directory clipboard. It does not establish package-specific real-OS clipboard, file/network permission or cross-application paste behavior.

Local process replay:

```sh
TMPDIR=/private/tmp go test -race ./...
python3 e2e/packaging/updates.py
python3 -m venv /private/tmp/unic-release-tools
/private/tmp/unic-release-tools/bin/pip install -r tools/release/requirements.txt
/private/tmp/unic-release-tools/bin/python e2e/packaging/publication.py
make desktop-e2e CASE=history-search-copy
```

Each process fixture emits a report and SHA256SUMS. The canonical native-window case also emits screenshots and a continuous recording, then removes private source/profile/binary state.

## Public delivery after separate authorization

1. Review `release/desktop-notes.json`. Accept the final candidate run, source SHA, CLI lock and update public key.
2. Run `Release CLI` with `candidate_run_id=<final-locked-cli-run-id>` to preserve the locked bytes. The current Desktop lock selects run `45030337326`. Select a new compatible source candidate when shared configuration/API has changed. Select the intended npm channel. Do not rebuild the candidate at a later source SHA.
3. Run `Release Desktop` with `operation=publish` and the accepted Desktop run ID. It validates the complete 22-artifact catalogue, both deployed contracts, all signatures and locked CLI npm availability before writes. It conditionally creates immutable objects, reads every object back, registers six updater archives and stores the immutable publication receipt. It does not change a Desktop channel. The workflow first stages/verifies the GitHub draft and downloads every attachment for hash verification. After origin availability passes, alpha publishes a prerelease page without selecting GitHub latest. Stable keeps the page in draft.
4. Read the current `desktop/alpha` or `desktop/stable` channel revision through the admin API. Review the selected build and availability receipt.
5. Run `Release Desktop` with `operation=promote`, the same candidate run ID, the intended channel and that exact revision. It uses the API's compare-and-swap operation and checks the public numbered identity and updater hashes. A concurrent change fails. Stable fails if lifecycle/native-GUI evidence is incomplete. After verified stable promotion, the workflow publishes the GitHub page as stable/latest. Page staging is verified before channel mutation. Omitted comparison/contributor input is restored from `release-page.json`.
6. Submit package-manager definitions only after origin availability passes. Submission, approval and public index read-back are separate channel states. Existing renderer output does not submit to any package manager.

The current FlareRelease registration limit is 512 MiB across registered artifacts. The six updater archives must fit that limit. All 22 formats remain in the immutable object catalogue. [R2 conditional operations](https://developers.cloudflare.com/r2/api/s3/conditional-operations/) provide create-only object semantics; the controller also checks complete bytes on retries and read-back.

## Stable acceptance input

After alpha publication, assign a native verifier to each of macOS, Linux and Windows. Test the exact published artifacts on both architectures. Record package lifecycle and native GUI results with a link to the platform evidence. Use the release catalogue for each filename and digest. Keep a receipt outside the immutable catalogue. A release operator supplies the three receipts together in the `stable_acceptance` workflow input.

The input is a JSON object with exactly three top-level keys, `darwin`, `linux` and `windows`. Each value has these fields.

| Field | Required value |
| --- | --- |
| `manifest_sha256` | SHA-256 of the published Desktop catalogue |
| `evidence` | HTTPS URL of the platform's retained acceptance evidence |
| `artifacts` | One entry for every catalogue artifact for this platform |
| `artifacts[].file` | Exact catalogue filename |
| `artifacts[].sha256` | Exact catalogue artifact SHA-256 |
| `artifacts[].package_lifecycle`, `artifacts[].native_gui` | `passed` after both checks pass for this artifact |

The release operator reviews the linked evidence before dispatch. The controller checks results, digests, uniqueness and completeness. It records the submitted receipts in the Actions delivery artifact. Promote the locked CLI set to npm `latest` before Desktop stable promotion. The Desktop controller reads back `latest` and integrity for all seven packages before mutating the Desktop channel.

```sh
jq -n --slurpfile darwin darwin.json --slurpfile linux linux.json --slurpfile windows windows.json \
  '{darwin: $darwin[0], linux: $linux[0], windows: $windows[0]}' > stable-acceptance.json
gh workflow run release-desktop.yml --ref main \
  -f operation=promote -f channel=stable -f candidate_run_id=<run-id> \
  -f expected_revision=<revision> -F stable_acceptance=@stable-acceptance.json
```

## Default branch cutover

The v2 development line continues on `main`. The three independent release workflows are registered on the default branch. `pr-checks.yml`, `build-cli.yml` and `check-expo.yml` run on pushes to `main`. The `v1` branch retains the legacy source and existing maintenance PRs. GitHub requires workflows to exist on the default branch for manual dispatch.

Run the read-only Desktop preflight with `gh workflow run release-desktop.yml --ref main -f operation=preflight`. After a cutover rollback, select the intended default branch explicitly and verify workflow registration before dispatching a release.

## GitHub page preview and draft

The v1 reference is [v1.6.2](https://github.com/UniClipboard/UniClipboard/releases/tag/v1.6.2), with source template/workflow at `c6483d73c7a0234be68571fdb5fa3d38a17c4871`. v2 keeps the page structure and independent product identities. It does not reuse v1 mobile links or legacy signature assertions.

Generate a local review bundle without GitHub writes:

```sh
python3 tools/release/github_desktop.py preview \
  --manifest <catalogue>/release-manifest.json \
  --cli-manifest <locked-cli>/release-manifest.json \
  --notes release/desktop-notes.json --channel alpha \
  --previous-tag v1.6.2 --output <new-page-directory>
```

Use `--development` only for a local preview of unaccepted inputs. The page states that limitation. Inspect `release-notes.md`, `release-page.json` and `SHA256SUMS.txt`. The separate CLI links point to the locked CLI Release; Desktop assets remain private product payloads. Do not copy CLI files into this page under a second identity.

Follow the default branch cutover above before dispatching the dedicated release workflows.

After production prerequisites and explicit remote-write authorization, run `Release Desktop` with `operation=github-draft`, the accepted candidate run ID and optional `previous_tag`. This stages a draft only. It does not upload to R2, change a channel or publish the page. macOS production signing/notarization and raw updater signing remain required even for remote staging. Windows Authenticode may remain unsigned. Use `publish` and `promote` as described above to complete delivery.

Replay nonpublishing process acceptance with `python3 e2e/packaging/github_release.py`. It uses the actual `gh` executable against an isolated HTTP service and emits `.cache/github-release-e2e/report.json`, a page preview and checksums. It does not prove live GitHub creation, production-token permissions or publication.


## Package-manager distribution

On the default branch, a completed and successful joint release starts
**Dispatch package-manager releases**. Each destination has its own Actions
run. Alpha prepares preview definitions and packages. COPR Alpha submits to
`uniclipboard-alpha`. Stable requests
publication or upstream submission. npm remains part of the joint release.

Retry one destination with the original joint run ID and attempt:

```sh
gh workflow run release-homebrew-tap.yml --ref main \
  -f release_run_id=123456789 -f release_run_attempt=1 -f publish=true
```

Use `publish=false` to prepare definitions and build packages without
mutating a destination. Retain the original run artifacts until distribution
completes. An older Stable selection cannot overwrite the current selection.

| Workflow | Destination and credentials |
|---|---|
| `release-homebrew-tap.yml` | Update the Cask and locked CLI Formula together in `UniClipboard/homebrew-tap`. `HOMEBREW_TAP_TOKEN` needs Contents write on the Tap. |
| `release-homebrew-cask.yml` | Submit a Cask update from `UniClipboard/homebrew-cask`. Use `HOMEBREW_CASK_TOKEN` with access to that fork and upstream PRs. Configure required reviewers on `homebrew-cask-production`. Approve after reviewing the rendered Cask and its `zap` paths. The approver is recorded as a co-author and handles upstream replies. |
| `release-scoop.yml` | Submit to Scoop Extras. Set `SCOOP_FORK` to an existing pushable fork and configure `SCOOP_TOKEN`. After the migration, Scoop can also track numbered Stable tags through its manifest. |
| `release-winget.yml` | Validate and submit both architectures through WinGetCreate. Configure `WINGET_TOKEN`. |
| `release-chocolatey.yml` | Pack and submit to the Chocolatey community repository. Configure `CHOCOLATEY_API_KEY`. |
| `release-aur.yml` | Build and update the project-owned `uniclipboard-git` source package. Use `AUR_SSH_PRIVATE_KEY` and the verified Ed25519 entry in `AUR_HOST_FINGERPRINTS`. The build also saves the separate binary recipe. |
| `release-pacman.yml` | Publish the signed x86_64 repository at `pacman-stable-x86_64`. Use the `pacman-production` environment and its `PACMAN_SIGNING_ENABLED`, `PACMAN_SIGNING_FINGERPRINT` and `PACMAN_GPG_PRIVATE_KEY`. The GitHub channel release must allow index updates. A client rejects a mixed index and signature during update. If the update stops in that state, restore the matching signed pair from the workflow artifact before retrying. |
| `release-copr.yml` | Build an SRPM with both verified RPMs and submit to the existing `uniclipboard` (Stable) or `uniclipboard-alpha` (Alpha) project. Configure `COPR_LOGIN`, `COPR_TOKEN` and `COPR_USERNAME`. |
| `release-snap.yml` | Build on both Linux architectures before uploading to Snap Store Stable. Configure `SNAPCRAFT_STORE_CREDENTIALS` for the registered `uniclipboard` snap. |
| `release-nix.yml` | Build the pinned flake on both architectures and publish its source archive to `nix/v<version>-build.<build>`. |
| `release-flatpak.yml` | Build both Flatpak bundles and publish them to `flatpak/v<version>-build.<build>`. Flathub requires its own human-authored submission. |

Install the first-party Nix flake from the versioned
`uniclipboard-nix.tar.gz` attachment with
`nix profile install tarball+<asset-URL>`. Install the matching first-party
Flatpak attachment with `flatpak install --user <bundle-file>` after making
the specified GNOME runtime available. Each Release includes
`SHA256SUMS.txt` linked to the exact Desktop catalogue.

For upstream PRs or moderated stores, use the workflow log to find the
submission. Check the destination index to confirm the package is publicly
installable. The joint release and its dispatch do not wait for external
moderation.

Replay read-only preparation for an existing successful joint run:

```sh
python3 e2e/packaging/channel_release.py --run-id 45712164931 --attempt 1
(cd .cache/channel-release-e2e && shasum -a 256 -c SHA256SUMS.txt)
```

This check uses real GitHub Actions receipts and release bytes while the
source artifacts remain available. It emits a definition and catalogue
digest report for every channel.

## GitCode release mirroring

After a published Desktop or Android release completes, `.github/workflows/mirror-gitcode.yml` is dispatched on `main` to copy the FlareRelease-registered artifacts (Desktop's 6 updater archives or Android's production APK) from R2 to GitCode via `mkdir700/gitcode-release-mirror@5055ef17b7302e27ca53cbe5f6e44722f7e55f6b` (`v1.0.0`) and register each verified copy with FlareRelease (`PUT /api/mirrors`).

GitHub tags map `/` to `-` on GitCode (`desktop/v2.0.0-build.11` -> `desktop-v2.0.0-build.11`, `android/v3.0.0-build.206` -> `android-v3.0.0-build.206`) because the Action forbids `/` in release tags.

Retry or backfill a published release manually from `main`:

```sh
gh workflow run mirror-gitcode.yml --ref main -f tag_name=desktop/v2.0.0-build.11
gh workflow run mirror-gitcode.yml --ref main -f tag_name=android/v3.0.0-build.206
```

### Required configuration and external prerequisites

- `mirror` environment (deployment branch restricted to `main`): `GITCODE_MIRROR_ACTION_SSH_KEY` (secret) and `GITCODE_MIRROR_ACTION_SSH_USER` (variable).
- Repository variables: `MIRROR_SSH_HOST`, `MIRROR_SSH_KNOWN_HOSTS`, optional `MIRROR_SSH_PORT` (default `22`), `GITCODE_OWNER`, `GITCODE_REPO`, optional Android overrides `GITCODE_ANDROID_OWNER` / `GITCODE_ANDROID_REPO`, optional `UPDATE_SERVER_URL` (default `https://release.uniclipboard.app`), optional `FLARE_RELEASE_ADMIN_URL` (default `https://release-admin.uniclipboard.app`).
- Shared organization secrets: `GITCODE_RELEASE_TOKEN`, `FLARE_RELEASE_ACCESS_CLIENT_ID`, `FLARE_RELEASE_ACCESS_CLIENT_SECRET`.
- **FlareRelease prerequisite**: update `UniClipboard/FlareRelease` `PUT /api/mirrors` (and `POST /api/mirrors/revoke`) to accept optional `buildNumber` and match `r.build_number = ?` instead of hardcoding `r.build_number IS NULL`. Public redirect (`findMirrorRedirect`) already matches by `a.r2_key`.
- **Shanghai relay host prerequisite**: update `/etc/gitcode-release-mirror/config.json` `fileNamePattern` to allow `.update.tar.gz` (Linux/Windows v2 Desktop updaters) and `.apk` (Android), and ensure `allowedRepositories` includes the target Android repository if `GITCODE_ANDROID_REPO` differs from `GITCODE_REPO`. `allowedSourcePrefixes` (`https://release.uniclipboard.app/artifacts/`) already admits v2 Desktop and Android R2 URLs.
