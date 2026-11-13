# v2 packaging

Production Desktop construction reuses an exact accepted CLI manifest and never rebuilds its CLI/service files. Versions, builds and targets come from `release/products.toml`.

```sh
# The metadata must contain the real CLI manifest digest and product identity.
go run ./tools/package -product desktop -target linux-amd64 \
  -cli-manifest /path/to/cli/release-manifest.json -output dist/desktop-stage
python3 tools/package/desktop.py smoke --stage dist/desktop-stage
python3 tools/package/install-tools.py --output .cache/package-tools/nfpm
python3 tools/package/desktop.py package --stage dist/desktop-stage \
  --format deb --nfpm .cache/package-tools/nfpm --output dist/desktop-deb
```

`-development` allows dirty source for a local candidate and marks it as such. It cannot establish a release. Existing output directories are rejected. Staging and packaging detect input tampering.

Supported construction entrypoints: DEB, RPM, pacman binary package, Windows NSIS setup EXE, portable ZIP, macOS DMG/app archive, and Linux AppImage/update archive. Tools and platforms are required per format. Format construction is separate from signing, installation, sandbox capability and public publication acceptance. AppImage currently requires the host GTK3 runtime; self-contained runtime acceptance is pending.

Generate channel definitions only from final files listed in verified `artifacts.json` records:

```sh
python3 tools/package/channels.py --input dist/desktop-packages \
  --base-url https://downloads.example.org/desktop/2.0.0/build-1 \
  --output dist/channel-definitions
```

The example URL must be replaced with the actual immutable destination. The renderer does not publish, upload or claim remote URL availability. It rejects mixed identities, missing formats and modified files. `channels.json` keeps installation and publication states pending. Generated definitions require real native tool and package-manager validation before submission.

The existing Homebrew Tap contains both a `uniclipboard` CLI Formula and a Desktop Cask. Desktop Casks own only their application bundle; never overwrite the Formula's public command files.

The source `packaging/aur/uniclipboard-git/PKGBUILD` builds Desktop from the selected v2 revision and fetches that revision's exact CLI lock. The generated binary recipe has separate provenance. Clean source-build acceptance, sandbox acceptance and channel publication remain explicit open work in the execution receipt.

See the [release contract](../docs/contracts/release-contract.md), [execution plan](../docs/exec/0018-v2-package-channel-plan.md) and [process acceptance](../e2e/packaging/README.md).

CLI signing can be requested with `UNIC_MACOS_SIGNING_IDENTITY` or `UNIC_WINDOWS_CERT_SHA1` before CLI archive construction. Desktop signing uses `desktop.py sign --stage ... --identity ...`, verifies locked CLI distribution signatures and never changes those files. Re-run `smoke` after signing. Real signing and notarization acceptance remain pending; the current build workflow constructs unsigned evidence only.

AppImage tool and runtime downloads are pinned by SHA-256 in `tools.json`. Their upstream continuous URLs may change; digest mismatches intentionally stop construction. nFPM uses fixed version 2.49.0. NSIS versions are recorded by cross-construction acceptance; hosted execution is a separate result.
