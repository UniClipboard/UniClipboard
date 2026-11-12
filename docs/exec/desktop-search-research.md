# Desktop search interface research

Status: research completed and approved for implementation. Exact adopted semantics and limits are now defined in the [search contract](../contracts/history-search-contract.md); implementation and evidence are recorded in the [execution receipt](desktop-search-implementation.md).
Source: `65d47985af5efbd33110f4057abf7359cd8f4399` on `v2`.
Method: inspect the approved demo source, existing acceptance cases, current contracts and the installed SQLite dependency. No application, browser or renderer was started.

## Recommendation

Keep editor interpretation in the desktop. Put searchable material, matching, ordering, pagination and exact counts in Engine.
Use typed queries shared by the main window, quick panel and CLI.
Preserve the two demos' existing matching differences explicitly. Do not select behavior by a `surface=main|quick` parameter in Engine.
Add three read operations: search for content-bearing hits, browse filter candidates, and count a bounded batch of prospective queries.
The candidate catalogue needs its own operation because current peers do not cover all historical source devices, and the current extension list is fabricated.
Use the existing SQLite dependency. Do not add an external search engine or a second desktop-owned index.

## Observed interaction contract

| Concern | Main-window demo | Quick-panel demo | Interface implication |
| --- | --- | --- | --- |
| Conditions | `@`, `#`, `/`, `on:`, `ext:` | `@`, `#`, `/`, time aliases | Send canonical values after the editor accepts a condition. |
| Free text | Each parsed word must occur; quotes/escapes preserve literal text | The trimmed input is one contiguous substring | Send an array of literal terms. Require all terms. A phrase is one term containing spaces. |
| Tags | Multiple tags intersect | Multiple tags form a union | Include an explicit `TagMatch=all|any`. |
| Types | One selected type | Multiple types form a union | Represent types as a set with OR semantics. |
| Sources | One selected device | Multiple sources form a union | Use typed source selectors, combined with OR. |
| Dimensions | Conditions combine with AND | Conditions combine with AND | Fix this rule in the query contract. |
| Completion | Enter, Tab, pointer selection; completed exact words followed by whitespace may commit | Explicit acceptance only; typing alone does not commit | Keep acceptance policy in the editor. |
| Candidate context | Active word follows the caret; completion preserves surrounding text | Suggestions can consume a recognized token and retain other text | Exclude the active uncommitted condition from the prospective base query. |
| Candidate counts | Show the result of the prospective edit under all remaining conditions | Suggestion rows have no counts; empty-state relaxations do | Count complete prospective queries, not independent facet frequencies. |
| Badge removal | Remove the complete dimension; all selected tags share one badge | Remove the selected chip/value | The editor constructs the resulting query. |
| Escape | Dismiss candidates, clear conditions, collapse empty search | Dismiss candidates, clear conditions, hide panel | No Engine behavior or request flag is needed. |
| Timing | Editor state updates immediately | Search waits 300 ms; preview waits 500/120 ms | Debounce and preview scheduling belong to the UI. |
| Empty results | Clear/change conditions | Suggest removing one dimension at a time | Count independently relaxed queries; keep only positive alternatives. |
| Highlight | Not a complete match-span contract | Highlight matching text in a row title | Return typed highlight ranges in bounded previews. |

Primary evidence:

- [Main search model](../../app/desktop/internal/desktopui/search_model.go) and [token-search acceptance cases](../../app/desktop/internal/desktopui/acceptance_token_search_test.go).
- [Badge-removal acceptance](../../app/desktop/internal/desktopui/acceptance_search_badge_delete_test.go).
- [Quick-panel model](../../app/desktop/internal/desktopui/quick_panel_model.go), [view](../../app/desktop/internal/desktopui/quick_panel_view.go) and [acceptance cases](../../app/desktop/internal/desktopui/acceptance_quick_panel_test.go).

Important fixture limits:

- Main-window `this_week` currently behaves like `today`; some other presets do not constrain fixtures at all. Quick-panel dates are similarly fabricated. These implementations are not calendar specifications.
- Quick-panel pinyin is a fixed alias table. It does not implement arbitrary pinyin matching for device/tag names or clipboard content.
- Quick-panel punctuation-only input is ignored by its literal matcher. This differs from the main window. Recommend treating punctuation as literal content in production; mark this as a deliberate behavior change to review.
- Selected-tag candidate counting in the main fixture calls a toggle. A selected candidate's count therefore describes removal, not adding the tag again.

