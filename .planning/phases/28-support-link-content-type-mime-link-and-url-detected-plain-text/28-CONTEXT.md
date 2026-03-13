# Phase 28: Support link content type (MIME link and URL-detected plain text) - Context

**Gathered:** 2028-03-13
**Status:** Ready for planning

<domain>
## Phase Boundary

Make the link content type fully functional across the entire pipeline: classification, capture/extraction, Dashboard display, and sync filtering. This includes both MIME-based link detection (text/uri-list) and URL detection in plain text content. The link toggle in per-device sync settings becomes operational.

</domain>

<decisions>
## Implementation Decisions

### Link identification scope

- **Two detection paths**: MIME `text/uri-list` (direct) AND URL detection in `text/plain` content
- **Plain text URL rule**: Entire text (after trim) must be a single valid URL — mixed content like "看这个 https://..." stays as text
- **Protocol recognition**: All common protocols — http, https, ftp, ftps, mailto, etc.
- **Detection layer**: Implemented in `classify_snapshot` — affects both sync filtering and Dashboard display
- Current `classify_snapshot` only matches `text/uri-list`; needs extension to inspect `text/plain` content for single-URL detection

### Dashboard display

- **List view**: Clickable URL with ExternalLink icon (existing frontend rendering logic, just needs backend to populate link data)
- **Detail panel**: Complete URL (copyable), domain name, character count, capture time — consistent with text detail style
- **Multi-URL (text/uri-list)**: Show first URL in list + "+N more" count badge; detail panel shows all URLs
- **Link filter**: NOT enabled in this phase — Header filter stays commented out/disabled
- Backend DTO `link` field (currently always `None`) must be populated when content type is link

### Sync filter activation

- Make link toggle operational: change `is_content_type_allowed` to respect `ct.link` for `ContentTypeCategory::Link`
- Remove "coming soon" badge from link toggle in DeviceSettingsPanel, make it interactive
- Other unimplemented types (file, code_snippet, rich_text) remain "coming soon"

### Link data extraction

- **Storage**: URL(s) + domain name per URL
- **Extraction timing**: During CaptureClipboardUseCase processing — data immediately available
- **text/uri-list handling**: Parse all URLs (one per line, skip comment lines starting with #), store as list
- **Plain text URL**: Single URL extracted, stored as single-element list for consistent structure
- **Domain extraction**: Parse URL to extract hostname (e.g., "github.com") — no network requests needed

### Claude's Discretion

- Exact URL validation regex or parser choice for plain text detection
- How to store link metadata (extend existing representation metadata vs new structure)
- ClipboardLinkItem frontend type extension details (urls array + domains)
- Backend DTO structure for link data (ClipboardLinkItemDto)
- "+N more" badge styling in list view

</decisions>

<specifics>
## Specific Ideas

No specific requirements — open to standard approaches

</specifics>

<code_context>

## Existing Code Insights

### Reusable Assets

- `classify_snapshot()` in `uc-core/src/settings/content_type_filter.rs`: Already classifies `text/uri-list` → Link; needs extension for plain text URL detection
- `is_content_type_allowed()` in same file: Already has `ContentTypeCategory::Link` match arm — just returns `true` unconditionally, needs to check `ct.link`
- `ClipboardItemDto` in `uc-tauri/src/models/mod.rs`: Has `link: Option<serde_json::Value>` field — always set to `None`, needs population
- `ClipboardLinkItem` in `src/api/clipboardItems.ts`: Has `{url: string}` — needs extension to `{urls: string[], domains: string[]}` or similar
- `ClipboardPreview.tsx`: Already renders link type with clickable URL — needs update for multi-URL and domain display
- `ClipboardItemRow.tsx`: Already has ExternalLink icon mapping for link type
- `DeviceSettingsPanel.tsx`: Has `contentTypeEntries` with editable/coming_soon status per type — link needs status change

### Established Patterns

- Content type classification via MIME type matching in `classify_snapshot`
- Sync filtering via `apply_sync_policy` (renamed from `filter_by_auto_sync` in Phase 25) with `classify_snapshot` + `is_content_type_allowed`
- DTO mapping in `uc-tauri/src/models/mod.rs` converts domain models to frontend-compatible JSON
- `CaptureClipboardUseCase` processes snapshots and persists representations with metadata

### Integration Points

- `classify_snapshot()` — extend with plain text URL detection logic
- `is_content_type_allowed()` — add `ct.link` check for Link category
- `CaptureClipboardUseCase` — extract URL(s) and domain(s) during capture
- `ClipboardItemDto` mapping — populate `link` field from extracted data
- `DeviceSettingsPanel.tsx` — change link toggle from "coming soon" to editable
- `ClipboardPreview.tsx` — update detail panel for link entries (domain, multi-URL)

</code_context>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

_Phase: 28-support-link-content-type-mime-link-and-url-detected-plain-text_
_Context gathered: 2028-03-13_
