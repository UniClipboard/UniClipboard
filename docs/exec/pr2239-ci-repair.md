# PR #2239 CI repair

## Failed source and evidence

The failed run is [43192571011](https://github.com/UniClipboard/UniClipboard/actions/runs/43192571011). Its checkout is PR merge `6a42bb35735641928c3c8524d2a1309ab6572a9f`, merging head `f5ffae37a8f6aeb6337e2c5e7f6b1de7d37e8ede` into v2 `1d542f4ca7412972ae4c2cbbac46c211ae19395c`. Both complete job logs and the Desktop failure artifact were inspected before edits.

The race job failed only `e2e/cli.TestCatalog`. The script already contained seven negative-assertion lines absent from generated `CASES.md`. Regenerating the catalogue restores the existing freshness invariant.

The Desktop suite passed 27 cases and failed only `tag-manager-platforms`. Linux and Windows use taller table rows than macOS. The new built-in order places Links last, outside the 360-pixel initial viewport on those skins. `RightClick("#链接")` then resolves the duplicate sidebar label behind the sheet and correctly rejects it as an input target. The existing E2E now scrolls the table through normal wheel input before the original right-click and exact protected-menu assertion. No product behavior, assertion, case, timeout or workflow was removed or relaxed.

## Reproduction and validation

Both original failures reproduced locally before edits:

```sh
go test -race ./e2e/cli -run '^TestCatalog$' -count=1
make desktop-e2e CASE=tag-manager-platforms
```

The red Desktop report is `.build/desktop-e2e/20261112T101012Z-6a062348/index.html`. The corrected report is `.build/desktop-e2e/20261112T101233Z-867cc447/index.html`: all six skin/theme subcases pass, with seven report assertions, six rendered checkpoints and successful cleanup. Its manifest and SHA256SUMS bind the source and evidence. This is software-view acceptance; no native window was opened locally.

```sh
go test ./e2e/cli -run '^TestCatalog$' -update
TMPDIR=/private/tmp go test -race ./...
make desktop-e2e CASE=tag-manager-platforms
build_dir=$(mktemp -d)
go build -o "$build_dir/unic" ./app/service/cmd/unic
go build -o "$build_dir/unicd" ./app/service/cmd/unicd
go build -o "$build_dir/unic-desktop" ./app/desktop/cmd/unic-desktop
```

Pinned golangci-lint v2.15.0 formatting and change-scoped lint pass with zero issues. The three production builds pass. The ordinary documentation checker passes; test/catalogue-only staleness warnings do not change product documentation semantics. The complete local race suite passes: 782 tests across 68 packages. New-head CI is triggered by the push; its final status is reported separately.
