# Protocol & Configuration Fixtures

Round-trip test inputs for any new SyncClipboard client implementation.
Each file is a real, parseable JSON document that the existing Flutter
client either produces (when uploading) or accepts (when downloading or
loading from `SharedPreferences`).

Use these files to drive `Codable` (Swift), `serde` (Rust), or equivalent
unit tests on a port. The contract: **decode → re-encode → byte-equivalent
JSON** (modulo whitespace and key ordering — but field presence and
`null`-omission must match exactly).

See [`../SYNC_PROTOCOL.md`](../SYNC_PROTOCOL.md) for the spec these
fixtures conform to. Section numbers below reference that document.

---

## Wire fixtures (`Clipboard` JSON, §3)

| File | Scenario | Notes |
|---|---|---|
| `clipboard_text_short.json` | Short plain text under threshold | `hasData=false`, `dataName`/`size` rules per §3.2. **`hash` is real**: `SHA256-UPPER("Hello, SyncClipboard!")`. `size = 21` (character count). |
| `clipboard_text_long.json` | Long text over the 10240-char threshold | `hasData=true`, `dataName=text_<HASH>.txt`. `size = 23457` is the **character count of the full text**, not byte length. The `text` field shown here is a placeholder — on the real wire it MUST be exactly the first 10240 characters of the original. The hash here is illustrative only (no real text behind it). |
| `clipboard_image.json` | Image payload | `hash` is real and was computed via `SHA256-UPPER("photo_2026.png" + "\|" + "9F86D08…A08")` per §4.2. `size` is the byte length of the payload file. |
| `clipboard_file.json` | Generic file | Same hash construction as the image case. |
| `clipboard_group.json` | ZIP-of-files (`Group`) | Hash is illustrative — a real Group hash MUST be derived from the actual archive entries via §4.3. The Flutter Android client currently only **reads** Group entries; it does not produce them. |
| `clipboard_no_hash.json` | Publisher omitted `hash` | Receivers MUST treat omitted/`null`/empty `hash` as "matches anything" (§4.4). Note also that `hash`, `dataName`, `size` keys are absent (not `null`) — that is the canonical encoding (§3.1). |

### What to assert in your decoder tests

- The `type` enum decodes from the strings `"Text" | "Image" | "File" | "Group"` (§3.3).
- Re-encoding `clipboard_no_hash.json` must NOT introduce `"hash": null`,
  `"dataName": null`, or `"size": null` keys. Optional fields decode to
  `nil` and re-encode to **omitted**.
- `size` is `Int` (or unsigned). The same Swift type is reused for both
  character counts (Text) and byte lengths (Image/File/Group); the
  semantic difference lives in your business logic, not the model.
- `hash`, when present, is uppercase 64-char hex. Trim whitespace, treat
  empty as `nil`.

---

## History fixtures (`HistoryRecord` JSON, §3.6)

| File | Scenario | Notes |
|---|---|---|
| `history_record_text.json` | Fully-populated text record | All optional fields present: timestamps, starred/pinned flags, version, `isDeleted=false`. Hash matches `clipboard_text_short.json` — the same logical content surfaces in both the live-clipboard family and the history family. |
| `history_record_minimal.json` | Only required fields | `hash` + `type`. Receivers MUST tolerate missing optional fields and supply sensible defaults (timestamps treated as unknown; flags `false`; `isDeleted=false`). |
| `history_record_deleted.json` | Soft-deleted tombstone | `isDeleted=true` in the **read** shape (§3.6). The PATCH update body uses `isDelete` (no trailing `d`) — see §2.10. Clients deciding whether to re-upload via §2.9 MUST treat `isDeleted=true` as "absent". |

### What to assert in your decoder tests

- All optional fields decode to `nil`/`false` defaults when absent (forward compat).
- `isDeleted` (read) and `isDelete` (PATCH update body) are **two different keys**.
  A single Swift property `isDeleted` works if PATCH bodies are encoded via a
  separate update DTO with its own `CodingKeys`.
- Composite `profileId = "<type>-<hash>"` is computable from any record.

---

## Persistence fixtures (§5)

