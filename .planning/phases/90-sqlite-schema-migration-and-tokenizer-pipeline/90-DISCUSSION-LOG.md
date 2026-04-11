# Phase 90: SQLite Schema Migration and Tokenizer Pipeline - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in `90-CONTEXT.md` — this log preserves the alternatives considered.

**Date:** 2026-04-10
**Phase:** 90-sqlite-schema-migration-and-tokenizer-pipeline
**Areas discussed:** Extraction scope, Tokenization rules, Version switching, Result summary, Metadata density, Isolation boundary

---

## Extraction Scope

### Source Priority

| Option | Description | Selected |
|--------|-------------|----------|
| Plain text priority | Keep plain text as the main searchable source when both plain text and HTML exist | ✓ |
| Merge both | Maximize recall by indexing both representations together | |
| HTML priority | Favor the rich-text representation over plain text | |

**User's choice:** Plain text priority  
**Notes:** User clarified that when the indexed representation itself is HTML, "visible text only" is the intended HTML behavior.

### HTML Content

| Option | Description | Selected |
|--------|-------------|----------|
| Visible text only | Strip markup and index only human-visible text | ✓ |
| Visible text + common attrs | Also include common attributes such as links / titles / alt text | |
| Index as much as possible | Aggressively include additional HTML-derived text | |

**User's choice:** Visible text only  
**Notes:** Applies specifically to HTML representations, not to entries where plain text already exists as the authoritative source.

### URL Content

| Option | Description | Selected |
|--------|-------------|----------|
| Host + path + query keys | Balanced URL extraction | ✓ |
| Include query values too | Higher recall but more noise/sensitivity risk | |
| Host only | Minimal, low-noise extraction | |

**User's choice:** Host + path + query keys

### File Path Content

| Option | Description | Selected |
|--------|-------------|----------|
| File name + extension + directory segments | Balanced file/path extraction | ✓ |
| File name only | Minimal extraction | |
| Full raw path | Maximum recall, highest noise | |

**User's choice:** File name + extension + directory segments

## Tokenization Rules

### Common Separators

| Option | Description | Selected |
|--------|-------------|----------|
| Split common separators | Split `_`, `-`, `.`, `/`, etc. | ✓ |
| Avoid splitting | Prefer whole segments only | |
| Split more aggressively | Push recall higher at the cost of more noise | |

**User's choice:** Split common separators

### camelCase / PascalCase

| Option | Description | Selected |
|--------|-------------|----------|
| Split camelCase/PascalCase | Make code identifiers searchable by parts | ✓ |
| Do not split | Keep case-joined identifiers whole | |
| Split only separators | Split `_` / `-` / `.` / `/`, but not case changes | |

**User's choice:** Split camelCase/PascalCase

### Whole Segment Preservation

| Option | Description | Selected |
|--------|-------------|----------|
| Keep whole segment too | Preserve exact whole-segment matches after splitting | ✓ |
| Keep only split parts | Leaner index, worse exact whole-segment search | |
| Keep only whole segment | Most conservative behavior | |

**User's choice:** Keep whole segment too

### Short Latin Tokens

| Option | Description | Selected |
|--------|-------------|----------|
| Keep length >= 2 | Preserve useful short abbreviations like `ts`, `ui`, `db` | ✓ |
| Keep everything | Maximum recall, more noise | |
| Keep length >= 3 | Cleaner index, loses many useful abbreviations | |

**User's choice:** Keep length >= 2

## Version Switching

### Version Mismatch Behavior

| Option | Description | Selected |
|--------|-------------|----------|
| Block search first | Prefer correctness over degraded best-effort results | ✓ |
| Keep searching anyway | Allow potentially stale / incomplete results | |
| Let the user decide | Prompt the user for degraded vs blocked behavior | |

**User's choice:** Block search first

### Rebuild Trigger Timing

| Option | Description | Selected |
|--------|-------------|----------|
| Auto-rebuild after unlock | Start rebuild automatically on the first unlocked opportunity | ✓ |
| Rebuild on first search | Delay the rebuild until the first search attempt | |
| Manual only | Require the user to trigger rebuild explicitly | |

**User's choice:** Auto-rebuild after unlock

### User Communication

| Option | Description | Selected |
|--------|-------------|----------|
| Explicit status | Clearly explain that the index is rebuilding and search is unavailable | ✓ |
| Busy state only | Show activity without real explanation | |
| Keep it subtle | Minimize interruption / explanation | |

**User's choice:** Explicit status

### Failure Behavior

| Option | Description | Selected |
|--------|-------------|----------|
| Stay blocked | Keep search blocked until rebuild succeeds | ✓ |
| Fall back to old index | Prefer availability over correctness | |
| Stop auto flow and wait | Convert into a manual-only recovery path | |

**User's choice:** Stay blocked

