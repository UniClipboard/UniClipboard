# PR 2231 merge acceptance

## Resolution

Merge the current `v2` branch into `cli-release-channels`. Preserve the upstream user-intent CLI, temporary service lifecycle, in-process migration importer and license text. Keep early release identity output and the v1 upgrade notice. Package only `unic` and `unicd`, plus the `uniclip` alias; migration runs inside `unic` and retains partial-result exit code 3. Update the release architecture and contract to match.

## Verification

- `go build ./app/service/cmd/unic ./app/service/cmd/unicd` passed.
- `python3 e2e/release/run.py --output .cache/pr2231-merge-release` passed all 45 process checks. This includes real local npm installation, migration preflight/import/retry, publication failure recovery, promotion and immutable record recovery. The report and SHA256SUMS allow replay verification.
- `make headless-e2e` passed `TestProductionHeadless` with race detection. The subprocess fixture is intentionally skipped as a standalone test. The manifest is `.cache/0015-acceptance/manifest.json`; all 13 production checks passed and source hashes were preserved.
- The five-platform native acceptance from the earlier receipt predates this merge. Native cross-platform acceptance must run again in CI. Public npm publication, GUI and device acceptance were not run.

Existing whitespace warnings inside the upstream stored probe patch are preserved. They are patch content, outside this conflict resolution.

Documentation validation passed with two existing upstream stale-page warnings in `pipeline.md` and `real-machine-testing.md`. Their files are unchanged from `v2`. Related release/headless architecture verification markers reference the merge commit. The release report SHA-256 is `7875390a6c9958aebcafb2a5ec4f383fc1ed42653a926e27042a73540ba23b3d`; `shasum -a 256 -c SHA256SUMS` passed.

## Concurrent base update

The target branch merged repository configuration in `cc94dc14b`. Merge that update too. Preserve its text/binary attributes and the explicit LF release-TOML rule. Repeated the full release E2E: all 45 checks passed, including Windows-style checkout. Final report: `.cache/pr2231-merge-release-final/report.json`, SHA-256 `477945a91fcc698988af2c2fed5d93e208abe417c174d5608091eb2c301037ab`. Runtime sources are unchanged since the accepted production headless run.
