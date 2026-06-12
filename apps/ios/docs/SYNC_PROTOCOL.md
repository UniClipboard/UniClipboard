# SyncClipboard Wire Protocol

This document specifies the on-the-wire protocol and persisted-data formats used
by the Flutter Android client to talk to a [SyncClipboard](https://github.com/Jeric-X/SyncClipboard)
server. It is the **authoritative reference for any new client implementation**
(SwiftUI / iOS, Web, etc.). It is derived from, and must be kept in sync with,
the Flutter implementation under `lib/dio/`, `lib/service/`, `lib/utils/` and
`lib/model/`.

If the code and this document disagree, the code wins until this document is
updated to match — the only acceptable resolution is to update this document.

Worked examples are checked in under [`examples/`](./examples/). They double
as round-trip test fixtures for any new client implementation.

---

## 1. Transport

| Concern | Value |
|---|---|
| Protocol | HTTP/1.1 or HTTP/2 over TCP, optionally TLS |
| Methods | `GET`, `PUT` |
| Authentication | HTTP Basic (`Authorization: Basic <base64(username + ":" + password)>`) on **every** request |
| TLS | Both `http://` and `https://` schemes are accepted. The client MAY offer a "trust insecure cert" toggle that disables certificate validation entirely (development/LAN only). |
| Connect timeout | 5 s nominal. This client (URLSession) has no separate connect clock; it uses a **10 s idle timeout** (`timeoutIntervalForRequest` — resets whenever data arrives, so it caps connect + server think-time, not transfer size). The idle timeout is the *backstop* for a blackholed route (e.g. a LAN IP dialed over cellular); the primary defense is active cancellation on network change (§5.3). |
| Send timeout | 10 min (large file uploads) |
| Receive timeout | 5 min (large file downloads; this client: `timeoutIntervalForResource` = 5 min) |
| Status codes treated as success | `200`, `201`, `204` |
| Status code treated as auth failure | `401` |
| Status code treated as not-found | `404` |
| Other `< 500` | Surface as a generic protocol error (do not retry automatically) |
| `>= 500` | Surface as transport error (the Flutter client uses `validateStatus: status < 500`, so dio raises) |

### 1.1 Base URL normalization

Before issuing requests the client MUST normalize the configured server URL:

1. Trim leading/trailing whitespace.
2. If the result does not end in `/`, append `/`.

All endpoint paths in §2 are relative paths joined onto this base URL.

### 1.2 Authorization header

```
Authorization: Basic <base64(utf8(username + ":" + password))>
```

The header is added to every request via an interceptor. There is no token
exchange, no session, and no refresh.

---

## 2. Endpoints

The server exposes two logical resource families:

1. **Live clipboard** — §2.1 – §2.4. The single most-recently-published entry
   plus its payload file. WebDAV-style key/value store; what's there is what's
   "current".
2. **History** — §2.6 – §2.12. A first-class server-side store of past
   entries with starring/pinning/soft-delete/versioning. Each record is
   addressable; clients can list (paginated), update, and incrementally sync.

Plus two utility endpoints (§2.5, §2.13).

> The Android client (`src/services/SyncClipboardClient.ts`) is the reference
> implementation. Both `/SyncClipboard.json` and `/api/history/*` are part of
> the SyncClipboard protocol — a server claiming SyncClipboard compatibility
> is expected to implement both families. The Android client wires them up in
> a single `SyncClipboardClient` class implementing two interfaces
> (`ISyncClipboardAPI` + `IHistoryAPI`); the iOS port should mirror that.

### 2.1 `GET SyncClipboard.json` — pull current clipboard state

Retrieves the JSON describing the most recently synced clipboard entry.

- **Method:** `GET`
- **Path:** `SyncClipboard.json`
- **Request body:** none
- **Response headers:** `Content-Type` SHOULD be `application/json` but the
  client also accepts a string body and parses it as JSON.
- **Response body:** see §3 (Clipboard JSON schema)
- **Status codes:**
  - `200 OK` — body is the Clipboard JSON
  - `401 Unauthorized` — credentials wrong
  - `404 Not Found` — no clipboard has ever been published yet
  - other — error

### 2.2 `PUT SyncClipboard.json` — publish clipboard state

Publishes a new clipboard entry. If the entry has an associated payload file,
that file MUST be uploaded **first** (§2.3) before `PUT SyncClipboard.json` is
called, otherwise other clients will see a metadata pointer to a file that does
not yet exist on the server.

- **Method:** `PUT`
- **Path:** `SyncClipboard.json`
- **Request `Content-Type`:** `application/json`
- **Request body:** Clipboard JSON (§3)
- **Response body:** ignored
- **Status codes:**
  - `200 OK` / `201 Created` / `204 No Content` — success
  - `401` — auth failure
  - other — error

### 2.3 `PUT file/<filename>` — upload payload file

Uploads the binary payload referred to by a Clipboard JSON whose `hasData` is
`true`. Used for images, generic files, large-text overflow, and group ZIPs.

- **Method:** `PUT`
- **Path:** `file/<filename>` where `<filename>` is exactly the value of
  `Clipboard.dataName` (§3.2). It MUST NOT contain path separators.
- **Request `Content-Type`:** `application/octet-stream`
- **Request `Content-Length`:** the byte length of the body
- **Request body:** raw bytes of the file
- **Status codes:** as in §2.2

### 2.4 `GET file/<filename>` — download payload file

- **Method:** `GET`
- **Path:** `file/<filename>` (the value of `Clipboard.dataName` from the most
  recent `GET SyncClipboard.json`)
- **Response body:** raw bytes
- **Status codes:**
  - `200` — bytes
  - `401` — auth failure
  - `404` — payload missing (this is a server inconsistency: metadata says
    `hasData=true` but the file is absent — surface as an error)

### 2.5 `GET /api/time` — server clock

Returns the server's current wall-clock time as a string (ISO-8601 in
practice, but the response body is parsed via `new Date(string)` so any
form `Date` accepts is allowed). The Android client uses this to
gate-keep the connection-test: if the local clock differs from the
server by **more than 5 minutes**, the test fails with a "请同步系统时间"
message — the rest of the protocol relies on `lastModified` timestamps,
so a clock skew breaks sync.

