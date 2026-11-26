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

Index committed plain text, otherwise HTML text after excluding head/script/style/template content and markup. Collapse whitespace in HTML fallback. HTML-only extraction uses the existing HTML tree parser. It preserves adjacent inline text and separates block text. HTML-only input is limited to 4 MiB before parsing. Larger input preserves history and reports an incomplete index. The plain-text limit remains 64 MiB. Index each file basename independently without reading the file. Base synchronous indexing never reads file bodies, host references, network URLs, or OCR. Optional asynchronous `enricher` plugins may append source-isolated derived documents after commit as specified below.
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

Catalogue pages contain persisted tag names other than the reserved `favorite` tag, historical source IDs with current/tombstoned device display names, and observed extensions. Unknown source names fall back to the source ID. Prefix filtering is Unicode-lowercase substring matching. Catalogue cursors bind field/prefix/instance and stable canonical keys.
CountHistorySearches accepts complete prospective queries and returns exact distinct-item counts in request order. The UI owns toggles, replacement and independent relaxation proposals. It counts only bounded candidate pages. Selected candidate removal must not be confused with addition.

## Desktop

The main editor sends parsed literal words and TagMatch=all. The quick panel sends its literal phrase (excluding accepted filter tokens) and TagMatch=all so favorites and multi-tag filters intersect consistently with the main Desktop. Both preserve caret/chip interactions and trigger suggestions on explicit filter prefixes (`#`, `/`, `@`, `on:`, `ext:`). Punctuation-only quick-panel text remains literal.
Each editor owns a generation and cancels stale requests. Hits, counts and selected-content reads have separate cancellation. Stale read work never queues ahead of restore/delete writes. Count labels stay absent while unknown.
The main Desktop Favorites scope, Quick Panel Favorites filter toggle, and an explicit Favorites filter condition (selected via `#` suggestions, the GUI filter popover, or an unquoted `#favorite` search term) all add the reserved `favorite` tag to the existing AND filters. The Favorites sidebar count covers all favorites in the local history. Ordinary Desktop tag displays, tag management, and the Engine tag catalogue API (`ListHistorySearchCandidates`) continue to omit the reserved tag. Unsupported code/link classifications and full-body pinyin search remain unavailable; Desktop UI candidate autocomplete for filter dimensions (types, tags, devices, time presets, extensions, and favorites) supports localized labels, cross-language aliases, and Chinese pinyin (full pinyin and initials), including user-created Chinese tags and device names.

The process fixture additionally requires a small ordinary submission to finish within one second while a 32-query count batch runs over its synthetic corpus. Record actual count/write latency and database/WAL bytes; this is not a production-scale latency guarantee.

## Failure cases specified before code

Long text/older-page false misses; short Chinese misses; phrase/term confusion; cross-filename phrases; Unicode/emoji offsets; wildcard/operator interpretation; tag intersections/unions; stale cursor/results/counts; deleted/expired/imported content; incomplete index/repair; body/path/script leakage; slow counts blocking writes; lost event refresh; cancellation and admission limits; index quota exhaustion; file-body access during filename search.

Directory snapshots and source references use Engine history kinds `directory` and `directory_reference`. Both participate in ordinary history queries. Their content semantics follow the [directory contract](directory-contract.md).

## Incremental desktop browsing

The production Desktop list and grid request the next 50-entry cursor page when the visible range is within ten entries of its loaded end. There is no manual page button. Near the loaded start, an evicted newer page is restored automatically.

