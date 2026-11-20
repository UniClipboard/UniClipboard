# Unsigned Windows delivery acceptance

Status: implementation and isolated native process acceptance passed. Production readiness remains incomplete. No public publication or channel mutation is authorized.

## Source and policy

- Worktree: `cb606084-bf03-4199-8c9f-68d14f5e016b`.
- Branch: `feat/release-unsigned-windows`.
- Base: `1e90f041d1f90e3c62a41a4bb86b5503ebe00c20`, freshly fetched `origin/v2`.
- Local implementation commit: `474fd01364c8fe9b42b8ec84b4898ac79afe74ba`. No push or PR occurred during native acceptance.
- Windows production Authenticode is optional for both payloads and installers. Unsigned artifacts must remain explicitly unsigned in manifests and release pages.
- macOS Developer ID/notarization, raw Ed25519 updater verification, immutable digests, source identity and exact CLI locks remain mandatory.

## Failure inventory written before implementation

1. An unsigned Windows payload or installer is rejected solely for missing Authenticode: production catalogue assembly must accept it without claiming a signature.
2. Missing macOS signing or notarization is accepted: assembly must reject it.
3. Missing updater key/signature or a damaged signature is accepted: assembly must reject it using the real raw Ed25519 verifier.
4. A changed artifact, duplicate slot, mixed source or CLI lock reaches publication: assembly must reject it before writes.
5. Release pages omit the Windows unsigned notice: production-mode local preview must fail acceptance.
6. Fixtures are mistaken for production evidence: reports must identify synthetic content, temporary signing keys and loopback destinations.
7. A Windows lifecycle run touches an existing installation/profile: stop before mutation; require a task-owned scope and refuse unrelated registration.
8. Uninstall changes public PATH or removes synthetic user data: native lifecycle must fail.
9. An ARM64 guest is described as native x64: report the OS architecture and executable target separately.
10. A v2 build with a changed version is described as v1: require a real, hash-verified v1 asset or record NOT RUN.
11. A temporary updater key becomes production configuration: do not configure it without a durable custody channel and matching existing-key checks.

## End-to-end sequence

1. Drive the actual catalogue assembly and page-preview subprocesses with six-target synthetic records and real temporary Ed25519 archive signatures. Test the positive policy and each rejection above. Retain a report and SHA256SUMS.
2. Replay conditional upload/read-back, GitHub page and native updater process runners against loopback services.
3. Build task-owned Windows installers from the selected source and verify native install, reinstall, forward update, CLI/service ownership, PATH, data retention and removal in the identified VM.
4. Inspect configured credential names and public contracts without exposing values. Prepare exact candidates where available. Keep public writes separate.

## Implemented corrections

- Production catalogue assembly permits unsigned Windows payloads and setup EXEs for alpha and stable. False signature fields remain false. Pages explicitly state unsigned Authenticode. The macOS and raw updater signature gates remain mandatory.
- NSIS silent downgrade and in-use-file refusal messages have a silent default. Downgrades return exit code 2. Before the fix, the real Windows downgrade E2E hung for 15 seconds; the same E2E now rejects the downgrade and preserves the newer installation.
- Native replacement exposed a separate Windows migration defect. `url.URL` encoded `C:/...` as `file://C:/...`; SQLite rejected `C:` as an authority. The importer now uses an absolute path component and produces `file:///C:/...`, retaining read-only/query-only options. The unchanged downloaded CLI build 2 fails the real migration; a new local build 3 passes it. No accepted CLI file was changed in place.
- The canonical Windows lifecycle runner now supports an explicitly selected fresh VM root under SYSTEM, initializes the CLI profile through its public command, records installed digests/Authenticode state and verifies downgrade recovery. The existing updater E2E also accepts source/hash-bound prebuilt fixture tools for compiler-free native guests.

## Observed acceptance

