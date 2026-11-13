# History detail prefetch

## Scope

Prefetch the complete visible history/search result range plus ten entries above and below it. Reuse the existing UI Kit viewport for the main list and grid. Use visible row bounds or the displayed image cells for the quick panel. Retain only the window and the selected detail. See the [architecture](../architecture/overview.md#history-detail-prefetch).

## Failure inventory

Written before implementation: incomplete visible coverage; incorrect grouped/grid indices; cached clicks rereading content; retained payload references after eviction; stale query/revision completions; duplicate overlapping reads; unbounded concurrency; reconciliation flicker; repeated error retries; loss of the active detail when scrolling.

## Acceptance

The existing `search-contract` process scenario uses an isolated real `unicd`, SQLite, production local API and Desktop binding. It injects inclusive viewport indices without opening a native window. Its receipt includes source hashes, service binary hash and named assertions. The canonical runner also records the frozen source manifest, checksum file and cleanup result.

Reproduce with:

```sh
make desktop-e2e CASE=search-contract
make desktop-e2e CASE=data-binding
```

The prefetch scenario covers 20 visible entries plus ten on each side, ready selection without rereading, concurrency bounds, bottom eviction, reverse scrolling, active-detail retention, unchanged reconciliation, query replacement and quick-panel cached previews. The existing data-binding scenario covers image, file and directory previews and real service reconnect behavior.

Native scrolling, actual pointer-to-detail latency, list/grid layout and platform rendering remain NOT_RUN. The process scenario does not claim native visual acceptance.

## Results (2026-11-13)

| Case | Result | Retained report |
| --- | --- | --- |
| search-contract | PASS with race detector | `.build/desktop-e2e/20261113T075028Z-98067669/index.html` |
| data-binding | PASS with race detector | `.build/desktop-e2e/20261113T075034Z-bf349913/index.html` |

Both runs report an unchanged frozen source snapshot and successful private cleanup. The search run's recorded hashes match the final production renderer, scheduler and acceptance source files. Each report has its own `manifest.json`, `SHA256SUMS`, case result and process receipt. No source, profiles or executable binaries remain in the retained evidence.

`go run ./tools/docscheck` passed with eight architecture freshness warnings. The required `-bump docs/architecture/overview.md` attempt rejected uncommitted source. The marker must be refreshed after committing the verified implementation. The first sandboxed E2E attempt emitted no case result; the normal-access rerun and final run passed. Native acceptance remains NOT_RUN.