One traversal retains at most 500 projected entries and one selected entry outside that window. Payload prefetch retains the existing visible-range and four-reader bounds. Neighbor image previews use at most 256-pixel thumbnails, with a 32 MiB conservative CPU/encoded thumbnail reservation that shrinks the edge for larger windows. Only the selected image upgrades to original resolution, under the independent 32 MiB encoded / 16-million-pixel preview bounds. Cached selection can display the thumbnail during that upgrade. Original dimensions, EXIF orientation, stored bytes and copy/apply semantics remain unchanged. Image response opening, service-side content materialization, buffering and decoding
use one cancellation-aware slot per Desktop process, shared by both views. Acquire
it before `HistoryContent`; release it after response close on every exit. Selected
original requests precede queued neighbors. Keep canceled jobs in the four-job view
bound until they finish; cancel a pending neighbor if selection needs a job. Text
and directory metadata remain independent. Single-file candidates conservatively
share the image slot because response headers supply their MIME/name. Neighbor
input uses one 64 KiB buffer over the shared byte limiter and streams through
the existing decoder, replaying only inspected headers under
the 32 MiB encoded limit. Validate dimensions before full decode and consume the
bounded remainder to detect trailing overflow. Check cancellation on reads and
codec phase boundaries. A codec/resize call already running is bounded by the pixel
limit rather than preemptible. Shown bitmaps release CPU pixels on a later view frame and retain reconstruction bytes through mygo; this lifecycle schedules no heartbeat. At most 128 content-free cursor checkpoints are retained; older checkpoints are thinned. Revisiting an evicted checkpoint follows Engine cursors sequentially from the closest retained checkpoint. Search workers occupy one slot, including canceled generations until they exit. Repeated viewport triggers coalesce into that slot. A failed read retains the previous window and backs off for two seconds.

Relevant service notifications schedule a revision check. An idle recovery check runs at most every 30 seconds. It reads one metadata entry and the collection revision, together with auxiliary service state. An unchanged collection does not invoke SearchHistory, replace entries, invalidate detail caches or reset selection. A broken event stream invalidates cursors and schedules reconciliation. Collection revision changes, expired loaded entries and a new query also require reconciliation. A new query cancels the old generation; obsolete results cannot publish.

Reconciliation follows fresh Engine cursors to a surviving anchor in the visible range, then reads only a bounded window. It discards pages scanned before that anchor. If every anchor was deleted, it returns the final available page. Revision changes during traversal fail that attempt and schedule recovery rather than publishing inconsistent pages. Stable item keys preserve the visible scroll anchor when it survives. Search text, filters, selected entry and open detail state remain unchanged. A selected entry outside the window retains one detail; deletion clears that selection and closes its detail and preview. A new query clears the old selection and restarts browsing from the first page.

The repeatable `incremental-browsing` Desktop E2E case uses a real isolated service and software-rendered scrolling. It records source and service binary hashes, the request timeline, assertions and view checkpoints. Its receipt does not establish native-window behavior.

## Derived enrichment search documents

`history_search_docs` records a `source` (`base` or `enricher:<plugin_id>`) and `doc_kind` (`text`, `filename`, `link_title`, `link_desc`, `ocr`, etc.) for every indexed document.

- **Base readiness isolation**: `history_search_state.ready` tracks **only** the synchronous base projection written in the commit transaction. Pending, skipped, failed, or budget-rejected enrichments never mark `ready=0` and never cause `SearchHistory` or `CountHistorySearches` to return `unavailable` (`ErrSearchIncomplete`).
- **Repair safety**: Startup `RepairSearch` repairs only `source='base'` documents and preserves valid derived enrichment documents.
- **Bounds and sub-budget**: Each enrichment document is valid UTF-8 up to 64 KiB, with at most 256 KiB total per enrichment run. Derived enrichment search reservations are tracked separately from base reservations and capped at half of the search projection budget (`min(history byte budget, 2 GiB) / 2`). Exhausting the enrichment search sub-budget skips storing the derived search documents without failing history retention or corrupting the base index.
- **Hit attribution**: `HistorySearchPreview` includes optional `match_kind` identifying whether the returned bounded preview matched a `base` document or a derived document kind (such as `link_title` / `link_desc`). Delete cascades remove both base and derived documents and release their reservations atomically.
