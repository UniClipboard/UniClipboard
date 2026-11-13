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
