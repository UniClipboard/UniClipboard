# npm distribution

Maintainer notes for the npm packages. End-user docs live in
`uniclipboard/README.md` (shipped with the main package).

## Layout

Six packages per release, following the esbuild/biome pattern:

| Package | Contents |
| --- | --- |
| `uniclipboard` | JS launcher only (`launcher.js`); selects the platform package via `optionalDependencies` |
| `@uniclipboard/cli-darwin-arm64` | `bin/uniclip` + `bin/uniclipd` (aarch64-apple-darwin) |
| `@uniclipboard/cli-darwin-x64` | same (x86_64-apple-darwin) |
| `@uniclipboard/cli-linux-arm64` | same (aarch64-unknown-linux-musl, static) |
| `@uniclipboard/cli-linux-x64` | same (x86_64-unknown-linux-musl, static) |
| `@uniclipboard/cli-win32-x64` | `bin/uniclip.exe` + `bin/uniclipd.exe` (x86_64-pc-windows-msvc) |

Only `npm/uniclipboard/` is checked in (with `0.0.0-dev` placeholders).
Platform packages are generated at publish time by
`scripts/build-npm-packages.mjs` from the `uniclipboard-cli-*` release
archives — the binaries are byte-identical to the GitHub release assets and
covered by the same signed `SHA256SUMS.txt`.

Invariants the script enforces:

- `uniclip` and `uniclipd` ship side by side in `bin/` — `uniclip start`
  resolves the daemon as a sibling of `current_exe()` (ADR-008 D13). This is
  also why the launcher spawns the binary by real path instead of letting npm
  symlink/shim it.
- Platform packages are pinned to the **exact** release version in the main
  package's `optionalDependencies` (no `^`/`~`).
- Platform packages publish before the main package (`publish-order.txt`).

## Publish flow

`.github/workflows/npm-publish.yml` listens for `release: published`
(stable only — same policy as homebrew-tap.yml) and can be dispatched
manually for any published release, including prereleases. dist-tag is
derived from the version suffix: `0.15.0-alpha.3` → `alpha`, stable →
`latest`. Re-runs are idempotent (already-published versions are skipped).

## One-time setup

1. Create the npm org `uniclipboard` (needed for the `@uniclipboard` scope):
   https://www.npmjs.com/org/create
2. Create a granular automation token with read/write access to the
   `uniclipboard` package and the `@uniclipboard` scope.
3. Add it as the `NPM_TOKEN` repository secret.

The very first publish of each package must create it; after that, consider
enabling npm "trusted publishing" (OIDC) per package and dropping the token.

## Local testing

```sh
node scripts/build-npm-packages.mjs \
  --version <X.Y.Z> --artifacts-dir <dir-with-cli-archives> --out npm-dist
```