| Layer | Result | Evidence and limit |
| --- | --- | --- |
| Production signing policy | PASS: 11 checks | Actual controller subprocesses; real temporary Ed25519 signatures; synthetic six-target files. Both page channels declare unsigned. Apple/updater/source/CLI/digest negatives still reject. |
| GitHub page controller | PASS: 18 checks | Actual gh process and isolated HTTP server; immutable source/assets, draft retry, read-back, publication/promotion gates and cleanup. No live GitHub writes. |
| Conditional delivery | PASS: 9 checks | Actual boto3, loopback S3/HTTP; all 22 files/catalogue/receipt uploaded and fully read back; conflicts/revisions rejected. No live R2 writes. |
| macOS native updater | PASS: 20 checks | Existing production adapter and actual mygo installation; temporary fixture key and loopback feed. No GUI or production feed. |
| Windows ARM64 native updater | PASS: 20 checks | Same runner, native ARM64 probe/signer, private embedded Python; signed download, identity/hash/signature rejection, actual owned-file replacement, data retention and cleanup. This is an updater probe, not a production-key-enabled Desktop UI. |
| Windows ARM64 NSIS lifecycle | PASS: 8 checks | Install, reinstall, real CLI/service IPC and shutdown, forward upgrade, downgrade recovery, PATH/ownership, uninstall and data retention. All three installed binary hashes match staged inputs; CLI/service match the exact accepted lock. Programs and installer report `NotSigned`. |
| Official v1.6.2 → v2 | PASS: 5 checks | Official ARM64 setup SHA-256 `24686225324422aa86f007e61c3b457ec1b686a369e027e39883d49b2f7fc45a`; actual v1 program replacement, frozen encrypted v3 history imported/reopened, source/data preserved and uninstall verified. This uses synthetic data, not a personal v1 profile. |
| macOS CLI/npm/migration regression | PASS: 46 checks | Existing real packager/npm/CLI process E2E; read-only migration, partial repair, retry and reopen; loopback registry. |
| Ruff/actionlint/gofmt/whitespace | PASS | Task Python files, changed workflows, importer formatting and `git diff --check`. |
| Documentation | PASS with 5 stale-page warnings | Release and pipeline pages verified at local implementation commit `474fd0136`; `go run ./tools/docscheck` passed. The remaining warnings cover desktop-runtime, headless, identity/trust, overview and real-machine-testing. They do not establish additional native acceptance. |

Small machine-readable receipts and exact hashes live in [the evidence directory](baselines/0018-unsigned-windows/summary.json). Verify them with `shasum -a 256 -c SHA256SUMS` from that directory.

## Candidate identities

The freshly fetched base is `1e90f041d1f90e3c62a41a4bb86b5503ebe00c20`. Current canonical run `44496820246` supplied all six CLI build-2 targets, with manifest SHA-256 `2833ca5db80a143c17cee12ab877a0ded0b38561a88fcf685286b52772a69ad7`. Those macOS programs are unsigned; this is native construction evidence, not a production signing candidate.

The migration correction required a new **local** CLI `2.0.0 (3)`. It contains only `windows-arm64`, has native version/alias acceptance, and records `source_clean=false`. A separate development Desktop locks its exact manifest and reuses both executables unchanged. Its official-v1 replacement setup SHA-256 is `ad17d9539529197958aa016c11b6dc2936ca061ac8149b2872e105f871a21496`. The [summary](baselines/0018-unsigned-windows/summary.json) records the exact manifest/package/binary hashes. These task candidates embed the base SHA and bind dirty source files by hash. They are not clean six-target production candidates. The checked-in production CLI lock remains unchanged; it must be replaced only after a new complete signed-macOS CLI candidate is accepted.

## VM identity and isolation

VM `32affa85-5174-4d5b-ab6b-903a06d7fa9b` is Windows 11 Pro ARM64, four virtual CPUs and 6144 MiB memory. Execution used Parallels guest tools as `S-1-5-18` (SYSTEM), session 0. No personal account, credentials, host clipboard or interactive desktop was used. This is native ARM64 process evidence; x64 emulation and GUI acceptance are NOT RUN. The official v1 installer used `/S /UPDATE` to omit the unrelated WebView2 bootstrap, without `/R` or GUI launch. v1's executable has no FileVersion resource; its official installer hash and actual uninstall registration establish v1 identity.

