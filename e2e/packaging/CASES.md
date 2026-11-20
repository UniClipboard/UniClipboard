# Package lifecycle failure inventory

For one-target construction failures and replay, read [single-target test package acceptance](../../docs/exec/0018-single-target-test-package.md). Run `python3 e2e/packaging/test_package.py` on macOS/Linux before changing that controller.

Write these expectations before implementing production packaging.

1. Missing CLI manifest, wrong lock digest, wrong CLI version/build, duplicate target, unsafe archive member, changed binary or missing acceptance: reject staging before building Desktop.
2. A CLI input or Desktop source changes during construction: reject the output; never overwrite accepted output paths.
3. Desktop build rebuilds or re-signs a locked CLI file: compare staged hashes with the accepted CLI package and reject.
4. Desktop and independent CLI expose the same owned files: install order and uninstall order must preserve the independent CLI.
5. Package versions sort an increasing build or product version backwards: actual package-manager comparisons must reject the encoding.
6. Linux package has missing runtime dependencies: clean package-manager installation must fail acceptance, even if --version works.
7. macOS bundle has an incorrect app ID, missing sibling service or unverified signing: bundle inspection and native acceptance remain separate.
8. Windows portable executable uses normal data paths: launch relocated programs and verify executable-relative portable roots, with explicit UNIC_HOME taking precedence.
9. Linux install/upgrade/remove modifies unrelated service/data or removes user data: fail lifecycle acceptance.
10. Corrupt or unsigned repository metadata is accepted as trusted: fail repository acceptance.
11. Channel manifests contain placeholder hashes, mutable URLs or a wrong target: fail rendering/preflight.
12. Sandbox builds without clipboard/IPC/network permission acceptance: retain pending capability status; do not publish stable.
13. Incomplete candidate or altered package is promoted: reject before any external write.
14. Source-built outputs are reported as the upstream binary build: reject provenance classification.
15. Unsupported format/tool/platform/signing prerequisites: fail clearly without fabricating an artifact.

## Process entrypoint

`python3 e2e/packaging/run.py` builds the real native CLI, assembles its npm candidate, locks it in temporary metadata, builds Desktop, verifies identities without starting a GUI, and runs malformed-lock/input cases.
Use `--format deb` on Linux with nFPM to build the real package and inspect/install it in a disposable container with `linux-lifecycle.sh`.
The report and checksums are retained. Temporary binaries, profiles and npm archives are removed.
Public destinations, native windows and system clipboard acceptance are separate requirements.

## Signing, release and updater follow-up failures

- Missing/invalid Apple certificate or keychain password must fail without logging private material; temporary keychains must be removed on success and failure.
- Notary rejection/timeout must not mark a package notarized. Stapling must precede final hashing. Locked CLI files must remain byte-identical.
- A CI CLI candidate with the wrong workflow/run/repository/SHA, missing target or altered package must fail lock selection before metadata changes.
- Mixed Desktop sources, missing formats, absent lifecycle/signing receipts or an untrusted updater key must fail final assembly.
- A destination object with different bytes must never be overwritten; retry of identical bytes must resume; partial upload must not move a channel.
- A live FlareRelease contract mismatch, stale revision or missing stable CLI must prevent channel mutation.
- The updater must reject corrupt signatures, oversized responses, wrong format/target and downgrades; package-managed/Scoop/portable installs must not replace manager-owned files.
- Reuse mygo's native updater and authenticated extraction/swap implementation. Its raw Ed25519/SHA256 protocol must not be confused with Minisign.

## GitHub Release page migration

Before implementation, require real Python release subprocesses and the actual GitHub CLI against disposable HTTP endpoints. Cover v1-style page rendering; all 22 native attachments, manifest, checksums and six raw updater signatures; exact CLI references; immutable tag/source binding; idempotent interrupted draft recovery; changed/extra/corrupt assets; publication without availability; stable without promotion; stable-to-alpha downgrade; and isolated fixture credentials. Missing evidence or cleanup fails. Never create a real Release in this fixture.

## Desktop icon reuse

- A placeholder or unrelated icon replaces the v1 artwork: compare staged resources with the canonical Desktop assets.
- macOS omits its icon declaration or resource: inspect the actual bundle plist and ICNS digest.
- Linux packages or AppImage retain the placeholder SVG: require the v1 PNG in the package recipe.
- Windows loses executable icon resources: cross-build both architectures and inspect PE resource types.
- Runtime artwork differs from the packaged artwork: share one embedded PNG between the window, Dock and About view.
- Tray artwork loses template contrast: retain mygo template handling and the v1 macOS padding.
- Native visual acceptance requires separate authorization; process acceptance must not start the GUI.

## Unsigned Windows policy

Run `python3 e2e/packaging/signing_policy.py`. Real production-mode assembly and page-preview subprocesses must accept unsigned Windows for both channels and retain false signature fields. Real temporary Ed25519 signatures must verify. Missing macOS signing/notarization, missing/corrupt updater signatures, changed bytes, mixed source and wrong CLI locks must fail. All files are synthetic fixtures; no production key or remote mutation is used.

The canonical Windows lifecycle runner accepts `-IsolatedVMRoot C:\ProgramData\UniClipboard-Acceptance-<task>` only under SYSTEM in an explicitly authorized disposable VM scope. It refuses an existing root or installation. It initializes the task CLI profile explicitly, checks silent downgrade rejection and preserves the newer installation. Interactive personal accounts are rejected. This path establishes native process lifecycle, not interactive GUI or v1 replacement.
