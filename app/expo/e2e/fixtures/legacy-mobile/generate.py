#!/usr/bin/env python3
"""Generate deterministic UniClip v2.4.1.204 (schema v5) legacy mobile SQLite fixtures."""

import argparse
import base64
import hashlib
import json
import shutil
import sqlite3
import struct
import time
import zlib
from pathlib import Path

# Valid 4x4 baseline JPEG bytes (self-contained, no platform imaging tools required).
SAMPLE_JPEG_BYTES = base64.b64decode(
    "/9j/4AAQSkZJRgABAQAASABIAAD/4QBMRXhpZgAATU0AKgAAAAgAAYdpAAQAAAABAAAAGgAAAAAAA6ABAAMAAAABAAEAAKACAAQAAAAB"
    "AAAABKADAAQAAAABAAAABAAAAAD/7QA4UGhvdG9zaG9wIDMuMAA4QklNBAQAAAAAAAA4QklNBCUAAAAAABDUHYzZjwCyBOmACZjs+EJ+"
    "/8AAEQgABAAEAwEiAAIRAQMRAf/EAB8AAAEFAQEBAQEBAAAAAAAAAAABAgMEBQYHCAkKC//EALUQAAIBAwMCBAMFBQQEAAABfQECAwAE"
    "EQUSITFBBhNRYQcicRQygZGhCCNCscEVUtHwJDNicoIJChYXGBkaJSYnKCkqNDU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0"
    "dXZ3eHl6g4SFhoeIiYqSk5SVlpeYmZqio6Slpqeoqaqys7S1tre4ubrCw8TFxsfIycrS09TV1tfY2drh4uPk5ebn6Onq8fLz9PX29/j5"
    "+v/EAB8BAAMBAQEBAQEBAQEAAAAAAAABAgMEBQYHCAkKC//EALURAAIBAgQEAwQHBQQEAAECdwABAgMRBAUhMQYSQVEHYXETIjKBCBRC"
    "kaGxwQkjM1LwFWJy0QoWJDThJfEXGBkaJicoKSo1Njc4OTpDREVGR0hJSlNUVVZXWFlaY2RlZmdoaWpzdHV2d3h5eoKDhIWGh4iJipKT"
    "lJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uLj5OXm5+jp6vLz9PX29/j5+v/bAEMAAgICAgICAwICAwUD"
    "AwMFBgUFBQUGCAYGBgYGCAoICAgICAgKCgoKCgoKCgwMDAwMDA4ODg4ODw8PDw8PDw8PD//bAEMBAgICBAQEBwQEBxALCQsQEBAQEBAQ"
    "EBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEP/dAAQAAf/aAAwDAQACEQMRAD8A+kKKKK/yPP8AQQ//2Q=="
)

PINNED_FIXTURE_TEXT = "Legacy mobile migration fixture — pinned"

SCHEMA_V5 = """
PRAGMA journal_mode = WAL;
CREATE TABLE clipboard_history (
  profileHash        TEXT PRIMARY KEY COLLATE NOCASE,
  type               TEXT NOT NULL,
  text               TEXT NOT NULL DEFAULT '',
  displayKind        TEXT,
  dataName           TEXT,
  size               INTEGER,
  fileUri            TEXT,
  hasData            INTEGER NOT NULL DEFAULT 0,
  hasRemoteData      INTEGER NOT NULL DEFAULT 0,
  localClipboardHash TEXT,
  timestamp          INTEGER NOT NULL DEFAULT 0,
  lastAccessed       INTEGER NOT NULL DEFAULT 0,
  lastModified       INTEGER NOT NULL DEFAULT 0,
  useCount           INTEGER NOT NULL DEFAULT 0,
  starred            INTEGER NOT NULL DEFAULT 0,
  pinned             INTEGER NOT NULL DEFAULT 0,
  isDeleted          INTEGER NOT NULL DEFAULT 0,
  isLocalFileReady   INTEGER NOT NULL DEFAULT 0,
  syncStatus         INTEGER NOT NULL DEFAULT 0,
  version            INTEGER NOT NULL DEFAULT 0,
  incarnationId      TEXT,
  "from"             TEXT,
  deviceName         TEXT,
  synced             INTEGER,
  contentId          TEXT,
  p2pEntryId         TEXT,
  p2pDeliveryState   TEXT,
  p2pDeliveryCounts  TEXT
);
PRAGMA user_version = 5;
"""


def make_png(width: int, height: int, rgba: tuple[int, int, int, int]) -> bytes:
    def chunk(ctype: bytes, data: bytes) -> bytes:
        crc = zlib.crc32(ctype + data) & 0xFFFFFFFF
        return struct.pack(">I", len(data)) + ctype + data + struct.pack(">I", crc)

    raw = b"".join(b"\x00" + bytes(rgba) * width for _ in range(height))
    ihdr = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(raw))
        + chunk(b"IEND", b"")
    )


