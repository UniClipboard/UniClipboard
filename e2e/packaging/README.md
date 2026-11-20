# Desktop packaging acceptance

Read [CASES.md](CASES.md) before modifying packaging behavior.

```sh
python3 e2e/packaging/run.py
(cd .cache/package-e2e && shasum -a 256 -c SHA256SUMS)
```

The runner uses real Go/npm processes and a temporary exact CLI lock. It builds Desktop but never starts a GUI. It retains a report with source hashes, commands, assertions and explicit untested boundaries. All temporary binaries and npm packages are removed on success and failure.

Set `GOCACHE` and `npm_config_cache` to writable task-owned locations when the host's normal caches are unavailable.
On Linux, add `--format deb` with pinned nFPM on PATH to construct and hash a real DEB. The lifecycle helper must run only in a disposable Debian-family container:

```sh
sh e2e/packaging/linux-lifecycle.sh /in/build-1.deb /in/build-2.deb /out/receipt
```

This installs packages and needs container root. It does not launch Desktop windows. Container execution is not a physical-desktop or native-installer-replacement result. The receipt distinguishes these limits.

Use repeated `--format` arguments to construct multiple native formats. `--lifecycle` requires `--format deb` inside a disposable container and verifies a real build-1 to build-2 upgrade. It attaches the receipt only to matching package hashes.

`python3 e2e/packaging/channels.py` executes the real renderer with synthetic artifact bytes. It verifies twelve-channel coverage and rejection of conflicting inputs; it does not validate native packages.
`python3 e2e/packaging/windows-build.py` cross-compiles actual amd64/arm64 PE payloads and constructs NSIS/ZIP packages. It does not execute Windows programs or accept their installer lifecycle.

## One device-test package

Dispatch the registered entry against the reviewed branch (use `v2` after merge):

```sh
gh workflow run build-cli.yml --repo UniClipboard/UniClipboard --ref v2 \
  --field test_target=windows-arm64 --field test_format=setup.exe
```

Supported targets are `darwin-arm64`, `darwin-amd64`, `windows-arm64`, `windows-amd64`, `linux-arm64` and `linux-amd64`. The target contains both OS and architecture. `test_format=default` selects DMG/setup EXE/DEB. Explicit formats use the existing target-compatible constructor. Leave `distribution`, `desktop_only`, `native_gui` and `sandbox_run_id` unset. The repository still defaults to v1; use the explicit `--ref v2` command rather than changing the default branch or relying on the default-branch dispatch form.

Download `desktop-test-<target>` from the completed Actions run. Read `TESTING.md` and verify `SHA256SUMS`. The bundle is unsigned/test-only and expires after seven days. Automatic updates are disabled. The existing application installation identity is retained; use an isolated test account/machine or installation path and explicit `UNIC_HOME`.

Local native construction and verification use the same controller:

```sh
python3 tools/package/desktop_test.py plan --target darwin-arm64
python3 tools/package/desktop_test.py build --target darwin-arm64 --format app.tar.gz --output dist/test-package
python3 tools/package/desktop_test.py verify --directory dist/test-package
python3 e2e/packaging/test_package.py
```

The archive-content E2E runs on macOS/Linux. It invokes real CLI/npm/Desktop/package processes, checks packaged CLI bytes against the exact manifest, verifies source/metadata preservation and rejects corrupt/extra bundle files. It keeps a small report/checksum receipt and removes temporary candidate bytes. It does not launch a GUI or accept installation.

`python3 e2e/packaging/signing_policy.py` drives production-mode catalogue and page controllers with synthetic six-target records and real temporary raw Ed25519 signatures. It accepts unsigned Windows without relaxing macOS or updater gates.

`windows-lifecycle.ps1 -IsolatedVMRoot C:\ProgramData\UniClipboard-Acceptance-<task>` permits the canonical runner in an explicitly authorized VM under SYSTEM only. It refuses existing installations and roots. Use task-owned inputs and receipts. This is native process acceptance, not an interactive desktop session.

`windows-v1-replacement.ps1` accepts a separately downloaded official v1 installer and its independently verified SHA-256. It installs v1 in a private location, replaces it with v2, imports frozen synthetic v3 history through the actual bundled CLI, verifies reopened content, then checks that uninstall preserves v1/v2 data. Pass the source fixture directory and the existing `internal/v1migration/testdata/v3.json` as fixture metadata; passphrases travel through stdin. The fixture can be created with `frozen_source` in `e2e/release/migration_acceptance.py`; remove its intentionally invalid `entry-bad` row for a complete-import case. No personal credentials or profile is used. `/UPDATE` excludes WebView2 bootstrap; no GUI is started.

For compiler-free native hosts, `updates.py --prebuilt <directory>` consumes `sign.exe`, `probe.exe`, `keys/` and `input.json`. Cross-build the existing signer and probe for the guest target. Embed the temporary fixture public key using the same linker flags as the default runner. The input receipt binds source SHA, the runner/Go source hashes and both executable hashes. Source paths use `/` on all hosts. The runner rejects mismatched sources, target or executables and runs the same signed-download, failure and actual mygo-install scenarios. These keys are fixture-only. Destroy them and the prebuilt binaries after acceptance.