## Result Summary

### Link Results

| Option | Description | Selected |
|--------|-------------|----------|
| Human text first, URL fallback | Prefer readable text when present | ✓ |
| Always URL | Show the raw URL as the summary | |
| Domain first | Favor the site identity over the specific content | |

**User's choice:** Human text first, URL fallback

### File Results

| Option | Description | Selected |
|--------|-------------|----------|
| File-name list | Prefer readable file names | ✓ |
| File name + directory hint | Add extra path context | |
| Full path | Show the whole raw path | |

**User's choice:** File-name list

### Summary Length

| Option | Description | Selected |
|--------|-------------|----------|
| Short summary | Optimize for fast scanning in a result list | ✓ |
| Medium summary | Show more information per row | |
| No stored summary | Avoid any summary-like field in the result path | |

**User's choice:** Short summary

### Multi-file Summary

| Option | Description | Selected |
|--------|-------------|----------|
| Representative names + count | Show a few names and the overall quantity | ✓ |
| Count only | Show quantity without representative names | |
| Try to list everything | Maximize detail in the summary | |

**User's choice:** Representative names + count

## Metadata Density

### Document Row Weight

| Option | Description | Selected |
|--------|-------------|----------|
| Carry everything needed for rendering | Make the index document row render-ready | |
| Store search-essential metadata only | Keep the persisted row lean | ✓ |
| Store a middle ground | Partial render-readiness | |

**User's choice:** Store search-essential metadata only

### Time Fields

| Option | Description | Selected |
|--------|-------------|----------|
| Keep `active_time` and `captured_at` | Preserve both time perspectives | ✓ |
| Keep `active_time` only | Simpler, more recency-focused | |
| Keep `captured_at` only | Simpler, more storage-focused | |

**User's choice:** Keep `active_time` and `captured_at`

### Type Fields

| Option | Description | Selected |
|--------|-------------|----------|
| Keep stable type + raw MIME | Preserve both the normalized filter type and original context | ✓ |
| Keep stable type only | Simpler normalized storage | |
| Keep raw MIME only | Most literal / least normalized | |

**User's choice:** Keep stable type + raw MIME

### Summary Storage

| Option | Description | Selected |
|--------|-------------|----------|
| Do not persist summary in the index row | Keep the index row lean; enrich later if needed | ✓ |
| Still persist a short summary | Keep the index row slightly richer | |
| Decide later | Leave summary placement unresolved for now | |

**User's choice:** Do not persist summary in the index row  
**Notes:** This refines the earlier "search-essential metadata only" decision.

### File Extensions

| Option | Description | Selected |
|--------|-------------|----------|
| Keep all unique extensions | Best support for extension filtering | ✓ |
| Keep only a primary extension | Simpler but lossy | |
| Do not store separately | Breaks or weakens extension filtering | |

**User's choice:** Keep all unique extensions

## Isolation Boundary

### Isolation Scope

| Option | Description | Selected |
|--------|-------------|----------|
| Isolate all search tables | Documents, postings, and index status are profile-scoped | ✓ |
| Isolate documents/postings only | Leave status/meta less isolated | |
| Add isolation later | Start single-profile and migrate later | |

**User's choice:** Isolate all search tables

### Rebuild Scope

| Option | Description | Selected |
|--------|-------------|----------|
| Rebuild current profile only | Keep rebuild work scoped and non-interfering | ✓ |
| Rebuild all profiles together | One global rebuild path | |
| Let the user choose | Expose rebuild scope as a runtime choice | |

**User's choice:** Rebuild current profile only

### Explicit `profile_id`

| Option | Description | Selected |
|--------|-------------|----------|
| Add `profile_id` from day one | Avoid future migration pain and keep boundaries explicit | ✓ |
| Omit it for now | Optimize for today's single default profile | |
| Add it to some tables only | Partial / mixed isolation | |

**User's choice:** Add `profile_id` from day one

### Cross-profile Tags

| Option | Description | Selected |
|--------|-------------|----------|
| Never allow the same tag across profiles | Separate profile boundaries at the tag layer too | ✓ |
| Allow identical tags | Simpler, weaker isolation | |
| Leave unresolved | Do not lock this boundary yet | |

**User's choice:** Never allow the same tag across profiles

## the agent's Discretion

- Exact SQL column types and index names
- Exact HKDF info-string byte layout
- Exact summary truncation length
- Exact separator list beyond the user-approved core set
- Exact rebuild-status wording / event naming

## Deferred Ideas

- Richer HTML attribute indexing beyond visible text
- URL query-value indexing
- Full raw-path indexing
- Turning the persisted SQLite row into a fat render-ready object

## Reviewed Todos

- `修复 setup 配对确认提示缺失` — reviewed and intentionally not folded into Phase 90 because it is unrelated UI/setup work