def insert_rows(db_path: Path, rows: list[tuple]) -> str:
    db_path.parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(str(db_path))
    conn.executescript(SCHEMA_V5)
    for r in rows:
        conn.execute(
            """INSERT INTO clipboard_history
            (profileHash, type, text, displayKind, dataName, size, fileUri, hasData,
             timestamp, lastAccessed, lastModified, starred, pinned, isDeleted, isLocalFileReady)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
            (r[0], r[1], r[2], r[3], r[4], r[5], r[6], r[7], r[8], r[9], r[9], r[10], r[11], r[12], 1 if r[7] else 0),
        )
    conn.commit()
    conn.close()
    return hashlib.sha256(db_path.read_bytes()).hexdigest()


def build_fixtures(out_dir: Path) -> dict:
    if out_dir.exists():
        shutil.rmtree(out_dir)
    out_dir.mkdir(parents=True)

    png_bytes = make_png(16, 16, (0, 122, 255, 255))
    jpg_bytes = SAMPLE_JPEG_BYTES
    heic_bytes = b"\x00\x00\x00\x18ftypheic" + b"legacy-heic-payload-bytes"
    doc_bytes = b"UniClip legacy file payload for migration E2E.\n"
    zip_bytes = b"PK\x03\x04\x14\x00\x00\x00\x00\x00legacy-group-zip"
    temp_bytes = b"UniClip legacy cache/temp_files attachment bytes.\n"
    missing_bytes = b"Restored attachment bytes for partial->complete repair.\n"
    now = int(time.time() * 1000)

    # 1. iOS App Group layout: Databases/uniclipboard.db + extensionless payloads/{Type}-{profileHash}
    ag_root = out_dir / "ios-app-group"
    ag_payloads = ag_root / "payloads"
    ag_payloads.mkdir(parents=True)
    (ag_payloads / "Image-ios-png-1").write_bytes(png_bytes)
    (ag_payloads / "Image-ios-jpg-2").write_bytes(jpg_bytes)
    (ag_payloads / "Image-ios-heic-3").write_bytes(heic_bytes)
    (ag_payloads / "File-ios-doc-4").write_bytes(doc_bytes)
    (ag_payloads / "Group-ios-grp-5").write_bytes(zip_bytes)
    ag_db = ag_root / "Databases" / "uniclipboard.db"
    ag_sha = insert_rows(ag_db, [
        ("ios-txt-0", "Text", PINNED_FIXTURE_TEXT, "text", None, len(PINNED_FIXTURE_TEXT.encode()), None, 0, now - 7000, now - 6500, 1, 1, 0),
        ("ios-url-1", "Text", "https://uniclipboard.app/ios", "url", None, 28, None, 0, now - 6000, now - 6000, 0, 0, 0),
        ("ios-png-1", "Image", "", "image", "capture.png", len(png_bytes), "file:///var/mobile/Containers/Shared/AppGroup/STALE-UUID/payloads/Image-ios-png-1", 1, now - 5000, now - 5000, 0, 1, 0),
        ("ios-jpg-2", "Image", "", "image", "camera.jpg", len(jpg_bytes), None, 1, now - 4000, now - 4000, 1, 0, 0),
        ("ios-heic-3", "Image", "", "image", "live.heic", len(heic_bytes), None, 1, now - 3000, now - 3000, 0, 0, 0),
        ("ios-doc-4", "File", "", "file", "brief.txt", len(doc_bytes), None, 1, now - 2000, now - 2000, 0, 0, 0),
        ("ios-grp-5", "Group", "", "group", "photos.zip", len(zip_bytes), None, 1, now - 1500, now - 1500, 0, 0, 0),
        ("ios-deleted-6", "Text", "Soft deleted iOS item", "text", None, 21, None, 0, now - 500, now - 500, 1, 1, 1),
    ])

    # 2. iOS Sandbox default path: Documents/SQLite/uniclipboard.db + Documents/clipboards/ + Library/Caches/temp_files/
    sb_root = out_dir / "ios-sandbox"
    (sb_root / "Documents" / "clipboards" / "history" / "Image-sb-png-1").mkdir(parents=True)
    (sb_root / "Documents" / "clipboards" / "history" / "Image-sb-png-1" / "sb.png").write_bytes(png_bytes)
    (sb_root / "Library" / "Caches" / "temp_files").mkdir(parents=True)
    (sb_root / "Library" / "Caches" / "temp_files" / "temp-ios-clip.txt").write_bytes(temp_bytes)
    (sb_root / "repair").mkdir(parents=True)
    (sb_root / "repair" / "missing.txt").write_bytes(missing_bytes)
    sb_db = sb_root / "Documents" / "SQLite" / "uniclipboard.db"
    sb_sha = insert_rows(sb_db, [
        ("sb-txt-0", "Text", PINNED_FIXTURE_TEXT, "text", None, len(PINNED_FIXTURE_TEXT.encode()), None, 0, now - 5000, now - 4500, 1, 1, 0),
        ("sb-png-1", "Image", "", "image", "sb.png", len(png_bytes), "file:///var/mobile/Containers/Data/Application/OLD-UUID/Documents/clipboards/history/Image-sb-png-1/sb.png", 1, now - 4000, now - 4000, 0, 0, 0),
        ("sb-tmp-2", "File", "", "file", "temp-ios-clip.txt", len(temp_bytes), "file:///var/mobile/Containers/Data/Application/OLD-UUID/Library/Caches/temp_files/temp-ios-clip.txt", 1, now - 3000, now - 3000, 0, 0, 0),
        ("sb-missing-3", "File", "", "file", "missing.txt", len(missing_bytes), None, 1, now - 2000, now - 2000, 0, 0, 0),
        ("sb-deleted-4", "Text", "Deleted sandbox row", "text", None, 19, None, 0, now - 1000, now - 1000, 0, 0, 1),
    ])

    # 3. Android layout: SQLite/uniclipboard.db + clipboards/history/{Type}-{profileHash}/{dataName} + temp_files/
    an_root = out_dir / "android"
    an_hist = an_root / "clipboards" / "history"
    for sub, fname, content in [
        ("Image-android-png-1", "screenshot.png", png_bytes),
        ("Image-android-jpg-2", "camera.jpg", jpg_bytes),
        ("Image-android-heic-3", "photo.heic", heic_bytes),
        ("File-android-doc-4", "notes.txt", doc_bytes),
        ("Group-android-grp-5", "bundle.zip", zip_bytes),
    ]:
        d = an_hist / sub
        d.mkdir(parents=True)
        (d / fname).write_bytes(content)
    an_temp = an_root / "temp_files"
    an_temp.mkdir(parents=True)
    (an_temp / "clip-temp-android.txt").write_bytes(temp_bytes)
    (an_root / "repair").mkdir(parents=True)
    (an_root / "repair" / "deferred.txt").write_bytes(missing_bytes)
    an_db = an_root / "SQLite" / "uniclipboard.db"
    an_sha = insert_rows(an_db, [
        ("android-txt-0", "Text", PINNED_FIXTURE_TEXT, "text", None, len(PINNED_FIXTURE_TEXT.encode()), None, 0, now - 8000, now - 7500, 1, 1, 0),
        ("android-url-1", "Text", "https://github.com/UniClipboard/UniClipboard", "url", None, 44, None, 0, now - 7000, now - 7000, 0, 0, 0),
        ("android-png-1", "Image", "", "image", "screenshot.png", len(png_bytes), "file:///data/user/0/app.uniclipboard.android/files/clipboards/history/Image-android-png-1/screenshot.png", 1, now - 6000, now - 6000, 0, 1, 0),
        ("android-jpg-2", "Image", "", "image", "camera.jpg", len(jpg_bytes), None, 1, now - 5000, now - 5000, 1, 0, 0),
        ("android-heic-3", "Image", "", "image", "photo.heic", len(heic_bytes), None, 1, now - 4000, now - 4000, 0, 0, 0),
        ("android-doc-4", "File", "", "file", "notes.txt", len(doc_bytes), None, 1, now - 3000, now - 3000, 0, 0, 0),
        ("android-grp-5", "Group", "", "group", "bundle.zip", len(zip_bytes), None, 1, now - 2000, now - 2000, 0, 0, 0),
        ("android-temp-6", "File", "", "file", "clip-temp-android.txt", len(temp_bytes), "file:///data/user/0/app.uniclipboard.android/cache/temp_files/clip-temp-android.txt", 1, now - 1500, now - 1500, 0, 0, 0),
        ("android-missing-7", "File", "", "file", "deferred.txt", len(missing_bytes), None, 1, now - 1000, now - 1000, 0, 0, 0),
        ("android-deleted-8", "Text", "Soft-deleted Android row", "text", None, 24, None, 0, now - 500, now - 500, 1, 1, 1),
    ])

    manifest = {
        "pinned_text": PINNED_FIXTURE_TEXT,
        "ios_app_group": {"db_path": str(ag_db), "payloads_dir": str(ag_payloads), "db_sha256": ag_sha, "active_rows": 7},
        "ios_sandbox": {"db_path": str(sb_db), "db_sha256": sb_sha, "active_rows": 4, "initial_recoverable": 3},
        "android": {"db_path": str(an_db), "db_sha256": an_sha, "active_rows": 9, "initial_recoverable": 8},
    }
    (out_dir / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    return manifest


def main() -> None:
    parser = argparse.ArgumentParser(description="Generate legacy mobile Expo SQLite E2E fixtures")
    parser.add_argument("--out", required=True, help="Output directory for generated fixtures")
    args = parser.parse_args()
    manifest = build_fixtures(Path(args.out).resolve())
    print(json.dumps(manifest))


if __name__ == "__main__":
    main()
