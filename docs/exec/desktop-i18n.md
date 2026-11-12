# Desktop i18n execution receipt

## Implemented behavior

- Five embedded catalogues: Simplified Chinese, English, Japanese, Brazilian Portuguese and Russian.
- General settings language selection, persistent host-only `desktop.language`, and updates without restarting.
- Shared history, settings, device sheets, quick-panel controls/actions, tray menus and device notifications.
- Application-owned strings only; content, filenames, device names, user tags and internal search identities stay unchanged.
- No new dependency, translation network request, protocol change or compatibility layer.

## Acceptance

The failure inventory precedes the implementation in `acceptance_i18n_test.go`. The canonical view case selects languages through the real control, navigates to history, filters files and renders the quick-panel action list. It covers five languages, three skins and both themes. It also checks catalogue coverage and formatting arguments. Evidence includes software-view PNGs, assertion results, source identity and SHA256SUMS.

The canonical `data-binding` process case saves each language through an isolated production `unicd`, verifies configuration and disk read-back, opens a fresh desktop binding, and rejects an unsupported language without replacing valid state.

Native windows, real tray menus, notification delivery and OS-level focus/paste were NOT_RUN. Software-view skins are not native-platform acceptance. The i18n catalogue has not received native-speaker review.

Final report paths and verdicts are recorded after the final checks below.

## Final checks (2026-11-12)

Base source: `3f9a8c2dc986572ece590dafb4b17c03d28c8046`, with this task's uncommitted changes. Each report binds its own frozen source and binary hashes.

| Check | Result | Report under `.build/desktop-e2e/` |
|---|---|---|
| `make desktop-e2e CASE=i18n` | PASS; 30 combinations, 90 software-view PNGs | `20261112T052438Z-3c089901/index.html` |
| `make desktop-e2e CASE=data-binding` | PASS; five persisted languages, disk read-back, fresh desktop binding, invalid-language rejection | `20261112T052347Z-b13cf095/index.html` |
| `make desktop-e2e CASE=native-menus` | PASS; software-view native-menu requests | `20261112T052213Z-516c9015/index.html` |
| `make desktop-e2e CASE=quick-panel` | PASS | `20261112T052213Z-632107cc/index.html` |
| `make desktop-e2e CASE=search-badge-delete` | PASS | `20261112T052213Z-a4539db3/index.html` |
| Desktop build | PASS | Built to an isolated temporary output; not launched |
| `go vet ./app/desktop/... ./internal/localconfig` | PASS | Static validation |
| Catalogue freshness and `git diff --check` | PASS | No stale catalogue or whitespace errors |
| `go run ./tools/docscheck` | PASS with 8 architecture freshness warnings | Existing markers retained |

The isolated process case required permission to bind its temporary Unix socket. Sync and telemetry were disabled. Test cleanup passed. No real profile or clipboard was used.

The initial `-bump` commands for overview and headless deferred verification until the source commit. PR preparation completed that refresh as recorded below.

## Settings screenshot supplement (2026-11-12)

`make desktop-e2e CASE=i18n` passed again after adding an explicit settings-page checkpoint before navigation. The report is `.build/desktop-e2e/20261112T054846Z-a3671c44/index.html`. It retains 120 software-view PNGs across the same 30 combinations, including 30 settings-page checkpoints. These use the shared demo composition and synthetic data; they are not native-window captures.

## Switch caption cleanup (2026-11-12)

Scanned all nine production Kit.Switch call sites and the single SwitchBase implementation. SwitchProps now retains only an accessible Label and Disabled flag. The shared renderer never draws a caption. Removed obsolete LabelHidden arguments from all callers; checkbox behavior is unchanged.

Extended the existing settings E2E checkpoint before implementation to reject duplicate text and verify accessible switch names, compact track width, mouse toggling and Space toggling for all four General switches across all 30 language/skin/theme combinations. The pre-change run reproduced the duplicate captions. The final run passed and retained 120 PNGs: `.build/desktop-e2e/20261112T064837Z-9d418da4/index.html`. Desktop build, desktop go vet, catalogue freshness and whitespace checks passed. Docscheck passed with the existing eight freshness warnings; verified markers still require a source commit before bumping. No native window was launched.

## PR preparation

Source commit: `b801141902cf765667c16cf36d6dc27d859acd6b`. All 481 Go/catalogue source files matched the final passing E2E snapshot before committing. The overview and headless architecture pages were rechecked and their `verified` markers now reference this source commit. The repository has no CONTEXT.md, so the required read-only context audit required no update. Catalogue freshness, gofmt and staged-snapshot checks passed. Local golangci-lint lacks the configured fmt flags; its CI job remains the formatter/linter gate.

## CI repair for PR 2236

The failed run was `43172407856` at PR head `a4f3c13e61eb4f1cb2c6b24caf6caa4293c11739`. Both complete job logs and the retained `desktop-e2e-evidence` artifact were inspected before edits. Lint reported three goimports grouping errors. The Desktop run passed i18n but failed sheet-motion and history-search-copy.

The resize scenario looked up the management dialog title after the live sheet switched to task content, where the visible management title is intentionally absent. It now follows the single live dialog by role throughout resize and dismissal. The native script clicked a macOS-only search disclosure that does not exist in the Linux accessibility tree. It now uses the platform find accelerator before locating the search field. Quit is registered at window scope so an unfocused view can shut down cleanly. Cleanup errors retain the original assertion failure.

The pinned golangci-lint v2.15.0 formatter reproduced all three import-group differences before the fix; the pinned linter passes after the fix. The canonical sheet-motion scenario reproduced the same missing-live-sheet error locally before the fix. No test expectation for resize geometry, animation, cleanup or copied bytes was removed. Native Linux behavior remains subject to the new CI run; local software evidence is recorded separately.

Local CI-repair checks: pinned v2.15.0 `golangci-lint run` and `fmt --diff --diff-colored=false` PASS; desktop build PASS; catalogue and whitespace checks PASS. `make desktop-e2e CASE=sheet-motion` PASS with complete resize/entry/dismissal evidence in `.build/desktop-e2e/20261112T074909Z-a4062409/index.html`. The 30-combination i18n case PASS in `.build/desktop-e2e/20261112T075034Z-73758254/index.html`. No local native window was launched.
