# Single-target Desktop test packages

Status: implemented; native construction and downloaded-byte acceptance passed for three platforms. Final Windows instruction-revision construction and read-back also passed. Base: `5158490dbf3f29413315d89edc7252af90f73ccf` (`origin/v2`).

## Failure inventory before implementation

1. A manual single-target request starts the six-target CLI/Desktop matrix: route it to one native runner and suppress the other construction paths.
2. Unsupported targets, target/format mismatches or conflicting legacy dispatch options consume native build time: reject selection before selecting the native runner or installing format tools.
3. Desktop contains an old CLI or a different-source service: build CLI from the same checkout, run native version/alias acceptance, assemble one exact temporary manifest, lock that digest, and compare staged executable hashes.
4. A test build edits checked-in versions, CLI locks or public update keys: generate temporary single-target metadata and verify original metadata bytes remain unchanged.
5. A package is mistaken for a release or uses a configured production updater: set development provenance, omit the updater public key, strip signing/upload configuration from child build processes, and label the downloadable bundle test-only/unsigned.
6. A missing signature is falsely described as signed: preserve actual signature fields; no test certificate or temporary updater key establishes production trust.
7. A package or receipt changes after construction/download: verify a complete SHA256SUMS inventory and the selected artifact's exact size/hash; reject changed bytes and extra files.
8. A rerun overwrites an output directory or accepts moving source: refuse existing outputs; keep existing source, metadata and immutable CLI checks active.
9. Native smoke initializes personal data or starts a GUI: reuse state-free version checks and the existing isolated CLI initialization/shared-configuration IPC smoke only.
10. A failed build leaves private source, profiles, npm packages or executables in scratch directories: use a temporary workspace and keep only a small failed-run receipt at the requested output.
11. A test installation is described as isolated by `--development`: retain the existing installation identity and document explicit `UNIC_HOME`/installation isolation for real-device testing.

## End-to-end acceptance

Use real tool subprocesses to select all six targets and reject invalid selections. On the native host, build the actual CLI and Desktop from the same checkout, construct one application archive, inspect its CLI bytes against the exact locked manifest, and verify the downloadable inventory. Reject existing outputs, wrong-host construction and tampered/extra bundle files. Retain a report and checksums; delete private candidate bytes after local acceptance. Separately dispatch the registered GitHub build entry for one target, verify that only that native job ran, download the artifact and repeat full checksum verification.

## Implementation and use