## Current implementation and gaps

The [history contract](../contracts/history-contract.md) gives Engine sole ownership of history and prohibits a second host-owned search index.
`HistoryItem` intentionally excludes content, file names and file paths. The current metadata-list contract excludes full-text search and total counts.
The [Engine query](../../engine/history_types.go), [service DTO](../../internal/localapi/api.go) and [SQLite query](../../internal/store/history.go) support types, origins, all-match tags, one remote device and an absolute time range.
They do not support text, extensions, multiple source alternatives, any-match tags, content-derived previews or exact query counts.

Existing pagination already provides useful semantics: signed opaque cursors, a first-page commit-sequence upper bound and stable `(order_ms, seq)` ordering. It is a live bounded traversal, not a retained database snapshot.
The live [desktop binding](../../app/desktop/internal/desktopui/live.go) currently disables unsupported search. Its query key includes only kind/source/tag/time. Search integration must replace that key with the entire normalized query, including text terms, tag mode and extensions.

The current preview client fetches parts individually and limits text previews to 1024 bytes. Searching those previews would miss long text, older pages and HTML-only records. Downloading every historical payload to the desktop would violate the ownership contract.

The store already has `clip_item_parts` with MIME, file basename, inline bytes or blob references, and `history_labels` with persisted tag names. It uses a single SQLite connection.

## Proposed query semantics

This is a design sketch, not a new public contract:

```go
type HistorySearchQuery struct {
    TextTerms       []string              // literal substrings; all required
    Types           []HistorySearchType   // text, richtext, image, files; any
    Tags            []string              // actual local tag values
    TagMatch        HistoryTagMatch       // all or any
    Sources         []HistorySearchSource // typed selectors; any
    Extensions      []string              // lowercase, no leading dot; any
    CreatedFromMs   int64                 // inclusive, zero means unbounded
    CreatedBeforeMs int64                 // exclusive, zero means unbounded
}
```

Rules:

1. Every populated dimension constrains the query. Values within type/source/extension dimensions combine with OR. Tags use the explicit mode, with a canonical default of `all`.
2. Empty terms or sets impose no restriction. Normalize order and duplicates before hashing a query. Reject invalid enum values and impossible time ranges.
3. A term must occur within one searchable document, such as the committed text or one file basename. Different required terms may match different documents of the same history item. A phrase must not span two file names or representation boundaries.
4. Preserve literal `%`, `_`, quotes, parentheses, `AND`, paths and unknown prefixes. Never expose SQL, FTS syntax, regular expressions or a generic Boolean expression language as the product query.
5. Recommend the demo's Unicode lowercase substring semantics. Do not silently introduce accent removal, stemming, fuzzy matching or Unicode normalization. Define composed/decomposed forms and supplementary characters in acceptance examples before coding. An FTS accelerator must not define correctness.
6. `text` includes plain-text and HTML-only text history. `richtext` is its HTML-bearing subset. It is a search classification, not a replacement for persisted `HistoryKind`. Match overlapping selections once per HistoryID.
7. Source selectors distinguish `local`, `device`, `external` and `migration`. Only `device` has a DeviceID. Local means the existing local-system/explicit/action origins. External and migration entries must not become fake local devices. Revoked or disconnected devices remain searchable when their history exists.
8. Resolve calendar presets on the calling device to absolute milliseconds. Freeze these bounds for one query generation and its cursor. Starting a new day or changing a preset starts a fresh query. Weeks begin on Monday, matching the reference intent.
9. File extensions come from persisted basenames. PNG image representations may match `png` through their MIME. Plain clipboard text has no inferred `txt` extension. Do not invent original filenames for converted images.
10. Use existing chronological order. Do not add relevance ranking or pin/favorite ordering as an accidental search change.

Concrete mappings:

```json
{
  "text_terms": ["HTTP", "服务示例"],
  "tags": ["设计", "工作"],
  "tag_match": "all",
  "sources": [{"kind": "device", "device_id": "opaque-device-id"}],
  "extensions": ["png"]
}
```

This expresses the main-window behavior. A quick-panel literal phrase sends `text_terms: ["HTTP 服务示例"]`. Its two accepted tags send the same tag values with `tag_match: "any"`.
The normalized query contains neither localized condition labels nor caret positions.

