# npm distribution

Maintainer notes for the npm packages. End-user docs live in
`uniclipboard/README.md` (shipped with the main package).

## Layout

Six packages per release, following the esbuild/biome pattern:

| Package | Contents |
| --- | --- |
| `@uniclipboard/cli` | JS launcher only (`launcher.mjs`); selects the platform package via `optionalDependencies` |
| `@uniclipboard/cli-darwin-arm64` | `bin/uniclip` + `bin/uniclipd` (aarch64-apple-darwin) |
| `@uniclipboard/cli-darwin-x64` | same (x86_64-apple-darwin) |
| `@uniclipboard/cli-linux-arm64` | same (aarch64-unknown-linux-musl, static) |
| `@uniclipboard/cli-linux-x64` | same (x86_64-unknown-linux-musl, static) |
| `@uniclipboard/cli-win32-x64` | `bin/uniclip.exe` + `bin/uniclipd.exe` (x86_64-pc-windows-msvc) |

The main package is scoped because the unscoped name `uniclipboard` is
permanently rejected by npm's typosquatting rule (E403: too similar to the
existing `uni-clipboard` — names are compared with punctuation stripped).

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

## Auth: OIDC trusted publishing (no token)

CI auth is npm [trusted publishing](https://docs.npmjs.com/trusted-publishers/)
via GitHub Actions OIDC — there is no NPM_TOKEN. Token-based publishing is a
dead end: npm requires 2FA/OTP for token publishes and the granular-token
"bypass 2FA" flag is broken server-side (npm/cli#8869, npm/cli#9268).

Each of the six packages must have a Trusted Publisher configured at
`https://www.npmjs.com/package/<name>/access`:
Organization `UniClipboard`, repository `UniClipboard`, workflow
`npm-publish.yml`, environment empty.

Trusted publishing cannot create a package (npm/cli#8544), so the first
version of any NEW package must be published manually from a maintainer
machine (interactive 2FA), after which its Trusted Publisher can be
configured. This was done for all six packages with 0.15.0-alpha.1.

## Local testing

```sh
node scripts/build-npm-packages.mjs \
  --version <X.Y.Z> --artifacts-dir <dir-with-cli-archives> --out npm-dist
```
