# Phase 28: Support link content type - Research

**Researched:** 2028-03-13
**Domain:** Clipboard link content type detection, extraction, display, and sync filtering
**Confidence:** HIGH

## Summary

Phase 28 makes the link content type fully functional across the entire pipeline. The work is well-scoped: extend `classify_snapshot` to detect URLs in plain text, extract link metadata during capture, populate the `link` field in `ClipboardItemDto`, update the frontend display for links (including multi-URL), and activate the link sync toggle.

The codebase already has all the scaffolding in place: `ContentTypeCategory::Link` exists, `ClipboardItemDto.link` field exists (always `None`), frontend `ClipboardPreview` already renders link type, and `DeviceSettingsPanel` has the link toggle (currently "coming soon"). The work is primarily connecting these existing pieces with real data flow.

**Primary recommendation:** Add `url` crate to `uc-core` for URL parsing/validation in `classify_snapshot`, create a `ClipboardLinkItemDto` struct in `uc-tauri/models`, and thread link data from classification through capture to DTO mapping.

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

- **Two detection paths**: MIME `text/uri-list` (direct) AND URL detection in `text/plain` content
- **Plain text URL rule**: Entire text (after trim) must be a single valid URL -- mixed content like "see https://..." stays as text
- **Protocol recognition**: All common protocols -- http, https, ftp, ftps, mailto, etc.
- **Detection layer**: Implemented in `classify_snapshot` -- affects both sync filtering and Dashboard display
- **Dashboard display**: List view with clickable URL + ExternalLink icon; detail panel with URL, domain, character count, capture time
- **Multi-URL (text/uri-list)**: Show first URL in list + "+N more" count badge; detail panel shows all URLs
- **Link filter**: NOT enabled in this phase -- Header filter stays commented out/disabled
- **Sync filter activation**: Make link toggle operational via `is_content_type_allowed` checking `ct.link`
- **Remove "coming soon" badge** from link toggle in DeviceSettingsPanel, make it interactive
- **Storage**: URL(s) + domain name per URL
- **Extraction timing**: During CaptureClipboardUseCase processing
- **text/uri-list handling**: Parse all URLs (one per line, skip comment lines starting with #)
- **Plain text URL**: Single URL extracted, stored as single-element list
- **Domain extraction**: Parse URL to extract hostname -- no network requests

### Claude's Discretion

- Exact URL validation regex or parser choice for plain text detection
- How to store link metadata (extend existing representation metadata vs new structure)
- ClipboardLinkItem frontend type extension details (urls array + domains)
- Backend DTO structure for link data (ClipboardLinkItemDto)
- "+N more" badge styling in list view

### Deferred Ideas (OUT OF SCOPE)

None -- discussion stayed within phase scope
</user_constraints>

## Standard Stack

### Core

| Library      | Version | Purpose                    | Why Standard                                                                                    |
| ------------ | ------- | -------------------------- | ----------------------------------------------------------------------------------------------- |
| `url` (Rust) | 2.x     | URL parsing and validation | Already in `uc-tauri` Cargo.toml; implements WHATWG URL Standard; extracts host/scheme reliably |

### Supporting

| Library                  | Version | Purpose                            | When to Use                                         |
| ------------------------ | ------- | ---------------------------------- | --------------------------------------------------- |
| `url` added to `uc-core` | 2.x     | URL parsing in `classify_snapshot` | Needed because classification happens in core layer |

### Alternatives Considered

| Instead of                                   | Could Use                        | Tradeoff                                                                                             |
| -------------------------------------------- | -------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `url` crate                                  | Regex-based URL detection        | `url` is more correct for validation; regex is fragile for edge cases like IDN, ports, query strings |
| Storing link data in representation metadata | Dedicated `ClipboardLinkItemDto` | Dedicated DTO is cleaner, matches existing text/image pattern                                        |

**Installation:**

```toml
# Add to src-tauri/crates/uc-core/Cargo.toml
url = "2"
```

## Architecture Patterns

### Data Flow for Link Content

```
1. Clipboard snapshot captured (text/uri-list OR text/plain with URL)
     |
2. classify_snapshot() -> ContentTypeCategory::Link
     |-- text/uri-list: direct match (already works)
     |-- text/plain: check if trimmed content is a single valid URL (NEW)
     |
3. CaptureClipboardUseCase::execute()
     |-- Existing flow: normalize, persist event, cache, spool, select policy, persist entry
     |-- No changes needed to capture flow itself
     |
4. get_clipboard_item / get_clipboard_entries commands
     |-- DTO mapping: detect content_type == "link" or "text/uri-list"
     |-- Parse URL(s) from content, extract domains
     |-- Populate ClipboardItemDto.link field
     |
5. Frontend receives link data, renders accordingly
```

### Pattern 1: Extending classify_snapshot for Plain Text URL Detection

**What:** When snapshot has `text/plain` MIME and content is a single valid URL, classify as Link instead of Text.
**When to use:** During classification before the existing `text/plain -> Text` match.
**Key insight:** Must check `text/plain` content BEFORE returning `Text`. The function needs access to representation bytes.

```rust
// In classify_snapshot, after text/uri-list match and before text/plain match:
"text/plain" => {
    // Check if the entire content (trimmed) is a single valid URL
    if let Ok(text) = std::str::from_utf8(&rep.bytes) {
        let trimmed = text.trim();
        if !trimmed.is_empty() && url::Url::parse(trimmed).is_ok() {
            return ContentTypeCategory::Link;
        }
    }
    return ContentTypeCategory::Text;
}
```

**Important:** `classify_snapshot` currently only examines `rep.mime`, not `rep.bytes`. The function signature takes `&SystemClipboardSnapshot` which includes bytes, so no signature change needed.

### Pattern 2: ClipboardLinkItemDto Structure

**What:** Typed DTO for link data sent to frontend.
**Where:** `uc-tauri/src/models/mod.rs`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardLinkItemDto {
    pub urls: Vec<String>,
    pub domains: Vec<String>,
}
```

Update `ClipboardItemDto`:

```rust
pub link: Option<ClipboardLinkItemDto>,  // was Option<serde_json::Value>
```

### Pattern 3: Frontend ClipboardLinkItem Extension

**What:** Update frontend type to receive multi-URL data.

```typescript
export interface ClipboardLinkItem {
  urls: string[]
  domains: string[]
}
```

### Pattern 4: DTO Mapping for Links

**What:** In `clipboard.rs` commands, when `content_type` indicates a link, parse URLs from content and build link DTO.
**Where:** The `get_clipboard_item` command and `transformProjectionToResponse` (frontend).

Backend side (in clipboard.rs):

```rust
// When content_type is "text/uri-list" or classified as link
let is_link = proj.content_type == "text/uri-list"
    || (proj.content_type.starts_with("text/plain") && is_single_url(&proj.preview));

if is_link {
    let urls = parse_urls_from_content(&proj.preview, &proj.content_type);
    let domains = urls.iter().filter_map(|u| extract_domain(u)).collect();
    ClipboardItemDto {
        link: Some(ClipboardLinkItemDto { urls, domains }),
        // ... other fields None
    }
}
```

### Pattern 5: text/uri-list Parsing (RFC 2483)

**What:** Parse `text/uri-list` format per spec.
**Rules:** One URL per line, lines starting with `#` are comments, empty lines skipped.

```rust
fn parse_uri_list(content: &str) -> Vec<String> {
    content.lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter(|line| url::Url::parse(line).is_ok())
        .map(|line| line.to_string())
        .collect()
}
```

### Anti-Patterns to Avoid

- **Do NOT add link extraction to CaptureClipboardUseCase**: The capture flow persists raw representations. Link metadata should be derived at read time (DTO mapping) from the stored content, not stored separately. This avoids schema changes and keeps capture fast.
- **Do NOT use regex for URL validation**: The `url` crate implements WHATWG URL Standard correctly. Regex cannot handle all URL edge cases (IDN, IPv6, unusual ports, etc.).
- **Do NOT change ClipboardEntry domain model**: The core domain model has no content_type field. Content type is derived from representation MIME types at query time. Keep this pattern.

## Don't Hand-Roll

| Problem               | Don't Build             | Use Instead                              | Why                                                          |
| --------------------- | ----------------------- | ---------------------------------------- | ------------------------------------------------------------ |
| URL validation        | Custom regex            | `url::Url::parse()`                      | Handles IDN, IPv6, ports, query strings, fragments correctly |
| Domain extraction     | String splitting on `/` | `url::Url::host_str()`                   | Correct for all URL formats including IPv6                   |
| text/uri-list parsing | Ad-hoc splitting        | Line-by-line parse with `#` comment skip | RFC 2483 defines the format; simple but must handle comments |

## Common Pitfalls

### Pitfall 1: classify_snapshot Needs Byte Access for Plain Text URLs

**What goes wrong:** `classify_snapshot` currently only checks `rep.mime` string, not content bytes.
**Why it happens:** The function was designed for MIME-only classification.
**How to avoid:** The function already receives `&SystemClipboardSnapshot` which includes `rep.bytes`. Just access bytes inside the `text/plain` branch.
**Warning signs:** Tests that only set MIME without bytes will miss the URL detection path.

### Pitfall 2: url Crate in uc-core Dependency

**What goes wrong:** `classify_snapshot` lives in `uc-core` which doesn't have `url` crate.
**Why it happens:** `url` is only in `uc-tauri/Cargo.toml` currently.
**How to avoid:** Add `url = "2"` to `uc-core/Cargo.toml` dependencies.
**Alternative:** Move URL detection to a separate function that `classify_snapshot` calls, or use a simpler heuristic (scheme check) without full URL parsing.

### Pitfall 3: Frontend transformProjectionToResponse Only Handles Text/Image

**What goes wrong:** The `transformProjectionToResponse` function in `clipboardItems.ts` has a binary `isImage ? ... : text` logic. Links will be treated as text.
**Why it happens:** Only text and image were supported when the transform was written.
**How to avoid:** Add link detection in the transform function OR rely on backend `get_clipboard_item` which already builds `ClipboardItemDto` with the link field.
**Note:** There are TWO paths: (1) `get_clipboard_entries` -> `transformProjectionToResponse` (frontend transform) and (2) `get_clipboard_item` (backend builds DTO). Both need link handling.

### Pitfall 4: Content Type String in Projection vs Category Enum

**What goes wrong:** The `ClipboardEntryProjection.content_type` is a MIME string (e.g., "text/plain"), not the `ContentTypeCategory` enum.
**Why it happens:** Projection comes from DB which stores MIME types.
**How to avoid:** When building `ClipboardItemDto` in commands, re-classify or check both MIME string and content to determine if it's a link. For `text/uri-list` it's obvious from MIME. For `text/plain` URLs, need to check the preview text.

### Pitfall 5: "+N more" Badge for text/uri-list with Multiple URLs

**What goes wrong:** List view shows only first URL but needs a count of remaining.
**Why it happens:** Preview text might only contain the first URL.
**How to avoid:** The backend DTO should include the full URL list. Frontend reads `urls.length` for the badge count.

## Code Examples

### URL Detection in classify_snapshot (uc-core)

```rust
// Source: Project pattern + url crate docs
use url::Url;

pub fn classify_snapshot(snapshot: &SystemClipboardSnapshot) -> ContentTypeCategory {
    for rep in &snapshot.representations {
        if let Some(ref mime) = rep.mime {
            let m = mime.0.as_str();
            match m {
                "text/html" => return ContentTypeCategory::RichText,
                "text/uri-list" => return ContentTypeCategory::Link,
                "text/plain" => {
                    // Check if entire content is a single URL
                    if let Ok(text) = std::str::from_utf8(&rep.bytes) {
                        let trimmed = text.trim();
                        if !trimmed.is_empty()
                            && !trimmed.contains(char::is_whitespace)
                            && Url::parse(trimmed).is_ok()
                        {
                            return ContentTypeCategory::Link;
                        }
                    }
                    return ContentTypeCategory::Text;
                }
                "application/octet-stream" => return ContentTypeCategory::File,
                _ if m.starts_with("image/") => return ContentTypeCategory::Image,
                _ => {}
            }
        }
    }
    ContentTypeCategory::Unknown
}
```

Note: The `!trimmed.contains(char::is_whitespace)` check ensures "look at https://..." is NOT classified as link, satisfying the "entire text must be a single URL" rule.

### is_content_type_allowed Update

```rust
pub fn is_content_type_allowed(category: ContentTypeCategory, ct: &ContentTypes) -> bool {
    match category {
        ContentTypeCategory::Text => ct.text,
        ContentTypeCategory::Image => ct.image,
        ContentTypeCategory::Link => ct.link,  // NEW: was in the "always true" group
        // Remaining unimplemented types always sync
        ContentTypeCategory::RichText
        | ContentTypeCategory::File
        | ContentTypeCategory::CodeSnippet
        | ContentTypeCategory::Unknown => true,
    }
}
```

### text/uri-list Parsing

```rust
fn parse_uri_list(content: &str) -> Vec<String> {
    content
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|s| s.to_string())
        .collect()
}

fn extract_domain(url_str: &str) -> Option<String> {
    url::Url::parse(url_str)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
}
```

### DeviceSettingsPanel Toggle Change

```typescript
// Change status from 'coming_soon' to 'editable'
{ field: 'link', i18nKey: 'syncLink', status: 'editable' },
```

### Frontend ClipboardLinkItem Update

```typescript
export interface ClipboardLinkItem {
  urls: string[]
  domains: string[]
}

// In ClipboardItemRow getPreviewText:
case 'link': {
  const linkItem = item.content as ClipboardLinkItem
  return linkItem.urls[0] ?? ''
}

// In ClipboardPreview for multi-URL:
case 'link': {
  const linkItem = item.content as ClipboardLinkItem
  // Show all URLs with domain info
}
```

## State of the Art

| Old Approach                   | Current Approach                | When Changed | Impact                        |
| ------------------------------ | ------------------------------- | ------------ | ----------------------------- |
| Link always `None` in DTO      | Populated from content          | Phase 28     | Links visible in Dashboard    |
| Link toggle "coming soon"      | Link toggle interactive         | Phase 28     | Users can filter link sync    |
| Only MIME-based link detection | MIME + plain text URL detection | Phase 28     | More links captured correctly |

## Open Questions

1. **Where to put URL parsing utility functions?**
   - Recommendation: Add a `link_utils.rs` module in `uc-core/src/clipboard/` for `parse_uri_list()` and `is_single_url()`. Add a `link_dto_builder` helper in `uc-tauri/src/commands/` or `uc-tauri/src/models/` for DTO construction.

2. **Should `url` crate be added to uc-core or kept in uc-tauri?**
   - Recommendation: Add to `uc-core`. The `classify_snapshot` function lives there and needs URL parsing. The `url` crate is lightweight (pure Rust, no system deps) and appropriate for the core layer.

3. **How to handle `mailto:` URLs in domain extraction?**
   - `url::Url::parse("mailto:user@example.com")` works. `host_str()` returns `None` for mailto. Use the email domain part instead, or just show the full URL without domain.
   - Recommendation: For mailto, extract domain from the email address string. For other schemeless cases, domain can be `None`.

## Sources

### Primary (HIGH confidence)

- Project codebase: `uc-core/src/settings/content_type_filter.rs` - existing classification logic
- Project codebase: `uc-tauri/src/commands/clipboard.rs` - existing DTO mapping
- Project codebase: `uc-tauri/src/models/mod.rs` - existing DTO structures
- Project codebase: `src/api/clipboardItems.ts` - existing frontend types
- Project codebase: `src/components/clipboard/ClipboardPreview.tsx` - existing link rendering
- Project codebase: `src/components/device/DeviceSettingsPanel.tsx` - existing toggle config
- `url` crate v2 - WHATWG URL Standard implementation for Rust

### Secondary (MEDIUM confidence)

- RFC 2483 - text/uri-list MIME type format (one URL per line, # comments)

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH - `url` crate already in project, well-known Rust ecosystem standard
- Architecture: HIGH - clear pattern from existing text/image handling, straightforward extension
- Pitfalls: HIGH - identified from direct code reading, all integration points inspected

**Research date:** 2028-03-13
**Valid until:** 2026-04-13 (stable domain, no external API dependencies)
