# Desktop search implementation

Baseline: 65d47985af5efbd33110f4057abf7359cd8f4399.
Scope and limits: [search contract](../contracts/history-search-contract.md).

## Execution order
1. Write production-process E2E scenarios before implementation.
2. Add transactional derived documents, substring search, signed paging and previews.
3. Expose paged candidates and exact batch counts through the same predicate.
4. Connect the main editor, then the quick panel, without changing their established matching policies.
5. Run frozen-source nonvisual acceptance, builds, scoped lint and documentation checks. Keep one small reproducible receipt; delete private profiles and binaries.

## Process acceptance scenarios
- Older-page and >1024-byte/inline-threshold text match; exact returned payload hash.
- One/two/three Chinese characters, non-ASCII case, punctuation, FTS words, quotes and emoji highlights.
- Phrase versus all terms, all versus any tags, file bundles and no cross-basename phrases.
- HTML-only visible text; scripts/style/attributes and file bodies remain absent.
- Catalogue values from persisted tags/sources/extensions; batch counts equal prospective searches.
- Cursor query mismatch/tamper/restart, chronology, deletion, expiry, offline import and reopen/repair.
- Search cancellation and slow read admission do not block submission/restore/deletion.
- Main and quick editor projections use global server search and discard obsolete generations.
- Record latency, database/WAL size, source and binary hashes; no native UI, offscreen renderer or system clipboard.

## Implemented result

Engine owns transactional search documents and the SQLite FTS5 trigram accelerator. Literal verification defines matching, including short Chinese queries. Normal capture, offline import, Actions, deletion, expiry and derived repair use the same projection. The service exposes the three explicit reads in the contract.

The main editor uses all text terms and all selected tags. The quick panel uses one phrase and any selected tag. Both query complete history, consume real catalogue values and exact prospective counts, and cancel obsolete reads independently from mutations. Previews carry bounded rune highlights. Existing demo interactions and fixture entry points remain available.

HTML fallback uses the existing HTML tree parser. Process regression covers an omitted head closing tag and adjacent inline elements. Hidden head/script/style/template text stays excluded. HTML-only input above 4 MiB retains history with an unavailable search projection; plain text retains the 64 MiB limit.

## Acceptance receipt

Working-tree source was verified against HEAD `7f1dbec010bb9a326cc4127c2e891bc567dc87a9`. Two unrelated documentation-only commits advanced the baseline during execution; their changes were preserved. The verified implementation was committed as `e697ae2bb`. The process receipts identify the exact source used for acceptance.

- Search production-process E2E: 14 scenario groups passed under the race detector. Receipt: `.build/history-search/receipt.json` (292 source hashes).
- Existing desktop production-process E2E: 16 checks passed. Receipt: `.build/history-search/desktop-regression-receipt.json` (286 source hashes).
- Production headless process E2E: 13 checks passed in 47.02 seconds. Receipt: `.build/history-search/headless-receipt.json` (287 source hashes).
- Every recorded source hash in those three receipts matches the final working-tree source.
- Core/store/local API/server/host: 242 existing checks passed across five packages after the final HTML change. The earlier nonvisual sweep also passed all 56 selected packages. No renderer test was executed.
- Pinned golangci-lint v2.15.0 reports zero new issues against the initial baseline.
- Production Desktop and CLI builds passed. Binary hashes are recorded in `.build/history-search/build-receipt.json`. Temporary binaries and private process profiles were removed.
- `git diff --check` passed. After the implementation commit, all six affected architecture pages were reviewed and their verified markers were updated to `e697ae2bb`. `go run ./tools/docscheck -strict` passed without warnings.

Synthetic-corpus sample: 291 ms for a concurrent 32-query count batch, 13 ms for an ordinary submission, and 5701400 bytes for database plus WAL. These values are not a production-scale performance guarantee.

## Reproduction

Run from the repository root on macOS. Use a canonical temporary directory for existing Engine fixture path checks.

```sh
TMPDIR=/private/tmp go test -race ./internal/store ./engine ./internal/localapi ./app/service/internal/server ./app/service/internal/host
UNIC_SEARCH_E2E=1 UNIC_SEARCH_RECEIPT="$PWD/.build/history-search/receipt.json" go test -race ./app/desktop/internal/desktopdemo -run '^TestHistorySearchProcessE2E$' -count=1 -v
UNIC_DESKTOP_DATA_E2E=1 UNIC_DESKTOP_DATA_RECEIPT="$PWD/.build/history-search/desktop-regression-receipt.json" go test -race ./app/desktop/internal/desktopdemo -run '^TestDesktopLiveDataE2E$' -count=1 -v
TMPDIR=/private/tmp UNIC_HEADLESS_E2E=1 go test -race ./e2e/headless -run '^TestProductionHeadless$' -count=1 -v
GOTOOLCHAIN=go1.27.3 go run github.com/golangci/golangci-lint/v2/cmd/golangci-lint@v2.15.0 run --new-from-rev=65d47985af5efbd33110f4057abf7359cd8f4399 ./engine/... ./internal/store/... ./internal/localapi/... ./app/service/... ./app/desktop/...
go run ./tools/docscheck
```

## Remaining acceptance and limits

Native windows, keyboard/focus behavior, visual rendering, system clipboard and OS paste were not exercised. No browser or offscreen renderer was launched. Production quick-panel Copy/Delete use the real service; automatic OS paste remains unavailable. Large-scale latency and Windows/Linux runtime acceptance remain unverified.

Code/link/directory classifiers, persistent favorites, general pinyin and global-hotkey preferences remain outside this search implementation. The implementation commit is recorded in Git history. No push or publication was performed.

Subsequent work implements global-shortcut preferences, target capture and native paste dispatch. Its current acceptance and platform limits are recorded in the [quick-panel system receipt](desktop-quick-panel-system.md); the search acceptance above remains historical.