## Proposed reads

### 1. SearchHistory

```go
SearchHistory(ctx, HistorySearchRequest{Query, Cursor, Limit})
    -> HistorySearchPage{Hits, NextCursor, HistoryRevision, EvaluatedAtMs}
```

Each hit contains normal `HistoryItem` metadata plus a bounded `Preview{Text, Highlights}`.
The preview uses committed plain text, visible HTML fallback, or a matching file basename. It is not the full payload.
Highlights use half-open Unicode-scalar/rune positions within the exact returned preview string. Do not return HTML markers or byte offsets.
Return no image bytes or absolute paths. Existing explicit content readers still supply the selected detail/preview.
Define fixed preview and response-size bounds before implementation; proposed starting limits are 512 runes per hit and at most 200 hits, subject to the acceptance budget review.
A file bundle returns one hit even if several files match.

Use a dedicated `POST /v1/history/search` route because this is an explicit content-reading operation and the query text should not enter URL logs.
Keep normal `GET /v1/history` metadata-only. Share the internal predicate evaluator instead of maintaining two independent filter implementations. No old-version compatibility facade is required.

### 2. ListHistorySearchCandidates

```go
ListHistorySearchCandidates(ctx, HistorySearchCandidatesRequest{Field, Prefix, Cursor, Limit})
    -> HistorySearchCandidatesPage
```

`Field` is tag, source or extension. Return canonical values and source display metadata, not complete history or body fragments.
The catalogue covers persisted tag definitions, historical source identities and observed extensions. It does not depend on the currently loaded result page.
Include unused known tags and permit zero-match candidates. Browse candidates with a bounded page size.
Unknown source labels must remain identifiable by stable kind/ID. Historical names require an explicit storage policy; the current active-peer projection is insufficient.
No pinyin behavior is promised by this method. Arbitrary name transliteration needs a separate product decision and a maintained implementation.

The UI supplies localized fixed type/time choices and the known aliases. It owns prefix interpretation, quoting, active-word replacement and selection rules. Reuse the current editor logic; do not copy the whole parser into unicd.

Suggested route: `POST /v1/history/search/candidates`. This also keeps typed prefixes out of request URLs.

### 3. CountHistorySearches

```go
CountHistorySearches(ctx, HistorySearchCountsRequest{Queries})
    -> HistorySearchCounts{Counts, HistoryRevision, EvaluatedAtMs}
```

`Counts[i]` is the exact number of distinct visible HistoryIDs matching `Queries[i]`.
Evaluate the bounded batch in one read transaction and one expiry-time evaluation. A proposed limit is 32 complete queries; validate it against the agreed work budget.
Deduplicate identical normalized queries internally. Return one clear cancellation/error result if the batch cannot finish; do not label an incomplete count as zero or exact.

The UI constructs complete prospective queries from its existing edit rules:

- Main tag candidate: add an unselected tag or remove a selected tag, retaining `TagMatch=all`.
- Main device/type candidate: replace that dimension, preserving other dimensions and remaining literal terms.
- Quick-panel new tag/type/source: add an alternative using its current semantics; alias-driven replacement remains an editor decision.
- Empty-state recovery: remove exactly one dimension from the base query. Count every proposed relaxation independently.

This avoids a generic patch language, surface-specific backend flags and server-side caret parsing. The same predicate handles result retrieval and every count.
Catalogue matching does not require counting the entire catalogue on each keystroke. Count only the bounded candidate page or visible proposals.
Suggested route: `POST /v1/history/search/counts`.

Counts describe the read at `EvaluatedAtMs`, not a frozen multi-page total. Results, catalogue pages and counts are independently live reads. Do not pretend their revisions form one database snapshot.

## UI request lifecycle

Use one shared desktop query projection and two existing editor/interaction adapters. Engine accepts neither mygo state nor a main/quick-panel enum.
Each text/filter change increments a local query generation. Cancel obsolete work and discard responses for older generations, including counts and candidate pages.
Keep hit loading, counts and selected-content reads independently cancellable. A slow count must not wait ahead of a new restore/delete intent in the existing single desktop I/O queue.
Keep the active query's cursor bound to the normalized terms, filters, instance and first-page upper bound. Changing text or tag mode invalidates that cursor just as changing a filter does today.
Do not encode raw query text or filenames in a cursor. Store a normalized-query fingerprint instead.
Refresh from the first page after relevant history changes or when a fresh result view is requested. Event loss still requires reconciliation.
Counts may arrive after hits. Until then, omit the count; do not replace it with the current page length.