PR: [2275](https://github.com/UniClipboard/UniClipboard/pull/2275), targeting `v2`. Implementation commit: `5e2086369`. Final installer/checksum guidance is in `1d5976240`; the architecture is verified against that commit.

Use the registered Build CLI workflow. Select one target and a compatible format. Until the PR is merged, use the feature branch; after merge, use `v2`:

```sh
gh workflow run build-cli.yml --repo UniClipboard/UniClipboard \
  --ref feat/desktop-test-package \
  --field test_target=windows-arm64 --field test_format=setup.exe
```

Targets: `darwin-arm64`, `darwin-amd64`, `windows-arm64`, `windows-amd64`, `linux-arm64`, `linux-amd64`. `default` selects DMG/setup EXE/DEB. Compatible explicit formats reuse the current packager. Leave distribution, desktop-only, native-window and sandbox inputs unset. `test_target=none` preserves normal CLI construction.

One lightweight Ubuntu selection job validates the request, then exactly one native package job runs. It builds the current source CLI, accepts its native executable identities/alias, assembles a temporary exact npm manifest and locks that digest into Desktop. It builds one requested format and uploads `desktop-test-<target>` for seven days. No preexisting release CLI, production signing secret, updater key or public registry package is required. Checked-in release metadata remains unchanged. This path has read-only repository permissions and no publication credentials.

The package retains the current application installation identity. Read `TESTING.md` before installing. Use a dedicated test machine/account for installers, and an explicit test `UNIC_HOME`; a custom path does not isolate a registered Windows installation. Archives may use a separate folder. The package is unsigned/test-only and has no automatic updater key. Production release gates remain unchanged.

## Observed acceptance

| Check | Result | Exact evidence and limits |
| --- | --- | --- |
| Native local controller/archive E2E | PASS: 20 checks | Real macOS ARM64 CLI/npm/Desktop/archive processes; target selection, source and exact CLI lock, native private-service IPC, unchanged checked-in metadata, archive CLI byte inspection and changed/extra file rejection. No GUI or installation. |
| Linux AMD64 DEB | PASS | Run [44658399851](https://github.com/UniClipboard/UniClipboard/actions/runs/44658399851), one native job, 155 seconds for that job. Downloaded complete inventory and CLI/stage identities verified. |
| macOS ARM64 DMG | PASS | Run [44658401620](https://github.com/UniClipboard/UniClipboard/actions/runs/44658401620), one native job, 228 seconds. Downloaded DMG and full bundle verified without mounting or launch. No Developer ID/notarization. |
| Windows ARM64 setup EXE | PASS | Run [44658401848](https://github.com/UniClipboard/UniClipboard/actions/runs/44658401848), one native job, 406 seconds. Downloaded EXE and full bundle verified without installation or execution. No production Authenticode. |
| Wrong Windows/DMG selection | PASS: expected rejection | Run [44658460480](https://github.com/UniClipboard/UniClipboard/actions/runs/44658460480) failed with `format does not match target`; zero native jobs and no artifacts. |
| Test target plus distribution | PASS: expected rejection | Run [44658462814](https://github.com/UniClipboard/UniClipboard/actions/runs/44658462814) failed in the conflict guard; zero native jobs and no artifacts. No signing secret was consumed. |
| Final Windows guidance revision | PASS | Run [44659128683](https://github.com/UniClipboard/UniClipboard/actions/runs/44659128683), source `322986d4dc77b17377b4df8f8c8e9f1e9426a954`, one native job in 272 seconds. The downloaded complete bundle passed verification. Its controller hash matches the final source; PowerShell checksum and installer-isolation guidance are included. |
| Ruff/actionlint/whitespace | PASS | New Python controller/E2E, changed workflows and `git diff --check`. |
| docscheck | PASS with 6 stale-page warnings | Release architecture verification updated. Remaining warnings cover desktop-runtime, headless, identity/trust, observability, overview and real-machine-testing. |

The initial three positive runs use source `ca1d79d6961c4b2f4c84a4ce252db113559a824e`. All normal CLI assembly, six-target Desktop, sandbox and native-window paths were skipped. Each positive run exported only its selected `desktop-test-<target>` artifact. The reported seconds describe individual observed native jobs with their cache/dependency conditions, excluding queue time; they are not a build-time guarantee.

Small source/CLI/package/download receipts are in [the checksummed evidence directory](baselines/0018-single-target-test-package/summary.json). The final [Windows ARM64 artifact](https://github.com/UniClipboard/UniClipboard/actions/runs/44659128683/artifacts/14594956780) contains setup EXE SHA-256 `86ac1faec4c93de0bbd1b617c9f91f580d1fc95e6023dcee3cdd540d8adb1cf4` and the full input/checksum bundle. Artifact downloads were verified with the same `desktop_test.py verify` controller. Receipt source SHAs and run IDs matched GitHub, and the original CLI hashes matched stage inputs. No CLI archive was rebuilt or relabelled after acceptance. Test outputs are not production catalogues and were not published to npm, R2, GitHub Releases or a channel.

## Replay and cleanup

```sh
python3 tools/package/desktop_test.py plan --target darwin-arm64
python3 e2e/packaging/test_package.py
python3 tools/package/desktop_test.py verify --directory <download-directory>
```

From the evidence directory, run `shasum -a 256 -c SHA256SUMS` to verify the retained small receipts. Native target/format construction requires the matching host. NSIS is installed only for setup EXE; Linux package formats use existing pinned tools. No new runtime or build dependency was added.

GUI, real-device installation, native credential prompts and production signing remain NOT RUN for this feature. They are separate from construction and byte read-back. No local VM or application was launched. Local candidate profiles/npm archives/programs were removed by the E2E runner. Downloaded candidate bytes, task caches and the temporary hosted-acceptance helper were removed after read-back. Only small redacted receipts/checksums remain locally; Actions retains test artifacts for seven days.
