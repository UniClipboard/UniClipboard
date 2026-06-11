# UniClipboard CLI

Cross-platform clipboard sync. This package installs the `uniclip` command
and the `uniclipd` daemon for your platform via optional dependencies.

```sh
npm install -g @uniclipboard/cli
uniclip --help
uniclip start
```

## Supported platforms

| Platform | Arch | Package |
| --- | --- | --- |
| macOS | arm64 | `@uniclipboard/cli-darwin-arm64` |
| macOS | x64 | `@uniclipboard/cli-darwin-x64` |
| Linux (static musl, works on glibc/musl) | arm64 | `@uniclipboard/cli-linux-arm64` |
| Linux (static musl, works on glibc/musl) | x64 | `@uniclipboard/cli-linux-x64` |
| Windows | x64 | `@uniclipboard/cli-win32-x64` |

Only the package matching your platform is downloaded. If your platform is
not listed, prebuilt binaries and other install channels (Homebrew, Snap,
COPR, AUR) are available at
[github.com/UniClipboard/UniClipboard](https://github.com/UniClipboard/UniClipboard/releases).

## Verifying binaries

The binaries shipped in the platform packages are byte-identical to the
`uniclipboard-cli-*` archives attached to the corresponding GitHub release,
which are covered by a minisign-signed `SHA256SUMS.txt`. npm packages are
published with [provenance](https://docs.npmjs.com/generating-provenance-statements)
from GitHub Actions.

## License

AGPL-3.0-only
