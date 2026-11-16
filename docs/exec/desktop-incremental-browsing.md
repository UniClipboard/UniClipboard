# Incremental Desktop history browsing

Issue: https://github.com/UniClipboard/UniClipboard/issues/2253

## Scope and failure inventory

The implementation uses existing Engine cursor pagination and collection revisions. It changes the Desktop host only. The [history search contract](../contracts/history-search-contract.md#incremental-desktop-browsing) defines request, cache, selection and recovery behavior.

Failure cases identified before implementation: idle reloads, unrelated events, lost SSE notifications, disconnect/reconnect, restart-invalidated cursors, query cancellation, concurrent page triggers, inconsistent multi-page revisions, duplicated or missing records, unbounded row/cursor/detail caches, selected deletion, evicted selected detail, and moved scroll anchors.

## Reproduction

The initial real-service E2E failed on the original production source after 11.5 seconds of idle polling: `unchanged idle history reloaded on the five-second schedule`. The source was `bd89ff5a12885079e4259c3a72c90ab5e8b420ca`.

## Repeatable acceptance

```sh
make desktop-e2e CASE=incremental-browsing
```

The canonical runner freezes source, runs the selected scenario with the race detector, binds retained evidence to source hashes and executable hashes, and deletes its private service profile and binaries after the owned process exits. The scenario uses 621 synthetic initial records, real service requests, production Desktop state and software-rendered scroll input. The scenario receipt contains assertions and a timestamped request timeline.

Native-window, physical input, system clipboard, OS focus and cross-application paste acceptance remain NOT_RUN. No native application or browser launch is required by this scenario.

## Results

All three canonical cases passed with the race detector. Every receipt's implementation hashes were compared with the final working-tree source.

| Case | Result | Retained report |
|---|---|---|
| incremental-browsing | PASS | `.build/desktop-e2e/20261116T040719Z-4499a5c5/index.html` |
| search-contract | PASS | `.build/desktop-e2e/20261116T040719Z-d6bff129/index.html` |
| data-binding | PASS | `.build/desktop-e2e/20261116T040719Z-fa318a48/index.html` |

The incremental case traversed all 621 initial records, retained at most 500 projected rows plus one selected detail, exercised automatic scroll paging and coalesced repeated page requests, and retained the keyed visible anchor after 60 new submissions. It verified no search reapplication during 11.5 idle seconds, selected deletion, query replacement, a deliberately dropped real SSE notification, and a real service restart. The missed-notification test advances the recovery deadline explicitly; the request timeline records the resulting metadata probe and reconciliation. Software-rendered checkpoints are retained in the report.

Each report records PASS for private-profile/source cleanup and unchanged frozen source. The incremental scenario receipt includes the service executable SHA-256 and per-file source SHA-256 values. No private service profile, source snapshot, executable or raw log remains in the report.

The Desktop program built successfully. Targeted race checks passed in Desktop, UI Kit, local API and Desktop E2E packages. The pinned golangci-lint v2.15.0 reported zero issues; format and whitespace checks passed.

`go run ./tools/docscheck -bump docs/architecture/desktop-runtime.md` updated the verified marker to implementation commit `262358694ac8eae96dd365c54e8183edf35b4b38` after the source was committed. The final documentation check passed; seven unrelated stale-page warnings remain.

Revisiting a distant discarded cursor checkpoint can require sequential cursor reads. Each search attempt has the existing ten-second budget. A timeout retains the current window and backs off before recovery. Native-window acceptance remains NOT_RUN.