| File | Persistence key | Notes |
|---|---|---|
| `server_config_list.json` | `server_config_list` | Three configs covering: (1) named config with a multi-URL `urls` candidate list spanning wan/lan/tailscale shapes, (2) named single-URL config, (3) unnamed single-URL config (no `name` key). Every config carries `url` (== `urls[0]`) **and** `urls`. `activeConfigId` points at config #3. |
| `server_config_legacy.json` | `server_config` (legacy) | Single-config format used before the multi-server feature. If a client finds **only** this key, it MUST migrate to the new format (§5.5): wrap into a `ServerConfigList`, allocate a fresh UUID v4, set as active, write the new key, delete the old key. |
| `app_settings.json` | `app_settings` | Every field set to a non-default value, exercising `ignoredVersion` (a non-null optional). |
| `app_settings_minimal.json` | `app_settings` | Only the required fields. `ignoredVersion` is **omitted** (not `null`) — verify your decoder handles this case and your encoder produces this shape when the value is `nil`. |

## QR-import fixtures (§5.6, UniClipboard extension)

| File | Format | Notes |
|---|---|---|
| `server_qr_payload.json` | JSON object | Canonical UniClipboard QR payload. All required fields (`url`, `username`, `password`) present plus optional `name`. Encode this as the textual content of a QR code to drive the Settings → Servers → Add scan flow. |
| `server_qr_payload_url.txt` | URL with userinfo | Fallback format. The userinfo segment carries credentials; password's `!` is percent-encoded as `%21` per RFC 3986. On scan, the receiver strips the userinfo and stores `url = https://clip.home.lan:5033/`. |

### What to assert in your decoder tests

- `server_config.name` is optional. Missing key, `null`, and empty string
  must all be treated equivalently for display fallback (§5.1).
- `urls` is the source of truth when present and non-empty; when absent (or
  empty) the decoder falls back to `[url]`. On encode, both `url` (== `urls[0]`)
  and `urls` are written. Pre-this-spec per-config auto-switch keys
  (`autoSwitchStrategy`, `autoSwitchWifiNames`) are tolerated on read and dropped.
- `ServerConfigList.activeConfigId` may point at an id that no longer
  exists in `configs`. Your getter for the "active config" must fall
  back to `configs[0]` in that case (and return `nil` if `configs` is
  empty) (§5.2).
- `AppSettings` decoder must fill defaults for any missing key (forward
  compatibility) and tolerate unknown keys (e.g. `_schemaVersion`) so
  that adding fields in the future does not break old clients.

---

## Round-trip recipe (Swift example)

```swift
import XCTest

final class FixturesTests: XCTestCase {
    func loadFixture(_ name: String) throws -> Data {
        let url = Bundle.module.url(
            forResource: name, withExtension: "json"
        )!
        return try Data(contentsOf: url)
    }

    func testClipboardTextShortRoundTrip() throws {
        let data = try loadFixture("clipboard_text_short")
        let decoder = JSONDecoder()
        let entry = try decoder.decode(Clipboard.self, from: data)

        XCTAssertEqual(entry.type, .text)
        XCTAssertEqual(entry.hash,
            "3F4E62D9F184380BAD1B0F94B5518DCBF35ACB79B34F6D6E34F3DAB16CD7BC8F")
        XCTAssertEqual(entry.size, 21)
        XCTAssertFalse(entry.hasData)
        XCTAssertNil(entry.dataName)

        // Re-encode and assert null-omission discipline
        let encoder = JSONEncoder()
        let reEncoded = try encoder.encode(entry)
        let asString = String(data: reEncoded, encoding: .utf8)!
        XCTAssertFalse(asString.contains("null"),
            "optional fields must be omitted, not encoded as null")
    }
}
```

For `clipboard_no_hash.json`, the same test should additionally assert
that `hash`, `dataName`, and `size` are absent from the re-encoded
output (use a regex or parse back into a `[String: Any]` dictionary
and check `keys`).

---

## Hash verification

You can independently re-derive the real hashes shown above:

```sh
# Text hash for clipboard_text_short.json
printf 'Hello, SyncClipboard!' | shasum -a 256 | awk '{print toupper($1)}'
# => 3F4E62D9F184380BAD1B0F94B5518DCBF35ACB79B34F6D6E34F3DAB16CD7BC8F

# File hash for clipboard_image.json
# Step 1: pretend the image bytes hash to 9F86…A08
# Step 2: combine with basename
printf 'photo_2026.png|9F86D081884C7D659A2FEAA0C55AD015A3BF4F1B2B0B822CD15D6C15B0F00A08' \
  | shasum -a 256 | awk '{print toupper($1)}'
# => 4DD7CC4227AA3FB2FDAC2597CB4F88EAC6F69A10BC1994F6B87CF8890C345AFC
```

The Group hash in `clipboard_group.json` is illustrative — derive yours
from a real archive following §4.3.
