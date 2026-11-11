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