Shared clipboard and personal-directory sharing were already enabled. They were not changed. Only exact task paths under `C:\ProgramData\UniClipboard-*` and the task worktree share were used. Host load was 11.48/9.47/7.32 during concurrent work; timings are not performance evidence. Script execution used process-scoped `RemoteSigned`, without changing machine/user execution policy or bypassing UAC/group policy. Failed-run cleanup left one stale task-only registration; its exact path was verified absent before removing that registration. The final replacement run independently verified registration and directory removal.

## Production blockers and remaining authorization

The corrected [credential/route audit](baselines/0018-unsigned-windows/credentials-and-routes.json) supersedes the earlier blanket missing-release-credentials conclusion. Existing repository Cloudflare token/account names and repository-shared organization Access names are configured. The repository-scoped organization-secret endpoint confirms both Access names now; v1 run `43187269797` also records actual successful upload and registration. Current external validity and S3/numbered-contract suitability remain unverified. The current v2 S3 aliases/endpoint variables and raw updater configuration are absent in the inspected effective Actions scopes; this is a wiring gap, not proof that new Cloudflare/Access credentials must be created. See [credential reconciliation and exact transport differences](0018-release-credential-reconciliation.md). No private value was retrieved or derived, and no production key was generated. Existing Minisign/Tauri and test-certificate names remain separate from raw Ed25519 updater configuration.

Public `/contract`, `/v1/desktop/alpha.json` and `/v1/desktop/stable.json` returned HTTP 403. Authenticated admin contract, v2 R2 upload/full download, public numbered selection and version-query behavior remain NOT RUN. This observation establishes no DNS/TLS/account-suspension diagnosis.

At acceptance time, branch publication and manual nonpublishing canonical acceptance CI still required separate authorization. For production delivery, construct a clean, complete new CLI build (including the Windows migration correction), sign both macOS targets with the existing Developer ID identity, accept its immutable manifest, and then update the Desktop lock. Check approved durable custody for an existing raw Ed25519 key before any generation; configure the matching private/public pair through the approved secret interface and commit only the public key. Reuse the configured organization-shared Access pair; resolve the S3 credential/endpoint wiring through the documented reuse check before requesting new credentials. Verify numbered contracts read-only before proposing any deployment. Construct/accept all 22 Desktop formats with macOS signing/notarization and the real updater key. Windows may remain explicitly unsigned.

Public npm/GitHub/R2 delivery, Worker deployment, alpha/stable/latest mutation, package-manager submissions and default-branch changes remain separately unauthorized. Stable also retains its per-format lifecycle and GUI requirements. Live winget/Chocolatey/Scoop, Windows interactive setup/credential prompts and x64 emulation remain NOT RUN.

## Replay

```sh
python3 e2e/packaging/signing_policy.py
python3 e2e/packaging/github_release.py
python3 -m venv /private/tmp/unic-release-tools
/private/tmp/unic-release-tools/bin/pip install -r tools/release/requirements.txt
/private/tmp/unic-release-tools/bin/python e2e/packaging/publication.py
python3 e2e/packaging/updates.py
python3 e2e/release/run.py
```

Native Windows replay entries are [the canonical lifecycle runner](../../e2e/packaging/windows-lifecycle.ps1), [the actual-v1 replacement runner](../../e2e/packaging/windows-v1-replacement.ps1) and `updates.py --prebuilt`. Inputs and their explicit isolation requirements are documented in [packaging acceptance](../../e2e/packaging/README.md). Generate a fresh fixture/key and fresh task roots for each replay. The captured digests identify this run; fresh random fixture keys produce new archive hashes.

## Cleanup

Windows task directories, synthetic databases, temporary keys, input programs and test registration were removed. No task process remained. The VM was restored to suspended; the other Parallels macOS VM remained stopped. Host candidate archives, programs, synthetic databases, fixture keys, downloaded Python and source subsets were removed. Only small redacted receipts/checksums and source changes remain. Task compiler/npm caches and the private Python venv were removed after final checks. Other worktrees and D-group VMs remain untouched.

