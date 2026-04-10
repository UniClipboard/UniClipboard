# Technology Stack: Local Encrypted Search (v0.5.0)

**Project:** UniClipboard Desktop
**Researched:** 2026-04-10
**Focus:** Stack additions for HMAC-keyed inverted index, tokenization, and search query execution
**Confidence:** HIGH

## Context: What Is Already In Place

Do not re-add or replace any of these. They are the integration surface for the new search feature.

| Technology              | Version | Crate(s)              | Relevant to Search                                  |
| ----------------------- | ------- | --------------------- | --------------------------------------------------- |
| Diesel 2 + SQLite       | 2.3.5   | uc-infra              | Database ORM and migration system — search schema goes here |
| libsqlite3-sys bundled  | 0.35    | uc-infra              | SQLite compiled in — WAL mode, r2d2 pool ready      |
| diesel_migrations       | 2.2.0   | uc-infra              | Embedded migration runner — new search tables use same system |
| blake3                  | 1.8.2   | uc-core, uc-infra     | Keyed hash for term tags; derive_key for search key |
| chacha20poly1305        | 0.10.1  | uc-infra              | Existing content encryption — do NOT reuse for search |
| argon2                  | 0.5.3   | uc-core, uc-infra     | KDF for KEK — search key derived from MasterKey, not passphrase |
| url                     | 2       | uc-core               | URL parsing for host/path/query extraction          |
| axum                    | 0.7     | uc-daemon             | HTTP server — search query routes added here        |
| tokio                   | 1       | all crates            | Async runtime                                       |
| serde / serde_json      | 1       | all crates            | DTO serialization                                   |

## New Dependencies Required

### 1. unicode-normalization — NFKC Normalization

| Property       | Value |
| -------------- | ----- |
| **Crate**      | `unicode-normalization` |
| **Version**    | `0.1` |
| **Latest**     | 0.1.25 (verified 2026-04-10) |
| **Where**      | `uc-infra` (tokenizer/normalization service) |
| **Why**        | The architecture spec requires NFKC normalization before tokenization. NFKC is stable, version-deterministic behavior needed for `index_version` guarantees. This is the canonical Rust crate from the unicode-rs organization. No alternative exists with the same coverage. |
| **Confidence** | HIGH |

```toml
# uc-infra/Cargo.toml
unicode-normalization = "0.1"
```

### 2. unicode-segmentation — UAX#29 Word Boundaries

| Property       | Value |
| -------------- | ----- |
| **Crate**      | `unicode-segmentation` |
| **Version**    | `1` |
| **Latest**     | 1.12.0 (verified 2026-04-10) |
| **Where**      | `uc-infra` (tokenizer) |
| **Why**        | UAX#29 word-boundary splitting for Latin text, paths, URLs, file names, code tokens. More correct than splitting on whitespace or regex `\W+` — handles apostrophes, hyphens, and mixed punctuation in clipboard content. The `unicode-segmentation` crate is the standard Rust implementation (unicode-rs org, same authors as `unicode-normalization`). |
| **Confidence** | HIGH |

```toml
# uc-infra/Cargo.toml
unicode-segmentation = "1"
```

### 3. HTML Tag Stripping — No Crate Needed

The architecture spec says HTML entries must have tags stripped ("去标签") before indexing. For V1 exact-keyword search, spec-compliant HTML5 parsing is not required — simple tag stripping plus basic entity decode is sufficient and adds zero dependencies.

Implement inline in the text extractor (~30 lines):

```rust
fn strip_html(html: &str) -> String {
    // Strip all tags
    let no_tags = regex_or_manual_strip(html);  // remove <...> spans
    // Decode common entities
    no_tags
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&nbsp;", " ")
        .replace("&#39;", "'")
}
```

Tag stripping without a regex crate: iterate chars, skip content between `<` and `>`. This is ~15 lines of safe Rust with no allocations beyond the output string.

