# Single-target Desktop test packages

Status: implementation in progress. Base: `5158490dbf3f29413315d89edc7252af90f73ccf` (`origin/v2`).

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

## Evidence

Pending.