## Searchable material and index ownership

Persist a derived search projection inside the Engine-owned SQLite store. Produce it once from the immutable committed content.
Prefer plain text when it exists. For HTML-only content, extract visible text through the already available HTML parsing library; skip markup, scripts and styles. Never index raw attributes as visible clipboard text.
Index each file basename separately. Do not read file bodies, walk directories, perform OCR, fetch URLs or expose host references merely to search.
Keep tag values and source filters in relational metadata; mutable labels and device names must not be copied into immutable body search documents.

Local/explicit/Action commits, remote receipts, temporary transfers and offline v1 imports all converge on persisted history but use more than one insertion path. Every insertion path must write the projection and index before its history record becomes query-visible.
Every query must enforce `expires_ms` against its evaluation time, even before physical expiry cleanup runs. Explicit deletion, expiry cleanup, quota eviction and failed receive rollback must remove history and derived index entries atomically. Foreign-key cleanup alone does not maintain an FTS external-content index.
A rebuild is derived-index repair from current authoritative history, not a compatibility migration. It must not resurrect deleted records, modify user files or silently return a partially indexed corpus as complete.
Account for projection/index space and rebuild resources. The existing history quota counts owned payload bytes; it does not automatically account for a new FTS index. Choose the storage/resource budget before implementation and measure it in the process receipt.

## SQLite mechanism and verified limitations

The dependency is already `modernc.org/sqlite v1.61.1`. The synthetic probe reports SQLite `3.54.1` on `darwin/arm64` with FTS5 trigram available.
No dependency change is needed for this mechanism.

Use a normalized substring projection with an FTS5 trigram accelerator. Quote literal terms and bind them as parameters. Recheck candidates against the canonical substring predicate so tokenizer case behavior cannot change the public result.
For one- and two-rune terms, scan the relevant normalized projection with exact substring matching, narrowed by other indexed filters. This is a required query path, not a future placeholder. Never convert a trigram short-term miss into an empty result.
A scan may exceed its budget. Surface timeout/cancellation instead of returning a partial or false empty result.
Keep validation and complexity limits explicit: bound request bytes, term lengths/counts, selected values, candidate batches and response bytes. Their exact limits must be adopted before coding; the demo does not supply a resource contract.

The installed-driver probe demonstrated:

| Probe | Result |
| --- | --- |
| Quoted trigram MATCH for `产品` | No rows |
| LIKE `%产品%` | The expected Chinese row |
| Quoted trigram MATCH for `产品讨` | The expected Chinese row |
| MATCH for lowercase `café` against `CAFÉ` | Matches |
| Default LIKE for lowercase `café` against `CAFÉ` | Does not match |
| Quoted literal `100%`, `_tag`, `AND`, embedded quote | Correct literal rows |
| Phrase `foo bar` versus required terms `foo` and `bar` | Distinct result sets, as needed by the demos |

The receipt at `.build/desktop-search-research/sqlite-probe.json` includes schema version, dependency/toolchain/source identity, every SQL query, its bound arguments and result IDs. The probe used an in-memory database and fabricated records only. Its temporary Go source was deleted.
Reproduce the cases by creating `tri USING fts5(body, tokenize='trigram')`, inserting the synthetic rows below and executing the receipt's SQL with its recorded bindings:

```text
1: 周一产品讨论
2: 设计评审记录
3: CAFÉ 100% _tag AND (x) a"b
4: foo ... bar
5: foo bar
```

