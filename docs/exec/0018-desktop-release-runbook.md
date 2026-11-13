# Desktop release runbook

Status: delivery implementation prepared; production activation blocked by the prerequisites below. No public release or channel mutation is authorized by the acceptance-CI approval.

## Accepted identities

- Desktop: `2.0.0 (1)`, six native targets.
- CLI: `2.0.0 (2)`, canonical run `43419452491`.
- Locked CLI manifest SHA-256: `ae2bd7d0cb96675d052544a4365b11319f0868bf01aea6afc01a4fac31e6c2be`.
- Complete native construction candidate: run `43421800442`, source `09488aaaa20aaf297d7be5c89271cba5f40786c4`. This run includes all 22 artifacts, real Windows installer lifecycle and macOS signing/notarization. Windows production signing and raw updater-key provisioning still block public delivery.

Native-window run `43432479654`, source `cb1749765fb824c05babd9ad490aa8b6d929a6bb`, passed the canonical history/search/copy case on all six targets. All downloaded artifact checksums and cleanup receipts verified. Its synthetic directory clipboard does not establish real OS integration.

Sandbox run `43423601595` accepted that candidate on both Linux architectures for Nix, Flatpak and strict Snap, including real private CLI/service IPC and shutdown.

A later source change requires its own accepted candidate. Publication and promotion consume immutable files; they do not rebuild them.

## Prerequisites

1. Configure production Windows executable and installer signing. The discovered `signpath-test` environment is test signing only. Do not mark it as a production certificate. The currently locked CLI candidate has unsigned Windows files. After production signing is available, construct a new CLI build, accept and lock its immutable manifest, then construct a new Desktop candidate. Never re-sign an accepted artifact in place.
2. Provision one durable raw Ed25519 updater key. Keep the private key in `UNIC_UPDATE_PRIVATE_KEY` and put the corresponding base64 public key in `products.desktop.update_public_key`. Use `go run ./tools/release/update-sign -keygen <private-directory>` to generate a protected directory. Upload the private key through GitHub's secret interface. Never commit it or substitute Minisign/Tauri key encodings.
3. Configure `R2_RELEASE_ACCESS_KEY_ID`, `R2_RELEASE_SECRET_ACCESS_KEY`, `FLARE_RELEASE_ACCESS_CLIENT_ID` and `FLARE_RELEASE_ACCESS_CLIENT_SECRET` in the release environment. Configure `R2_RELEASE_ENDPOINT` and `FLARE_RELEASE_ADMIN_URL` as GitHub variables. Limit object credentials to the release bucket. The Access application must accept the service identity.
4. Deploy and verify the matching public FlareRelease Worker through its own reviewed deployment. On 2026-11-13, `https://release.uniclipboard.app/contract` and `/v1/desktop/stable.json` returned HTTP 404. Admin code availability does not establish public route availability. Do not work around this by writing D1 directly.
5. Publish the exact locked CLI npm candidate through the existing CLI release gates. Configure trusted npm publishing and verify every selected platform package. Matching product version strings are insufficient.
6. Complete native installer, sandbox, update-window and real-OS integration acceptance. The current native-window case uses a directory clipboard. Stable promotion requires the additional package-specific evidence recorded in the package-channel plan.

## Nonpublishing construction and acceptance

Run from the approved isolated branch:

```sh
gh workflow run build-cli.yml --repo UniClipboard/UniClipboard \
  --ref feat/v2-package-release --field distribution=true --field desktop_only=true
```

This registered entry calls the reusable Desktop workflow. It does not publish packages. The dedicated Desktop workflow cannot be dispatched directly until GitHub registers it on the default branch.

After a complete successful Desktop run, select its run ID:

```sh
gh workflow run build-cli.yml --repo UniClipboard/UniClipboard \
  --ref feat/v2-package-release --field sandbox_run_id=<accepted-run-id>
```

The sandbox jobs execute Nix, Flatpak and strict Snap on both Linux architectures. They use exact pre-publication binary inputs. They do not submit to a Store.

Run the six-target source-frozen native-window case separately:

```sh
gh workflow run build-cli.yml --repo UniClipboard/UniClipboard \
  --ref feat/v2-package-release --field native_gui=true
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
2. Run `Release CLI` with `candidate_run_id=<final-locked-cli-run-id>` to preserve the locked bytes. The current run `43419452491` is acceptance evidence; it does not meet the production Windows-signing prerequisite. Select the intended npm channel. Do not rebuild the candidate at a later source SHA.
3. Run `Release Desktop` with `operation=publish` and the accepted Desktop run ID. It validates the complete 22-artifact catalogue, both deployed contracts, all signatures and locked CLI npm availability before writes. It conditionally creates immutable objects, reads every object back, registers six updater archives and stores the immutable publication receipt. It does not change a Desktop channel.
4. Read the current `desktop/alpha` or `desktop/stable` channel revision through the admin API. Review the selected build and availability receipt.
5. Run `Release Desktop` with `operation=promote`, the same candidate run ID, the intended channel and that exact revision. It uses the API's compare-and-swap operation and checks the public numbered identity and updater hashes. A concurrent change fails. Stable fails if lifecycle/native-GUI evidence is incomplete.
6. Submit package-manager definitions only after origin availability passes. Submission, approval and public index read-back are separate channel states. Existing renderer output does not submit to any package manager.

The current FlareRelease registration limit is 512 MiB across registered artifacts. The six updater archives must fit that limit. All 22 formats remain in the immutable object catalogue. [R2 conditional operations](https://developers.cloudflare.com/r2/api/s3/conditional-operations/) provide create-only object semantics; the controller also checks complete bytes on retries and read-back.
