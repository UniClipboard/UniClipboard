# CLI push, native build and candidate verification

## Result

Pushed `cli-release-channels` to origin. The verified code commit is `be4353ba672a0fc41d57dc8e22bf1e67feef087e`.

[Build CLI run 42777109814](https://github.com/UniClipboard/UniClipboard/actions/runs/42777109814), attempt 2, completed successfully. All five native targets passed: macOS arm64/amd64, Linux arm64/amd64 and Windows amd64. Native acceptance checks CLI, service, migration helper and command alias identities.

Linux production headless acceptance and all 45 release/upgrade process checks passed. The downloaded report's ZIP digest, report digest and 42 source hashes were independently verified against the code worktree.

The npm candidate contains five platform packages and the main package at `2.0.0-build.1`. Independent read-back verified the candidate ZIP digest, every package's SHA-256/SHA-512, all bundled executable hashes, exact main dependencies and command entrypoints. Every target has clean source and the same canonical LF metadata digest.

Actual offline installation of the macOS arm64 candidate passed. Installed `unic`, `uniclip` and `unicd` reported `2.0.0 (1)` with the verified source SHA. Explicit-package `npx` invocation also passed offline. This is candidate availability in the downloaded artifact, not public npm alpha availability.

The [small verification receipt](baselines/0018-cli-remote-verification.json) records run/job/artifact identities, hashes, package identities, native execution and remaining permission verification.

## Failures retained in the record

The [first run](https://github.com/UniClipboard/UniClipboard/actions/runs/42775669691) passed native checks but failed npm assembly. Windows checked out release TOML with CRLF. Its raw metadata digest differed from the LF digest used by assembly. Git still reported clean source. The Windows artifact digest and its manifest confirmed the exact CRLF mismatch.

Added a real Git checkout regression with `core.autocrlf=true` before the fix. It failed. `.gitattributes` now requires LF checkout bytes for release TOML. The regression and full 45-check process suite passed after the fix. Digest validation remains intact. The new native run passed assembly without special-case normalization.

Attempt 1 of the fixed run timed out waiting for a text receipt in production headless acceptance. The same commit passed on retry. No timeout, assertion or runtime code was changed to obtain that result. The intermittent receive timeout remains recorded.

## Publication prerequisites

Set the public repository variable `POSTHOG_HOST=https://us.i.posthog.com`, matching the already accepted US Cloud project. The `POSTHOG_PROJECT_KEY` secret name exists. No secret value was read or changed.

This was a CI build with distribution metadata disabled. The release workflow will build its publication candidate with the configured deployment metadata and the same acceptance gates.

Public npm channels remain alpha `1.6.2-alpha.1` and latest `1.6.1`. `2.0.0-build.1` is not published. No release or promotion workflow was invoked.

The local npm identity query and unauthenticated trust-configuration query returned HTTP 401. This prevents inspecting publisher permissions; it does not prove that GitHub OIDC publishing is misconfigured. An authenticated package owner must allow a read of the six packages' trust settings. Check `release-cli.yml` for direct publishing and dist-tag management, and `promote.yml` for dist-tag management. The request for an npm login remains pending.

## Offline candidate replay on macOS arm64

Use Node 22.23.4 or a supported newer version. Download the exact run artifact, verify its files against `release-manifest.json`, then install only local package files:

```sh
cli_check_dir=$(mktemp -d)
gh run download 42777109814 --repo UniClipboard/UniClipboard --name cli-npm-candidate --dir "$cli_check_dir/packages"
cd "$cli_check_dir"
npm install --offline --ignore-scripts --omit=optional --no-audit --no-fund \
  ./packages/uniclipboard-cli-2.0.0-build.1.tgz \
  ./packages/uniclipboard-cli-darwin-arm64-2.0.0-build.1.tgz
npx --offline --package=@uniclipboard/cli@2.0.0-build.1 unic --version --json
```

Other platforms require their matching platform package. CI artifacts expire; the receipt retains the observed identities and hashes. Temporary archives, installed binaries and the standalone verification prototype were removed after verification. No GUI or personal v1 profile was accessed.
