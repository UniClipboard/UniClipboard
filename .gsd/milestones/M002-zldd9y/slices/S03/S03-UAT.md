# S03: Storage Stats &amp; Clear Cache HTTP Handlers — UAT

**Milestone:** M002-zldd9y
**Written:** 2026-03-30T02:16:01.708Z

## Preconditions

- Daemon running with valid SQLite DB and spool directory initialized
- Authenticated session token (L2+ endpoint — valid JWT + PID whitelist)
- Clipboard history with at least one entry (for blob_count > 0)

## Smoke Test

```bash
curl -s -H "Authorization: Bearer <TOKEN>" http://localhost:<PORT>/storage/stats | jq .
# Expected: 200 with {data: {totalSizeBytes, blobCount, databaseSizeBytes, cacheSizeBytes, spoolSizeBytes}, ts}
```

## Test Cases

### 1. GET /storage/stats returns all 5 fields

1. Send GET /storage/stats with valid auth token
2. **Expected:** HTTP 200, data object with all 5 fields (totalSizeBytes, blobCount, databaseSizeBytes, cacheSizeBytes, spoolSizeBytes) plus ts

### 2. GET /storage/stats without auth returns 401

1. Send GET /storage/stats with no token or invalid token
2. **Expected:** HTTP 401

### 3. POST /storage/clear-cache missing body returns 400

1. Send POST /storage/clear-cache with no body
2. **Expected:** HTTP 400, {error: {code: "confirmation_required", message: "confirmed field must be set to true"}}

### 4. POST /storage/clear-cache with confirmed:false returns 400

1. Send POST /storage/clear-cache with {"confirmed": false}
2. **Expected:** HTTP 400, same confirmation_required error

### 5. POST /storage/clear-cache with confirmed:true clears cache and returns freed_bytes

1. Send POST /storage/clear-cache with {"confirmed": true}
2. **Expected:** HTTP 200, {data: {freedBytes: <number>}, ts}
3. Repeat — should succeed again (idempotent safe)

## Edge Cases

- **Cache dir absent:** GET /storage/stats returns cacheSizeBytes: 0, not an error
- **Spool dir absent:** GET /storage/stats returns spoolSizeBytes: 0, not an error
- **Empty clipboard:** blobCount should be 0

## Failure Signals

- HTTP 500 on /storage/stats: check logs for "Failed to compute storage stats" or "Failed to list clipboard entries"
- HTTP 500 on /storage/clear-cache: check logs for "Failed to clear cache"

## Not Proven By This UAT

- End-to-end correctness of freed_bytes value (requires pre-populated cache with known size)
- Performance under large spool directories
