# PR 2231 CI repair

## Failure evidence

PR checks run 42948760888 failed at head `d7eec40f66e116ea25928443f607327c76ddc1ed` in three jobs:

- Lint reported 73 issues: 47 unchecked error returns, 15 formatting issues, eight static analysis findings and three unused helpers.
- The production headless container could not build `unic`: Git rejected repository ownership while Go collected VCS status.
- The Linux search-token remove interaction failed because the button was not the input target.

## Resolution

Use the CI-pinned golangci-lint 2.15.0. Apply formatting and equivalent static-analysis rewrites, remove unused helpers, and explicitly discard best-effort cleanup/display errors. Keep actual archive write, sync and final close error checks intact.

Trust only `$GITHUB_WORKSPACE` in the disposable headless CI container before building. Do not disable Go VCS stamping or trust arbitrary repositories.

Allow search condition chips and value text to shrink within the field. Keep the remove button at its fixed size and expose the full value through a tooltip. Strengthen the existing interaction E2E with a long ASCII tag and a 1000-pixel window so the failure is reproducible independently of CJK font availability.

## Reproduction and validation

The strengthened interaction failed before the component fix: `linux/remove` reported the button outside the window at `(1067.1758, 14)`. The same command passed after the fix and writes `search-badge-delete.json`:

```sh
UNIC_UI_TEST_ARTIFACTS=/tmp/pr2231-ui go test -race ./app/desktop/internal/desktopdemo -run '^TestSearchBadgeDeleteE2E$' -count=1
```

The pinned linter reproduced all 73 issues. After repair, `golangci-lint run` reported zero issues. No lint rule was disabled.

The following commands reproduce the ownership failure and then build successfully with trust restricted to the checkout:

```sh
GIT_TEST_ASSUME_DIFFERENT_OWNER=1 GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null go build -o /tmp/pr2231-unic ./app/service/cmd/unic
GIT_TEST_ASSUME_DIFFERENT_OWNER=1 GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=safe.directory GIT_CONFIG_VALUE_0="$PWD" go build -o /tmp/pr2231-unic ./app/service/cmd/unic
```

The first command must fail with `error obtaining VCS status`; the second must succeed. The private temporary executable can be deleted after verification.

Production headless E2E passed all 13 process checks with race detection. Its manifest is `.cache/0015-acceptance/manifest.json`. Full race-suite and remote CI validation are pending at this documentation commit. No native GUI or browser was launched.

## Concurrent handshake and stop failure modes

Run 42965739173 removed the original failures but exposed a stop/handshake deadlock in Engine. The teardown holds the peer-state publication lock while the backend waits for its handshake goroutines. A handshake that revives a revoked device synchronously requests the same state lock. Repeating the original pairing test 30 times did not reproduce this scheduling window locally.

The deterministic isolated integration regression must cover: a callback waiting on an in-flight state push; a state push waiting on its own callback during shutdown; lost/coalesced refresh requests; and background refresh surviving Engine stop/store closure. Preserve synchronous revocation barriers and pending-device approval rules. This isolated callback/state-lock boundary is necessary to control the scheduling order; ordinary process tests retain end-to-end closure coverage.

The controlled regression failed in 1.13 seconds before the fix. Afterward it and the original pairing test passed 30 repetitions with race detection (21.857 seconds). Engine's complete race suite passed (55.391 seconds); production headless acceptance passed again (53.47 seconds), with all 13 process checks. The pinned linter still reports zero issues. Callback refreshes now coalesce into one Engine-owned worker; Stop prevents new work and waits for that worker. Explicit revocations retain their synchronous barrier. No test timeout was increased and no test was skipped to obtain these results.

The initial full local run passed the entire desktop interaction suite. Its three failing packages were `engine`, `app/service/internal/host` and `internal/v1migration`, caused by macOS temporary-directory aliases; all passed with `TMPDIR=/private/tmp`. Remote run 42965739173 also passed the desktop suite, lint and production headless acceptance before exposing the callback deadlock. Five-platform candidate build 42965739183 passed. Final remote CI is pending at this commit.