**Why not `html2text 0.16` or `nanohtml2text`:** `html2text` pulls in `html5ever` (Servo's HTML5 parser) — significant binary size increase for tag-stripping only. `nanohtml2text` is a low-traffic crate; verifying its maintenance status adds friction. The V1 spec goal ("去标签") does not require a browser-grade parser.

## Decisions: What NOT to Add

| Avoid | Why | Use Instead |
| ----- | --- | ----------- |
| `rusqlite` | The codebase uses Diesel exclusively. Adding a second SQLite access layer creates two connection pools, two migration systems, and coordination complexity. | Diesel 2.3.5 (already present). Use `diesel::sql_query()` for posting-list queries that Diesel's query builder cannot express cleanly. |
| `ring` crate | Already rejected at project start in favor of Rust-native crypto. Not in the workspace. | `blake3::keyed_hash()` (already in uc-core and uc-infra). |
| `hmac` crate (from RustCrypto) | New crypto dependency for a use case that blake3 keyed mode already covers. They are equivalent PRFs for this purpose. | `blake3 1.8.2` — already present. |
| `sha2` for HMAC | Using SHA2-HMAC for term tags would require the `hmac` crate and diverges from the blake3-first pattern already established in this workspace. | `blake3::keyed_hash()` |
| `jieba-rs` or `lindera` | Full CJK dictionary segmenters. V1 uses bigrams, which require no dictionary. These add multi-MB dictionaries to the binary. | Manual bigram generation in the tokenizer (~15 lines): detect CJK codepoint ranges U+4E00–U+9FFF, U+3400–U+4DBF, U+F900–U+FAFF, slide a 2-char window. |
| `tantivy` | Full-text search engine — would replace the HMAC inverted index design entirely. Not aligned with the security model (Tantivy stores plaintext terms on disk). | Custom Diesel-backed inverted index as specified in the architecture doc. |
| Tauri `#[command]` for search | Search requests are routed through the daemon HTTP API (axum), not directly through Tauri IPC. The existing GUI→daemon architecture requires search to follow the same path as other operations. | Axum route in `uc-daemon::api` following existing patterns. |
| `html2text 0.16` | Pulls in `html5ever` (Servo HTML5 parser). Binary size cost is unwarranted for tag-stripping only. | Zero-dep manual tag stripping (see above). |
| `nanohtml2text` | Low-traffic crate; maintenance status uncertain. The simple inline approach has no dependency risk. | Zero-dep manual tag stripping (see above). |

## Integration Details

### HMAC-Keyed Term Tags: blake3 keyed_hash

The architecture spec calls for `HMAC(search_key, normalized_token)`. The implementation uses blake3's built-in keyed hash mode, which is a PRF with equivalent security properties for this use case.

```rust
// In uc-infra (tokenizer/normalization service)
// search_key: [u8; 32] derived from MasterKey via blake3::derive_key

fn term_tag(search_key: &[u8; 32], normalized_token: &str) -> [u8; 32] {
    blake3::keyed_hash(search_key, normalized_token.as_bytes()).into()
}
```

No new dependency. `blake3 1.8.2` already in `uc-infra/Cargo.toml`.

### Search Key Derivation: blake3 derive_key

The search key must be separate from content encryption keys. Derive it from the unlocked `MasterKey` using blake3's domain-separated key derivation:

```rust
// In uc-infra (search key derivation adapter)
// context string must be unique and stable — never change after V1 ships
const SEARCH_KEY_CONTEXT: &str = "uniclipboard.local-search.v1.term-key";

fn derive_search_key(master_key: &MasterKey) -> [u8; 32] {
    blake3::derive_key(SEARCH_KEY_CONTEXT, master_key.as_bytes())
}
```

No new dependency. `blake3::derive_key` is part of the blake3 1.x public API.

### Database Schema: Diesel Migrations

Add new Diesel migrations in `uc-infra/migrations/`:

```
migrations/
  2026-04-10-000001_create_search_index/
    up.sql    -- CREATE TABLE search_document, search_posting, search_index_meta
    down.sql  -- DROP TABLE ...
```

The `search_document` and `search_posting` tables are added to `uc-infra/src/db/schema.rs` after running `diesel migration run`. No new SQLite library needed.

### Posting-List Intersection: diesel::sql_query with GROUP BY / HAVING

For AND queries (entry must match all N term tags), use a single `GROUP BY entry_id HAVING COUNT = N` query rather than chained INTERSECT clauses. This parameterizes cleanly regardless of term count:

```rust
// AND: entry_ids that appear in ALL N posting lists
// term_tags: Vec<[u8; 32]>, bound as one ? per tag
diesel::sql_query(
    "SELECT entry_id FROM search_posting
     WHERE term_tag IN (?, ?)          -- one ? per term
     GROUP BY entry_id
     HAVING COUNT(DISTINCT term_tag) = ?"  // = N
)
.bind::<Binary, _>(&term_tags[0])
.bind::<Binary, _>(&term_tags[1])
.bind::<Integer, _>(term_tags.len() as i32)
```

For OR queries (entry must match any term tag), use a plain `WHERE term_tag IN (...)` with `DISTINCT`:

```rust
// OR: entry_ids that appear in ANY posting list
diesel::sql_query(
    "SELECT DISTINCT entry_id FROM search_posting WHERE term_tag IN (?)"
)
```

This pattern already has a precedent in the codebase — `pool.rs` uses `diesel::sql_query` for PRAGMAs.

### Daemon HTTP Route: Axum 0.7

Search query, rebuild, and index-status endpoints are added to `uc-daemon::api` following the existing route pattern:

```rust
// uc-daemon/src/api/search.rs (new file)
pub fn router() -> Router<Arc<DaemonApiState>> {
    Router::new()
        .route("/search", post(search_clipboard))
        .route("/search/rebuild", post(rebuild_index))
        .route("/search/status", get(index_status))
}
```

No new Axum version or features needed. The existing `axum 0.7` with `tokio` and `http1` features covers this.

### CJK Bigram: No Crate Needed

CJK bigram generation requires no library. Implement inline in the tokenizer:

```rust
fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x4E00..=0x9FFF    // CJK Unified Ideographs
        | 0x3400..=0x4DBF  // CJK Extension A
        | 0xF900..=0xFAFF  // CJK Compatibility Ideographs
        | 0x20000..=0x2A6DF // CJK Extension B
    )
}

fn cjk_bigrams(s: &str) -> impl Iterator<Item = String> + '_ {
    let chars: Vec<char> = s.chars().filter(|c| is_cjk(*c)).collect();
    chars.windows(2).map(|w| w.iter().collect::<String>())
}
```

### Tokenizer Pipeline Summary

For each clipboard entry being indexed, the text extraction and tokenization pipeline in `uc-infra` is:

1. Extract text fields from clipboard domain model (body_text, html_text, url_text, file_path_text, file_name_text)
2. For HTML fields: strip tags (manual char iteration) + decode basic HTML entities
3. For URL fields: parse with the existing `url` crate (already in uc-core) to extract host, path segments, query keys
4. For all text: apply NFKC normalization (`unicode-normalization`)
5. Lowercase
6. Split into word-boundary tokens (`unicode-segmentation`)
7. CJK ranges: additionally generate bigrams
8. For each token: compute `term_tag = blake3::keyed_hash(search_key, token)`
9. Write `(term_tag, entry_id, field_mask, term_freq)` rows to `search_posting`

## Cargo.toml Changes Summary

### uc-infra/Cargo.toml — ADD (2 new crates only)

```toml
unicode-normalization = "0.1"
unicode-segmentation = "1"
```

### All Other Crates — No Changes

blake3, url, diesel, and axum are already at the required versions with the required features. No changes to uc-core, uc-app, uc-daemon, or uc-tauri Cargo.toml files for the dependency layer.

`uc-core` gains new types (`SearchQuery`, `SearchResult`, `SearchFilter`, `SearchIndexPort`) but no new crate dependencies.

`uc-app` gains new use cases (`SearchClipboardEntries`, `IndexClipboardEntry`, `RemoveIndexedClipboardEntry`, `RebuildSearchIndex`) but no new crate dependencies.

`uc-daemon` gains new HTTP routes but no new crate dependencies.

## Version Verification

| Crate                   | Recommended | Latest Verified | Date Verified | Source               |
| ----------------------- | ----------- | --------------- | ------------- | -------------------- |
| unicode-normalization   | 0.1         | 0.1.25          | 2026-04-10    | WebSearch            |
| unicode-segmentation    | 1           | 1.12.0          | 2026-04-10    | WebSearch            |
| blake3                  | 1.8.2       | 1.8.2 (locked)  | 2026-04-10    | Cargo.lock           |
| diesel                  | 2.3.5       | 2.3.5 (locked)  | 2026-04-10    | Cargo.lock           |
| axum                    | 0.7         | in use          | 2026-04-10    | uc-daemon/Cargo.toml |

## Alternatives Considered

| Decision | Recommended | Alternative | Why Not |
| -------- | ----------- | ----------- | ------- |
| SQLite access | Diesel + `diesel::sql_query()` | rusqlite | Two competing SQLite layers, dual migration systems, additional connection pool. Diesel handles the posting-list queries via raw SQL escape hatch. |
| Term tag PRF | `blake3::keyed_hash()` | `hmac` + `sha2` | New dependency for same security property. blake3 already present in all crypto-adjacent crates in this workspace. |
| Search key derivation | `blake3::derive_key()` | HKDF | HKDF requires `hkdf` crate (new dep). blake3 domain-separated derive_key is equivalent, zero new deps. |
| CJK handling | Manual bigram (no crate) | `jieba-rs`, `lindera` | Dictionary segmenters add MBs of data. V1 bigram spec requires no dictionary lookup. |
| HTML stripping | Manual char iteration (no crate) | `html2text 0.16`, `nanohtml2text` | html2text pulls in html5ever. nanohtml2text has uncertain maintenance. Manual ~30-line implementation has zero dependency risk. |
| AND query pattern | `GROUP BY entry_id HAVING COUNT = N` | chained INTERSECT | GROUP BY/HAVING is a single parameterized query regardless of term count; INTERSECT requires dynamic SQL construction proportional to N. |

## Confidence Assessment

| Area | Confidence | Rationale |
| ---- | ---------- | --------- |
| blake3 keyed_hash / derive_key for crypto | HIGH | Already in lockfile at 1.8.2; `keyed_hash` and `derive_key` are stable public API |
| Diesel + diesel::sql_query for index queries | HIGH | Pattern already used in pool.rs; GROUP BY/HAVING is standard SQLite |
| unicode-normalization 0.1 | HIGH | Canonical crate, unicode-rs org, API stable since 0.1.x |
| unicode-segmentation 1.x | HIGH | Canonical crate, unicode-rs org, API stable |
| Manual HTML tag stripping (no crate) | HIGH | Simple char-iteration approach, no external correctness dependency |
| CJK bigram via manual codepoint ranges | HIGH | Codepoint ranges are stable Unicode standard, not library-dependent |
| No rusqlite, no ring, no hmac | HIGH | All ruled out by existing codebase patterns and security model |

## Sources

- Cargo.lock at `src-tauri/Cargo.lock` — blake3 1.8.2 verified
- `uc-infra/Cargo.toml` — Diesel 2.3.5, libsqlite3-sys bundled, blake3 present
- `uc-core/Cargo.toml` — blake3, url, argon2 present; confirms no rusqlite
- `uc-infra/src/db/pool.rs` — diesel::sql_query pattern confirmed in use
- `uc-daemon/Cargo.toml` — axum 0.7, no changes needed
- [unicode-segmentation on crates.io](https://crates.io/crates/unicode-segmentation) — 1.12.0 current
- [unicode-normalization on crates.io](https://crates.io/crates/unicode-normalization) — 0.1.25 current
- [html2text on crates.io](https://crates.io/crates/html2text) — 0.16.7 (considered, rejected)
- Architecture spec: `docs/architecture/local-encrypted-search.md`

---

_Stack research for: UniClipboard v0.5.0 Local Encrypted Search milestone_
_Researched: 2026-04-10_