- **Method:** `GET`
- **Path:** `/api/time`
- **Request body:** none
- **Response body:** ISO-8601 timestamp string, e.g.
  `"2026-05-17T16:43:21.420Z"`
- **Status codes:** `200` on success; `401` on auth failure; others error.

### 2.6 `GET /version` — server build version

Lightweight build-id endpoint. Used for the Settings → About screen and
log diagnostics. Treat parse failures as `"Unknown"` rather than an
error — many self-hosted servers in the wild don't implement it.

- **Method:** `GET`
- **Path:** `/version`
- **Request body:** none
- **Response body:** opaque short string, e.g. `"1.5.2"` or
  `"sync-clipboard-server@2026-04"`
- **Status codes:** `200` ok; anything else MUST be treated as
  `"Unknown"` (don't error the call site).

### 2.7 `POST /api/history/query` — list history (paginated)

Returns a page of `HistoryRecord` entries (§3.6). Page-based (1-indexed);
the server determines page size; an **empty array signals end-of-list**.
Filters are passed as a `multipart/form-data` body — string values
serialized as documented.

- **Method:** `POST`
- **Path:** `/api/history/query`
- **Request `Content-Type`:** `multipart/form-data`
- **Request body:** multipart form with any subset of these fields:

| Field | Type | Meaning |
|---|---|---|
| `page` | int as string (e.g. `"1"`) | 1-indexed page. Omit to fetch from start. |
| `before` | ISO-8601 string | Only records with `createTime < before`. |
| `after` | ISO-8601 string | Only records with `createTime >= after`. |
| `modifiedAfter` | ISO-8601 string | Only records with `lastModified > modifiedAfter`. The incremental-sync primitive — clients store the highest `lastModified` they've seen and pass it on the next pull. |
| `types` | int as string (bitmask) | Type filter. `Text=1`, `Image=2`, `File=4`, `Group=8`. Use `15` for "all", `12` for "files + groups". |
| `searchText` | string | Server-side substring match against `text`. |
| `starred` | `"true"` / `"false"` | Filter to starred-only when `true`. |
| `sortByLastAccessed` | `"true"` / `"false"` | When `true`, sort by `lastAccessed` desc; otherwise `createTime` desc. |

- **Response `Content-Type`:** `application/json`
- **Response body:** `HistoryRecord[]` (§3.6). Empty array = no more pages.
- **Status codes:** `200` success; `401`; others error.

#### Pagination contract

```text
page = 1
loop:
    records = POST /api/history/query { page, modifiedAfter? }
    if records.isEmpty: break
    process(records)
    page += 1
```

No `total` or `hasMore` field — the empty-page sentinel is the only
end-of-list signal.

### 2.8 `GET /api/history/<profileId>` — fetch one record

Returns a single record by composite id. Used to dedup before
re-uploading (§2.10) and to follow up on a successful upload.

- **Method:** `GET`
- **Path:** `/api/history/<profileId>` where
  **`<profileId>` is the composite `"<type>-<hash>"`** (e.g.
  `Text-3F4E62D9F184380BAD1B0F94B5518DCBF35ACB79B34F6D6E34F3DAB16CD7BC8F`).
  The `<type>` segment is the literal capitalized type name from §3.3.
- **Response body:** `HistoryRecord` (§3.6)
- **Status codes:**
  - `200` — record exists (may have `isDeleted: true`; callers MUST
    treat soft-deleted as "absent" when deciding whether to re-upload)
  - `401` — auth failure
  - `404` — no record with that id

> **Note on the id form** — §2.8 uses the *composite* id in the URL;
> §2.10 (PATCH) uses a *split form* with type as its own path segment.
> Don't try to unify the two — that's how the wire is.

### 2.9 `POST /api/history` — create / re-upload a record

Creates a new history record with an optional payload file. Multipart
form combining metadata fields with a single optional file field. The
server returns `200` on success or `409` if a record with the same
`<type>-<hash>` already exists.

- **Method:** `POST`
- **Path:** `/api/history`
- **Request `Content-Type`:** `multipart/form-data`
- **Request body:** multipart form. Required fields:
  - `hash` — uppercase hex SHA-256 (§4)
  - `type` — `"Text"` | `"Image"` | `"File"` (Group is unsupported on
    upload — the Flutter client doesn't produce it either, §3.3)

  Optional metadata fields (string-serialized booleans / numbers):

  | Field | Notes |
  |---|---|
  | `text` | Omit entirely when empty/whitespace-only (Android calls `isTextInvalid` before adding the field). |
  | `createTime`, `lastModified`, `lastAccessed` | ISO-8601 strings. Default to "now" if omitted. |
  | `starred`, `pinned`, `hasData`, `isDeleted` | `"true"` / `"false"`. |
  | `size` | Byte length for binary, char count for text. |
  | `version` | Optimistic-lock version. New records start at `0`. |

  Plus optionally one file field whose contents are the §2.3-style
  payload bytes when `hasData=true`. The file field name is the
  multipart implementation's default (the Android client uses
  `nativeUploadMultipart` which writes the file under a default `file`
  field — confirm against your platform's multipart helper).

- **Status codes:**
  - `200` / `201` — created. Caller follows up with `GET §2.8` to
    retrieve the canonical server copy (the upload response body is
    ignored by Android).
  - `401` — auth failure
  - `409` — a record with the same `<type>-<hash>` already exists. The
    response body is the server's current `HistoryRecord` so the client
    can rebase. Map to a `SyncConflictError`-equivalent.

### 2.10 `PATCH /api/history/<type>/<hash>` — update record (star, pin, soft-delete, version bump)

Optimistic-locking partial update. The path uses the **split form** —
`<type>` as a path segment and the bare `<hash>` (without the
`<type>-` prefix) as the next segment.

- **Method:** `PATCH`
- **Path:** `/api/history/<type>/<hash>` — e.g.
  `/api/history/Text/3F4E62D9F184380BAD1B0F94B5518DCBF35ACB79B34F6D6E34F3DAB16CD7BC8F`
- **Request `Content-Type`:** `application/json`
- **Request body:** subset of:

  ```jsonc
  {
    "starred":      true,                                // optional
    "pinned":       false,                               // optional
    "isDelete":     true,                                // optional, soft-delete — NOTE: "isDelete", not "isDeleted"
    "version":      3,                                   // optional, the client-known version
    "lastModified": "2026-05-17T16:43:21.420Z",          // optional
    "lastAccessed": "2026-05-17T16:43:21.420Z"           // optional
  }
  ```

  > ⚠️ **Naming inconsistency to memorize:** the *read* shape (§3.6)
  > and the *create* form (§2.9) use **`isDeleted`** (past participle).
  > The *update* JSON uses **`isDelete`** (verb form, no `d`). This
  > looks like a typo in the server contract but is load-bearing —
  > sending `isDeleted` here is silently ignored.

- **Response body:** the server's updated `HistoryRecord` (§3.6),
  including the bumped `version` and refreshed timestamps.
- **Status codes:**
  - `200` — applied
  - `401` — auth failure
  - `404` — no record at that id (map to "record not found")
  - `409` — version conflict. Body is the server's current record;
    client SHOULD reload, re-apply the update on top of the new
    version, and retry.

### 2.11 `GET /api/history/<profileId>/data` — download record payload

Streams the raw bytes for a history record's attached payload. Same
semantics as §2.4 but bound to a specific historical record (not the
"current" pointer).

- **Method:** `GET`
- **Path:** `/api/history/<profileId>/data` where `<profileId>` is the
  composite `"<type>-<hash>"` (same form as §2.8 — NOT §2.10's split).
- **Response body:** raw bytes
- **Status codes:** `200` / `401` / `404`. Mirrors §2.4.

### 2.12 `GET /api/history/statistics` — usage counters

Optional dashboard endpoint. Returns counts/totals for the entire
history store under the authenticated user.

- **Method:** `GET`
- **Path:** `/api/history/statistics`
- **Response `Content-Type`:** `application/json`
- **Response body:**

  ```jsonc
  {
    "totalCount":      1024,    // includes soft-deleted
    "starredCount":    42,
    "deletedCount":    18,      // count of records with isDeleted=true
    "activeCount":     1006,    // totalCount - deletedCount
    "totalFileSizeMB": 3.7
  }
  ```

- **Status codes:** `200` success; `401`; others error.

### 2.13 Endpoint summary

| # | Method | Path | Notes |
|---|---|---|---|
| 2.1 | GET    | `SyncClipboard.json` | current clipboard metadata |
| 2.2 | PUT    | `SyncClipboard.json` | publish current clipboard |
| 2.3 | PUT    | `file/<name>` | upload payload bytes |
| 2.4 | GET    | `file/<name>` | download payload bytes |
| 2.5 | GET    | `/api/time` | server clock |
| 2.6 | GET    | `/version` | server build version |
| 2.7 | POST   | `/api/history/query` | paginated history list (multipart filters) |
| 2.8 | GET    | `/api/history/<type>-<hash>` | one record (composite id) |
| 2.9 | POST   | `/api/history` | create/re-upload record (multipart) |
| 2.10 | PATCH  | `/api/history/<type>/<hash>` | partial update (split id) |
| 2.11 | GET    | `/api/history/<type>-<hash>/data` | record's payload bytes |
| 2.12 | GET    | `/api/history/statistics` | counters |

---

## 3. Clipboard JSON schema

The Clipboard JSON is the heart of the protocol. It describes exactly one
clipboard "snapshot": its kind, an optional content hash for de-duplication, a
short preview text, and an optional payload file.

### 3.1 Schema

```jsonc
{
  "type":     "Text" | "Image" | "File" | "Group",   // required
  "hash":     "SHA256-UPPER-HEX-STRING" | null,      // optional, see §4
  "text":     "string",                              // required, see §3.2
  "hasData":  true | false,                          // required
  "dataName": "string",                              // present iff hasData=true
  "size":     1234                                   // optional, see §3.2
}
```

#### JSON encoding rules

- `null` for `hash`, `dataName`, or `size` fields MUST be **omitted entirely**
  rather than serialized as `null`. (Flutter uses `includeIfNull: false`.)
- `hash`: when non-null, MUST be uppercase hexadecimal SHA-256. When parsing,
  empty string and pure whitespace MUST be normalized to `null`.
- All string fields are UTF-8.

### 3.2 Field semantics

| Field | Meaning |
|---|---|
| `type` | One of the four enum values from §3.3. Determines how the payload (if any) is to be interpreted. |
| `hash` | Content fingerprint used by all clients to detect "same content as last time" and skip re-applying. Computed per §4. May be omitted/`null` if the publisher chose not to compute one (e.g. legacy data). |
| `text` | For `Text` type: the clipboard text (or a preview prefix if too long, see §3.4). For `Image`/`File`/`Group`: a human-readable label, typically `basename(dataName)`. NEVER the full payload for non-text types. |
| `hasData` | `true` ⇔ a companion file exists at `file/<dataName>`. `false` ⇔ everything is in `text`. |
| `dataName` | Required when `hasData=true`. The exact filename to use under `file/<…>`. MUST NOT contain `/` or `\`. |
| `size` | Optional descriptive size. For `Text`: the **character count** of the original text (not byte length). For `Image`/`File`: the **byte length** of the payload. For `Group`: byte length of the ZIP. May be omitted if unknown. |

### 3.3 `type` enum

The type is serialized as a capitalized string:

| Wire value | Semantics |
|---|---|
| `"Text"` | Plain UTF-8 text. Payload (if `hasData=true`) is the full text as UTF-8 bytes; `text` then holds a preview prefix only (§3.4). |
| `"Image"` | Single image. `dataName` carries the filename with extension (`png`, `jpg`, `gif`, `webp`, `bmp`, `tiff`, `heic`, `heif`). |
| `"File"` | Single arbitrary file. `dataName` carries the original filename. |
| `"Group"` | Multiple files packaged as a ZIP archive. `dataName` is the ZIP filename. The current Flutter client only **reads** Group entries (downloads + verifies hash); it does not currently produce them. |

### 3.4 Long-text overflow rule

Implementations MUST apply the following rule when uploading text:

```
threshold = 10240    // characters (NOT bytes)

if text.length > threshold:
    preview     = text[0:threshold]              // first 10240 chars
    payloadName = "text_<HASH>.txt"              // <HASH> = computeTextHash(text), §4.1
    payloadBody = utf8(text)                     // FULL text, UTF-8 encoded
    => upload PUT file/<payloadName>             // §2.3
    => publish: { type: "Text", hash: <HASH>, text: preview, hasData: true,
                  dataName: payloadName, size: text.length }
else:
    => publish: { type: "Text", hash: <HASH>, text: text, hasData: false,
                  size: text.length }
```

When downloading, an implementation that sees `type=Text`, `hasData=true` SHOULD
fetch `file/<dataName>` and decode as UTF-8 to recover the full text. The
`text` field alone is only a preview in that case.

### 3.5 Upload sequencing

For an entry with `hasData=true`:

1. `PUT file/<dataName>` with the payload bytes.
2. `PUT SyncClipboard.json` with the metadata.

If step 1 fails, step 2 MUST NOT be attempted. There is no transaction; an
implementation that crashes between (1) and (2) leaves an orphan file on the
server, which is harmless (it will be overwritten on next publish).

### 3.6 `HistoryRecord` JSON schema

The shape returned by §2.7 / §2.8 / §2.10 and accepted (in multipart
form, not JSON) by §2.9. Like `Clipboard`, missing/unknown fields MUST
be tolerated.

```jsonc
{
  "hash":         "SHA256-UPPER-HEX",        // required, §4
  "type":         "Text" | "Image" | "File", // required (no "Group" in practice)
  "text":         "string",                  // optional preview; absent or empty when content is purely binary
  "hasData":      true | false,              // true ⇔ a payload file is downloadable via §2.11
  "size":         1234,                      // optional, same semantics as Clipboard.size (§3.2)
  "createTime":   "2026-05-17T16:43:00.000Z",// optional, ISO-8601 (server may default to "now")
  "lastModified": "2026-05-17T16:43:21.420Z",// optional, used for incremental sync (§2.7 modifiedAfter)
  "lastAccessed": "2026-05-17T16:43:21.420Z",// optional, when this record was last surfaced/applied
  "starred":      false,                     // optional, user-marked favorite
  "pinned":       false,                     // optional, user-pinned (sticks to the top)
  "version":      3,                         // optional, server-side optimistic-lock version
  "isDeleted":    false                      // optional, soft-delete tombstone (treat as absent for sync)
}
```

The composite **`profileId`** used in URL paths (§2.8, §2.11) is
`"<type>-<hash>"`. Despite the name `Hash`, the composite id IS what
addresses a record on the server.

#### Lifecycle and version

- A record is created via §2.9 with `version: 0`.
- Every successful §2.10 PATCH increments the server's stored `version`
  by 1 and refreshes `lastModified`. The client MUST send the version
  it observed (its rebase point) — the server rejects a stale version
  with `409` and includes the current record in the response body.
- Soft delete is a §2.10 PATCH with `"isDelete": true` (the
  no-`d` variant — see §2.10). A soft-deleted record is still returned
  by §2.7 / §2.8 with `isDeleted: true`; clients SHOULD treat it as
  absent when deciding whether to re-upload via §2.9 (Android's
  reference: `SyncClipboardClient.putContent` re-uploads if the existing
  record has `isDeleted=true`).

#### Composite-id field-name discipline

| Context | Field name |
|---|---|
| Read response (§2.7 / §2.8 / §2.10 reply) | `isDeleted` |
| §2.9 multipart upload | `isDeleted` |
| §2.10 PATCH JSON body | `isDelete` (no trailing `d`) |

This asymmetry is not a documentation typo — sending the wrong key on a
PATCH is silently ignored, which is how UniClipboard-server contracts
the operation. Wrap it in a helper so call sites can't get it wrong.

---

## 4. Hash computation

All hashes are **SHA-256, lowercase hex transformed to uppercase** (i.e. the
canonical hex digest, then `.toUpperCase()`).

### 4.1 Text hash

```
computeTextHash(text: string) -> string:
    return SHA256(utf8(text)).hex.upper
```

### 4.2 File / image hash

Raw SHA-256 over the payload bytes — the same algorithm as §4.1, applied to
the file's bytes instead of UTF-8 text. The filename does **not** participate;
renaming a file without changing its content keeps the same hash:

```
computeFileHash(bytes: bytes) -> string:
    return SHA256(bytes).hex.upper
```

> Historical note: an earlier revision of this spec described a
> basename-bound two-step hash (`SHA256(basename + "|" + SHA256(bytes))`).
> That never matched what SyncClipboard servers actually compute — real
> servers hash raw bytes — and interop testing surfaced the mismatch as
> spurious §4.4 verification failures. Per this spec's own rule, the spec
> was corrected to match reality.

### 4.3 Group (ZIP) hash

Used to verify a downloaded `Group` ZIP against the server-supplied hash. The
hash is computed over a deterministic textual manifest of the archive, so
implementations MUST agree on byte-perfect output.

#### Algorithm

1. Walk every entry in the archive. Normalize each entry name by replacing
   every `\` with `/`.
2. Maintain a set `entries` of normalized names.
3. For each entry:
   - **File entry:** add the normalized name to `entries`. Remember the bytes.
   - **Directory entry:** ensure the name ends with `/`; add to `entries`.
4. For every entry name added in (3), also synthesize its parent directory
   names (each trailing-slash form, e.g. `a/b/` from `a/b/c.txt`) and add them
   to `entries`.
5. Sort `entries` by **UTF-8 byte order** (not by Unicode code point — strings
   must be encoded to UTF-8 first and compared byte by byte; the Flutter
   implementation calls this `_compareByUtf8Bytes`).
6. Build a manifest string by iterating sorted entries and appending one record
   per entry, terminated by a single NUL byte (`\x00`):
   - Directory: `D|<name>` where `<name>` ends in `/`
   - File:     `F|<name>|<sizeBytes>|<contentHashUpper>` where `<contentHashUpper> = SHA256(bytes).hex.upper`
7. Final hash = `SHA256(utf8(manifest)).hex.upper`.

#### Pseudocode

```
def computeGroupHashFromArchive(archive):
    names = set()
    files = {}
    for entry in archive:
        name = entry.name.replace("\\", "/")
        if entry.is_file:
            names.add(name); files[name] = entry
            add_parents(names, name)
        else:
            if not name.endswith("/"): name += "/"
            names.add(name)
            add_parents(names, name)

    sorted_names = sorted(names, key=lambda s: s.encode("utf-8"))   # byte-wise

    buf = bytearray()
    for name in sorted_names:
        if name.endswith("/"):
            buf += f"D|{name}".encode("utf-8")
        else:
            entry = files[name]
            content_hash = sha256(entry.bytes).hexdigest().upper()
            buf += f"F|{name}|{len(entry.bytes)}|{content_hash}".encode("utf-8")
        buf += b"\x00"
    return sha256(buf).hexdigest().upper()
```

`add_parents(names, name)` for `a/b/c.txt` adds `a/`, `a/b/`. For `a/b/`
it adds `a/`. Empty path components are skipped.

### 4.4 Hash comparison

When an implementation needs to compare two hashes (e.g. local content vs
server-supplied):

```
hashMatches(expected, actual):
    if expected is null or whitespace-only: return true   // no expectation
    return expected.trim().toUpperCase() == actual.toUpperCase()
```

A `null`/empty `expected` MUST be treated as "match anything" — this is what
allows publishers to omit hashes for data they cannot fingerprint.

---

## 5. Multi-server configuration & auto-switch (client-only)

This section is **not** part of the wire protocol — it describes data the
client persists locally to manage multiple servers and automatic profile
switching. iOS implementations should be able to import/export a config file
following the same JSON shape so users can migrate between platforms.

### 5.1 `ServerConfig`

```jsonc
{
  "id":       "uuid-v4-string",                  // required
  "name":     "string | null",                   // optional, user-chosen label
  "url":      "https://...",                     // required; == urls[0]; raw (not normalized)
  "urls":     ["https://...", "http://..."],     // ordered candidate base URLs; default: [url]
  "username": "string",                          // required
  "password": "string"                           // required
}
```

- `id`: stable UUID v4. Two configs with the same `id` are the same config.
- `name`: human-readable label. If null/empty, the UI falls back to `url`.
- A `ServerConfig` is **one logical server identity** (one credential pair, one
  device id from the pairing QR) reachable at one or more candidate base URLs.
- `urls`: the **ordered candidate base URLs**, each a complete base URL (the
  publisher omits the trailing slash; readers MUST tolerate one). The network
  decides which candidate is used *right now* (§5.3). Never empty for a valid
  config.
- `url`: the canonical base URL, kept **identical to `urls[0]`**. It exists so a
  reader that only knows the old single-URL shape still works. On encode the
  client writes **both** keys.

**Migration.** When persisted data (or a pairing payload) carries only the
legacy single `url` and no `urls`, the client fills `urls = [url]`. Pre-this-spec
data also carried per-config auto-switch keys (`autoSwitchStrategy`,
`autoSwitchWifiNames`): these are **decoded-and-dropped** — tolerated on read,
never re-encoded. Auto-switch no longer selects *between* profiles; it selects
between a single profile's `urls` (§5.3).

#### URL classification (host shape)

A candidate URL is classified by the kind of network path its **host** reaches,
from the host alone — no DNS resolution, no probing:

| Class | Host matches |
|---|---|
| `tailscale` | IPv4 in `100.64.0.0/10` (Tailscale CGNAT), or a `*.ts.net` MagicDNS name |
| `lan` | IPv4 in `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`, `169.254.0.0/16`, or a `*.local` mDNS name |
| `wan` | anything else (a public IP or any other hostname) |

#### SSID normalization rule

SSID *names* are no longer matched for auto-switch (that is URL-shape based
now), but a normalized SSID still flows cross-process as a "which Wi-Fi am I on"
signal. Both stored SSIDs and the system-reported current SSID MUST be
normalized identically:

1. Trim whitespace.
2. If empty after trim → treat as "no SSID".
3. If wrapped in matching ASCII double quotes (`"foo"`), strip them and trim
   the inner string. (Some platforms — notably Android — return SSIDs wrapped
   in quotes.)
4. Reject the magic strings `<unknown ssid>` and `0x` (Android privacy
   placeholders): treat as "no SSID".

### 5.2 `ServerConfigList`

```jsonc
{
  "configs":         [ ServerConfig, ... ],   // default: []
  "activeConfigId":  "uuid-v4-string | null"
}
```

If `activeConfigId` is set but no config in `configs` has that id, the client
falls back to `configs[0]`. If `configs` is empty, there is no active config
and the client MUST refuse to make network calls. `activeConfigId` is the
**single source of "which server am I using"** — there is no separate
override/pin concept.

**Migration note.** Pre-unification iOS builds persisted a home-screen "pin"
in a client-private `manualOverrideConfigId` key that out-prioritized
`activeConfigId`. That key was never part of this spec. On read, a client MUST
promote a resolvable `manualOverrideConfigId` into `activeConfigId` (the user's
last explicit pick becomes the current server) and MUST NOT re-encode the key.
An absent or unresolvable value is ignored.

### 5.3 Network-based URL auto-switch (effective URL within the active profile)

Which **profile** is active is purely the user's manual pick (`activeConfig`,
§5.2) — it does **not** change with the network. What the network changes is
which of that profile's **`urls`** is used right now. So a user with one server
reachable over LAN, Tailscale, and a public relay configures one profile with
three `urls`, and the device picks the fastest reachable path automatically.

Resolution has two layers:

**Layer 1 — shape ordering (pure, no I/O).** Both the main app and the keyboard
extension compute a *try-order* over the active profile's `urls` from the
current network. This never does I/O and never rewrites persisted state.

The device's current network is captured as a `NetworkContext`:

```
NetworkContext = {
  ssid:        string | null   # normalized §5.1 SSID; null when not on a named Wi-Fi
  isWifi:      bool            # primary path uses Wi-Fi (OS network path)
  isCellular:  bool            # primary path is cellular data
  isTailscale: bool            # a Tailscale virtual network is up (see below)
}
```

`isWifi` / `isCellular` / interface type come from the OS network path, and
`isTailscale` from enumerating local interfaces — none needs any permission. The
SSID *name* needs the Wi-Fi-info entitlement + Location; a client that can't read
it (e.g. the keyboard extension) supplies `ssid = null`. SSID name is no longer
matched — it only acts as a fallback "on Wi-Fi" signal when `isWifi` is unset.

**Tailscale detection.** `isTailscale` is true iff a local interface holds an
IPv4 address in Tailscale's CGNAT range **100.64.0.0/10** (via `getifaddrs`).
This pins it to Tailscale rather than "some VPN", and works from any process.

```
def orderedURLs(cfg, network):                        # stable reorder of cfg.urls
    pref = classPreference(network)                   # or None → keep publisher order
    if pref is None: return cfg.urls
    return stable_sort(cfg.urls, key=lambda u: index_or_end(pref, classify(u)))

def classPreference(network):                         # most-preferred class first
    onWifi = network.isWifi or network.ssid is not None
    if onWifi:              return ["lan", "tailscale", "wan"]   # direct LAN is lowest-latency
    if network.isTailscale: return ["tailscale", "wan", "lan"]  # off-Wi-Fi, TS up (e.g. cellular)
    if network.isCellular:  return ["wan", "tailscale", "lan"]  # LAN unreachable; TS may tunnel
    return None                                                  # no signal → publisher order

def effectiveActiveConfig(list, network):
    cfg = getActiveConfig(list)                       # §5.2 — unchanged by the network
    return cfg.with(urls = orderedURLs(cfg, network)) if cfg else None
```

`orderedURLs` is a **stable** sort: a more-preferred class moves ahead, but
within one class (and when the network gives no signal) the publisher's order is
preserved. Reachability is **not** consulted here — this only decides the
*try-order*. The keyboard extension uses `urls[0]` of the result as its best
guess (no probing, no entitlement).

**Layer 2 — reachability probing (main app only).** The shape order is a guess
(e.g. a LAN URL ranks first on Wi-Fi even on a foreign network where it is
unreachable). The main app probes the ordered candidates **concurrently** and
adopts the first *in shape order* that is reachable as the profile's **live
URL** (deterministic given the probe results — NOT a connection race; two
reachable candidates resolve to whichever ranks earlier). All-unreachable
clears the live URL, and readers fall back to pure shape order.

Probe semantics:

- One `GET /SyncClipboard.json` per candidate with a **short timeout** (~2 s
  — a candidate that can't work on this network must fail fast), no retry,
  no body decode.
- **`404` counts as reachable** (§2.1: server up, clipboard empty).
- **`401` counts as reachable** — wrong credentials are an account problem,
  not a path problem; the sync engine surfaces auth failures on its own and
  the picker must not skip a working direct path because the password is
  stale.

Probes run on: network change, profile switch, app foreground, a
network-class sync failure (debounced — the 1 Hz tick retries failures every
second), and the user's explicit "test connection" (which SHOULD probe all
candidates and present per-URL reachability). In steady state the 1 Hz sync
tick *is* the probe of the live URL; no background probing beyond the
triggers above.

The effective try-order layers the verdict over the shape order: the live URL
leads, remaining candidates follow in shape order as fallbacks
(`preferredURLs`). A persisted live URL that is no longer in the profile's
`urls` (the config was edited since the probe) MUST be ignored, not
resurrected. Switching the live URL *within* a profile MUST NOT reset
per-server sync state (watermarks, last-synced hash) — it is the same server
and the same content timeline.

The live URL is cached cross-process per profile (`{configId: url}`; this
client: a JSON file in the App Group container, written atomically by the
main app only) so the keyboard extension reads the app's confirmed choice
instead of re-probing; absent a cached value it falls back to Layer 1's
`urls[0]`.

**Verdict invalidation — network epochs.** A probe verdict is only valid for
the network it was measured on. The main app keeps a monotonic **network
epoch** counter, advanced on every network-context change (Wi-Fi ↔ cellular,
SSID change, VPN up/down), and the moment the epoch advances:

- The **in-memory** live URL is dropped immediately — readers fall back to
  pure shape order, which is already the right guess for the new network —
  rather than letting a verdict from the old network lead `preferredURLs`
  until the next probe lands. (The *persisted* per-profile value is left
  alone; it is only read at cold launch / profile switch as a first guess
  and the next probe overwrites it.)
- Any **in-flight sync request** was built against a URL chosen for the old
  network and is cancelled outright; a cancelled request is a deliberate
  abort, NOT a sync failure — no backoff, no error surfaced.
- The sync engine's **failure backoff is cleared** — failures of the old
  path say nothing about the new one.
- Probes are **epoch-stamped at start**: a verdict landing after the epoch
  (or the active profile) moved on describes the wrong network and is
  discarded wholesale — no cache write, no debounce-clock touch — and a
  fresh probe is started for the new epoch. The probe debounce (the 1 Hz
  sync tick re-kicks a probe on every network-class failure) is scoped to
  one epoch: the first probe after a network change is never suppressed.

Conversely, a probe verdict that **flips the live URL to a reachable
candidate is a recovery signal**: the engine cancels whatever is still
talking to the old URL, clears its backoff, and attempts a sync immediately
instead of waiting out the retry window accumulated against the dead path.

A client MAY surface which URL is in use (e.g. a "direct / relay" badge) so the
user understands why latency changed without a manual pick.

### 5.4 `AppSettings`

```jsonc
{
  "trustInsecureCert":        false,    // disable TLS cert validation
  "autoCheckUpdate":          true,     // check for app update on launch
  "manualUploadDialogShown":  false,    // user dismissed first-run hint
  "downloadRelativePath":     "",       // subdirectory under the platform Downloads dir
  "logViewLevelFilter":       "info",   // last-used log level filter
  "ignoredVersion":           null,     // version string the user chose to skip
  "autoApplyServerChanges":   true,     // write new server content to the device clipboard automatically
  "autoPushDeviceChanges":    false     // read + push new local clipboard content automatically
}
```

- All fields have defaults (shown). A missing key MUST be filled with its
  default; the file MUST still parse if extra unknown keys are present
  (forward compatibility).
- `autoPushDeviceChanges` defaults **false** on iOS: reading `UIPasteboard`
  content fires the system "Allow Paste" prompt, so device→server push is
  consent-based by default (a `PasteButton` the user taps). Setting it true
  opts into fully-automatic push and the recurring prompt that entails. A
  client MAY persist additional UI/cache keys (appearance, prefetch policy,
  payload cache size, keyboard-extension feedback toggles `keyboardSoundFeedback`
  / `keyboardHapticFeedback`); they follow the same default-and-tolerate rule.

### 5.5 Persistence keys

The Flutter client uses Android `SharedPreferences` (a `String → String` KV
store). For an iOS port using `UserDefaults`, the same keys SHOULD be reused so
that an export-from-Android / import-to-iOS round trip is straightforward.

| Key | Value (UTF-8 string) |
|---|---|
| `server_config_list` | `serverConfigListToJson(...)` (§5.2) |
| `app_settings` | `appSettingsToJson(...)` (§5.4) |
| `server_config` | **legacy**, single-config format. If `server_config_list` is absent and `server_config` is present, the client MUST migrate: wrap the legacy single config into a `ServerConfigList` (allocate a fresh UUID, set it as active), write the new key, delete the old key. |

Legacy `server_config` shape (read-only, write-once-on-migrate):

```jsonc
{ "url": "...", "username": "...", "password": "..." }
```

### 5.6 QR-code import payload (UniClipboard extension)

UniClipboard supports importing a server configuration by scanning a QR
code from the **Settings → Servers → Add → "从二维码填充"** flow. This is
a UniClipboard-only convention; upstream SyncClipboard has no QR format
and other clients are not required to interoperate with it.

A scanned barcode is decoded as a UTF-8 string and dispatched, in
priority order:

1. **JSON object** (preferred — can carry an alias)
2. **URL with userinfo** (fallback — fits any QR generator)

Anything else fails parsing and surfaces an inline alert; the scanner
keeps running so the user can try another code.

#### Format 1: JSON object

```jsonc
{
  "url":      "https://clip.home.lan:5033/",   // required, non-empty
  "username": "alice",                          // required, non-empty
  "password": "p4ssw0rd!",                      // required, non-empty
  "name":     "Home NAS"                        // optional
}
```

Rules:

- `url`, `username`, `password` are required and MUST be non-empty
  strings. A payload missing any of them, or with empty-string values,
  is rejected.
- `name` is optional. When absent, the consumer falls back to a
  generated alias (see [`ServerNameGenerator`](../UniClipboard/Models/ServerNameGenerator.swift)).
- Unknown keys are tolerated and ignored. A future revision can add
  fields (e.g. an explicit `trustInsecureCert` hint) without breaking
  older scanners.
- A `type` discriminator field is NOT required. If a future version
  introduces one, scanners SHOULD still accept payloads without it.

#### Format 2: URL with userinfo

```
https://alice:p4ssw0rd!@clip.home.lan:5033/
```

Rules:

- The URL MUST parse as a valid `URL` and carry both a `user` and a
  `password` component. Missing either component rejects the payload.
- The reconstructed `url` field (passed to the Add form) is the URL
  **without** the userinfo segment — i.e. `https://clip.home.lan:5033/`.
- `name` is always nil for this format (URL syntax has no slot for it).
- Special characters in user/password MUST be percent-encoded by the
  generator. The scanner percent-decodes both before assigning to the
  draft.

#### Receiver behavior

After a successful parse, the scanner:

1. Dismisses the camera cover.
2. Opens the Add Server sheet pre-filled with the payload's fields.
3. Leaves `trustInsecureCert` at the app-wide default — the QR format
   does not carry it deliberately, so the user can confirm.
4. Leaves the autoSwitch SSID list empty — auto-switch is per-device
   network state, not a portable identity, and shouldn't be cloned.

The user can edit any field before tapping **保存**, and **测试连接**
runs against the in-form draft using the shared `ConnectionTester`.

#### Worked examples

See [`examples/server_qr_payload.json`](./examples/server_qr_payload.json)
and [`examples/server_qr_payload_url.txt`](./examples/server_qr_payload_url.txt).

---

## 6. Error model

Implementations SHOULD surface errors with at minimum:

- a `statusCode` (when the error is HTTP-related, otherwise null),
- a localized human message,
- the underlying exception for debugging/log.

Recommended message mapping (mirrors `_handleDioException` in
`lib/dio/sync_clipboard_client.dart`):

| Condition | Message (suggested) |
|---|---|
| Connect timeout | "Connection timed out — check the server URL" |
| Receive timeout | "Receive timed out" |
| TLS handshake / connection refused / DNS failure | "Cannot reach server — check network and URL" |
| HTTP 401 | "Authentication failed — username or password wrong" |
| HTTP 404 on `SyncClipboard.json` | "No clipboard published yet" |
| HTTP 404 on `file/<name>` | "Payload `<name>` not found on server" |
| Other `4xx` | "Server returned HTTP `<code>`" |
| Other `5xx` | "Server error `<code>`" |
| HTTP 404 on `/api/history/<id>` | "History record not found" — map to `RecordNotFound` so callers can distinguish "exists but soft-deleted" from "never existed" |
| HTTP 409 on `POST /api/history` or `PATCH /api/history/...` | "History record version conflict" — surface as `SyncConflict` carrying the server's current record (response body) so the caller can rebase |
| Time skew detected during connection test (§2.5) | "服务器与本地时间差距过大，请同步系统时间" — block the test before any further API call |
| Request cancelled by the client (§5.3 network-epoch invalidation) | Not user-facing from the sync engine — a deliberate abort, swallowed silently (no backoff, no error state). UI surfaces triggered by a user action MAY show "请求已取消". |

Authentication failures (`401`) MUST NOT trigger automatic retries.
Version conflicts (`409`) on PATCH SHOULD trigger an automatic
read-modify-retry (one round); on POST they indicate "already exists"
and the client SHOULD treat that as a successful upload of an
idempotent record.

---

## 7. Client implementation checklist (SwiftUI port)

Use this list when porting to iOS. Each item maps directly to a section above.

- [ ] HTTP client with Basic Auth interceptor on every request (§1.2)
- [ ] Per-request `Content-Type` discipline: `application/json` for the JSON
      endpoint, `application/octet-stream` for file uploads (§2.2, §2.3)
- [ ] Trust-insecure-cert toggle wired to `URLSessionDelegate`
      `urlSession(_:didReceive:completionHandler:)` (§1)
- [ ] Base URL normalization (trailing slash) (§1.1)
- [ ] `Codable` types for `Clipboard`, `ServerConfig`, `ServerConfigList`,
      `AppSettings` matching §3.1, §5.1, §5.2, §5.4
- [ ] `JSONEncoder` configured to **omit `nil`** for `hash`, `dataName`, `size`
      (Swift default already does this for optionals; just don't set
      `nilEncodingStrategy = .null`) (§3.1)
- [ ] `type` enum encoded as `"Text" | "Image" | "File" | "Group"` (§3.3) —
      use raw-value enum
- [ ] `CryptoKit.SHA256` for all hashes; manual `.uppercased()` (§4)
- [ ] Long-text threshold = `10240` **characters**, not bytes (§3.4) — be
      careful with `String.count` vs UTF-8 byte length
- [ ] Group hash: walk archive, synthesize parent dirs, sort by UTF-8 bytes
      (§4.3). Use ZIPFoundation; iterate `Archive` entries; encode manifest as
      `Data`; remember `\x00` between records.
- [ ] SSID normalization parity (strip outer `"…"`, reject `<unknown ssid>`
      and `0x`) (§5.1) — required for cross-platform config import to behave
      identically.
- [ ] WiFi auto-switch resolver (§5.3) using `NEHotspotNetwork.fetchCurrent`
      (requires Access WiFi Information entitlement). When the entitlement is
      not granted, treat current SSID as `nil` — this is the documented
      degenerate path and just falls back to the active config.
- [ ] Persistence under `UserDefaults`, keys per §5.5; if you also need to
      share with a Share Extension, use an App Group `UserDefaults(suiteName:)`
      and use the **same keys** inside the suite.
- [ ] Legacy `server_config` migration if you ship an Android→iOS export
      flow (§5.5).
- [ ] History API client (§2.7 – §2.12) — separate from the live-clipboard
      path. Build the multipart-form bodies for §2.7 and §2.9 via
      `URLSession`'s upload task with a hand-rolled `multipart/form-data`
      body (or a tiny helper); the JSON-encoder path won't work.
- [ ] Composite vs split id discipline (§3.6): use composite
      `<type>-<hash>` for §2.8 / §2.11 GETs, split `<type>/<hash>` for
      §2.10 PATCH. Wrap both URL builders so call sites never construct
      paths by hand.
- [ ] `isDelete` vs `isDeleted` discipline (§3.6 / §2.10): one type for
      reads + creates (`isDeleted`), a different field name for PATCH
      bodies (`isDelete`). Encode both in your `Codable` types via
      `CodingKeys` rather than a single property reused across contexts.
- [ ] Incremental history pull loop (§2.7): page-based, 1-indexed,
      `modifiedAfter` for delta; store the highest `lastModified` seen
      across the merged page set as the next watermark; empty page →
      stop.
- [ ] Optimistic-lock retry (§2.10): on `409`, decode the response body
      as the server's `HistoryRecord`, re-apply the local mutation on
      top of the returned `version`, retry once. If the second attempt
      also `409`s, surface as a sync error.
- [ ] Time-skew gate (§2.5): the connection-test SHOULD pull
      `/api/time` and fail the test if `|local - server| > 5 min`.
