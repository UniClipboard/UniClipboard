# History search contract

Status: implemented in this working tree. Acceptance status is recorded in the [execution receipt](../exec/desktop-search-implementation.md).

## Ownership and reads

Engine owns the derived search projection in the history SQLite database. Hosts never download all payloads or own a second index.
Three explicit reads are exposed: SearchHistory, ListHistorySearchCandidates and CountHistorySearches. Their HTTP routes are POST /v1/history/search, POST /v1/history/search/candidates and POST /v1/history/search/counts.
Ordinary history listing, events, logs, errors and telemetry remain content-free. Only explicit search hits contain bounded previews. Request bodies and cursors never appear in diagnostics.

## Query

TextTerms are literal Unicode-lowercase substrings. All terms must match within the same item; each individual term stays within one document. Terms can match different file basenames of that item. A phrase is one term, including spaces. Percent, underscore, quotes, punctuation and FTS operators are literal.
Types are text, richtext, image or files, combined with OR. Richtext is text history with HTML. Dimensions combine with AND. Tags use all (default) or any. Sources use OR over local, device (with DeviceID), external and migration. Local means local_system/explicit/action. Extensions use lowercase suffixes without dots, with OR; PNG representations expose png. Plain clipboard text has no synthetic txt filename.
Time is an absolute [from,before) millisecond range. The desktop resolves presets in local calendar time and freezes them for one query generation. Monday starts a week.
Normalize and deduplicate query sets before signing cursors. Ordering remains (order_ms,seq) descending. Cursors fix a first-page sequence upper bound and bind the Engine instance and complete normalized query. They contain no raw query. Wrong query returns conflict; tampering returns invalid_argument; another instance returns resource_expired.
Results remain bounded live traversal. Counts belong to their own evaluation time and read transaction; they are not a frozen total for multi-page results.

## Material

Index committed plain text, otherwise HTML text after excluding head/script/style/template content and markup. Collapse whitespace in HTML fallback. HTML-only extraction uses the existing HTML tree parser. It preserves adjacent inline text and separates block text. HTML-only input is limited to 4 MiB before parsing. Larger input preserves history and reports an incomplete index. The plain-text limit remains 64 MiB. Index each file basename independently without reading the file. Never index file bodies, host references, URLs fetched over the network, or OCR.
Keep original preview text and a Unicode-lowercase matching projection. Use trigram only as an accelerator for eligible terms; canonical substring verification defines correctness. One- and two-rune terms use exact substring matching. Do not normalize accents or canonical Unicode forms.
Write history, search state and documents in one transaction. FTS insert/delete triggers keep the index synchronized, including cascades. Every query checks expiry at its own evaluation time. Repair missing derived projections from persisted content on open. Repair never creates history or touches user files.
Projection failure or budget exhaustion preserves the history record and records an unavailable search state. Text/extension search and extension-catalogue requests over an incomplete corpus return unavailable, never a partial or false-empty answer. Metadata-only browsing remains available.

## Limits and isolation

- Limit defaults to 50; valid result/catalogue limits are 1..200.
- At most 16 terms, 4096 UTF-8 bytes per term and 8192 bytes over all terms. NUL is rejected in queries.
- At most 64 values in each filter dimension. Tag values use existing history constraints.
- At most 32 count queries per batch; all-or-error, with one read transaction and expiry evaluation.
- Search preview: at most 512 runes. Highlight ranges are half-open rune offsets in that exact preview.
- HTTP search bodies: at most 1 MiB. No body/raw lower-level error is returned on invalid input.
- Search and catalogue timeout: 2 seconds. Batch counts: 4 seconds. A single bounded read-only connection serves search in the same WAL database; it never holds the writer connection while waiting for search work.
- Derived projection admission budget: min(history byte budget, 2 GiB). Each document reserves 8 times its original+folded UTF-8 bytes plus 256 bytes for index/storage overhead. This is a conservative logical reservation, not a physical disk-size guarantee. Measure database/WAL bytes separately in acceptance. Search does not change payload retention to make room for the derived index.
- One text representation is at most the existing 64 MiB content cap. Repair and extraction read no file-body data. Invalid/unreadable content is marked unavailable without logging its identity or bytes.

## Catalogue and counts

Catalogue pages contain actual persisted tag names, historical source IDs with current/tombstoned device display names, and observed extensions. Unknown source names fall back to the source ID. Prefix filtering is Unicode-lowercase substring matching. Catalogue cursors bind field/prefix/instance and stable canonical keys.
CountHistorySearches accepts complete prospective queries and returns exact distinct-item counts in request order. The UI owns toggles, replacement and independent relaxation proposals. It counts only bounded candidate pages. Selected candidate removal must not be confused with addition.

## Desktop

The main editor sends parsed literal words and TagMatch=all. The quick panel sends its literal phrase and TagMatch=any. Both preserve current keyboard/caret/chip interactions. Punctuation-only quick-panel text now remains literal.
Each editor owns a generation and cancels stale requests. Hits, counts and selected-content reads have separate cancellation. Stale read work never queues ahead of restore/delete writes. Count labels stay absent while unknown.
Unsupported code/link classifications, persistent favorites and arbitrary pinyin remain unavailable. They do not become similarly named user tags. Existing fixed type/time aliases remain UI-owned.

The process fixture additionally requires a small ordinary submission to finish within one second while a 32-query count batch runs over its synthetic corpus. Record actual count/write latency and database/WAL bytes; this is not a production-scale latency guarantee.

## Failure cases specified before code

Long text/older-page false misses; short Chinese misses; phrase/term confusion; cross-filename phrases; Unicode/emoji offsets; wildcard/operator interpretation; tag intersections/unions; stale cursor/results/counts; deleted/expired/imported content; incomplete index/repair; body/path/script leakage; slow counts blocking writes; lost event refresh; cancellation and admission limits; index quota exhaustion; file-body access during filename search.

Directory snapshots and source references use Engine history kinds `directory` and `directory_reference`. Both participate in ordinary history queries. Their content semantics follow the [directory contract](directory-contract.md).