Primary documentation confirms that trigram MATCH has a three-character minimum, LIKE with ESCAPE cannot use trigram optimization, and external-content indexes need explicit synchronization. [SQLite FTS5](https://www.sqlite.org/fts5.html#the_trigram_tokenizer), [external-content tables](https://www.sqlite.org/fts5.html#external_content_tables).
Default LIKE folds ASCII case only. [SQLite expressions](https://www.sqlite.org/lang_expr.html#the_like_glob_regexp_match_and_extract_operators).

The current one-connection store makes slow searches/counts compete with writes. Include a bounded read-only SQLite search connection in the resource design while retaining the existing writer. Use context-aware SQL and bounded admission. Do not expand an unrestricted connection pool or create a second history database.
No realistic latency, index-size or writer-interference benchmark has run. The synthetic probe establishes semantics and feature availability only.

## Patterns from mature products

Raycast exposes clipboard search, separate type filtering and distinct paste/copy actions. Alfred documents matching words or phrases in the original clip. These support separating retrieval from editor gestures and clipboard application; they do not establish either product's storage implementation. [Raycast Clipboard History](https://manual.raycast.com/clipboard-history), [Alfred Clipboard History](https://www.alfredapp.com/help/features/clipboard/).

## Product decisions still required

1. The recommended query supports both existing tag and phrase semantics. Unifying them would be a separate UX change.
2. Recommend punctuation-only input remain literal. The quick-panel fixture currently ignores it.
3. General pinyin matching is not implemented by the demo. Decide whether fixed localized aliases are enough or arbitrary device/tag transliteration is required.
4. Demo virtual `#链接`, `#代码`, `#图片` and `#文件夹` are not normal stored tags. Preserve separate identities for virtual classifications and user labels. Define real classification rules before exposing them. Existing `builtin.text` emits mutable `url`/`email` hint tags over only the first 64 KiB; these are not equivalent to a reliable link/code classifier.
5. Favorites/pins have no persistent model. Search must not infer them from a similarly named user tag.
6. Define treatment of unreadable persisted content and oversized searchable projections. Recommend explicit failure/index-readiness state over silently incomplete search. Do not change history persistence or discard content as an incidental indexing policy.
7. Agree corpus sizes, deadlines, index-space limits and writer-interference thresholds before implementation. Large-record indexing cannot be solved by indexing only the current UI preview prefix.

## Failure-driven end-to-end acceptance plan

Write these scenarios before implementation. Use production unicd, private profiles, real local clients and fabricated content. Retain one small JSON receipt with source/binary hashes, counts, result IDs and resource measurements.

1. Find a match beyond the first history page and beyond the existing 1024-byte preview prefix. Verify returned content hashes through the explicit reader.
2. Search one, two and three Chinese characters; ASCII and non-ASCII case; quotes; punctuation; literal percent/underscore; unknown prefixes and paths.
3. Distinguish two required terms from one phrase. Verify a phrase cannot span file names or representation boundaries.
4. Verify main-window tag intersections and quick-panel tag unions over the same stored records. Count each hit once.
5. Search plain text, HTML-only visible text, multiple-file basenames and PNG extension classification. Confirm no file body, external path, script content or network URL is read for search.
6. Filter local, remote, removed-device, external and migration origins. Exercise local midnight, DST transitions, Monday weeks and month boundaries with fixed clock fixtures.
7. Assert every candidate count equals a fresh search for its complete prospective query. Test selected-tag removal, type replacement, union addition and independent zero-result relaxations.
8. Exercise query changes during paging, cursor tampering, service restart, deletions, expiry and tag updates. Distinguish live-page semantics from count snapshots.
9. Cover local/Action/remote/temporary-transfer/offline-import indexing, commit rollback, quota eviction and reopen/rebuild. Removed records must never return as search hits.
10. Inject slow or cancelled searches/counts. Restore/delete must not queue behind stale desktop search work. Verify connection/goroutine/memory/disk bounds.
11. Check Unicode highlight ranges against the exact returned preview, including emoji. No HTML markers or raw paths may leak.
12. Capture logs/events/errors/telemetry. Assert query strings, snippets, names and paths remain absent.

Use the existing demo interaction scenarios later for parser, caret, IME, focus, badges and keyboard acceptance. Native/offscreen rendering remains a separate explicitly authorized task.

## Suggested implementation order

1. Adopt normalized query semantics, source/type classification and search-content/resource limits. Update contracts to distinguish ordinary metadata listing from explicit search reads.
2. Implement one production-process slice: durable projection, text/file substring search including Chinese short terms, existing filters, chronological paging and bounded previews.
3. Add the paged candidate catalogue and batched exact counts through the same predicate evaluator.
4. Bind the main-window editor, then the quick-panel editor. Preserve their existing query mappings and interaction timing.
5. Close optional classification/pinyin/favorite gaps only after their definitions are adopted. Do not build placeholder classifiers or temporary desktop-owned indexes.
