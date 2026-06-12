#!/usr/bin/env python3
"""Local UniClipboard mobile-LAN stub for development and simctl regression.

Implements the desktop daemon's `/reference/mobile-api` surface so the iOS
client can drive both read and write paths against a single in-memory
backend. The stub is stateful within one process — restart = clean slate.

Endpoints (all under HTTP Basic Auth):

- Probes:        GET /, /version, /api/version, /api/time
- Live clipboard: GET /SyncClipboard.json, PUT /SyncClipboard.json
- Files:         GET/HEAD/PUT /file/<name>, GET/DELETE /file
- History:       POST /api/history, POST /api/history/query,
                 GET /api/history/statistics,
                 GET /api/history/<profileId>, GET /api/history/<profileId>/data,
                 PATCH /api/history/<type>/<hash>, DELETE /api/history/clear

Body-size caps:  16 MiB for JSON/metadata, 10 GiB for binary uploads.
Image sniffing:  when Content-Type is application/octet-stream, magic-byte
                 detect JPEG/PNG/GIF/WEBP/BMP/TIFF/HEIC/HEIF so Android-style
                 raw uploads still classify as Image.

Usage:

    # Default — stateful stub. Auth: u/p (override via STUB_USER/STUB_PASS).
    scripts/sync-stub-server.py

    # Force every endpoint to return a given HTTP status (regression mode).
    STUB_MODE=401 scripts/sync-stub-server.py
    STUB_MODE=500 scripts/sync-stub-server.py
    STUB_MODE=418 scripts/sync-stub-server.py     # any int → that status

    # Custom port.
    STUB_PORT=9000 scripts/sync-stub-server.py

    # Disable Basic Auth (for ad-hoc curl).
    STUB_AUTH=off scripts/sync-stub-server.py

State is seeded so the initial GET /SyncClipboard.json is byte-identical to
docs/examples/clipboard_text_short.json — keeps the fixture round-trip
property the model tests rely on.

iOS simulator note — write the configured server as `Data` into the App
Group container's prefs plist (see CLAUDE.md "Local stub server" section).
The full recipe is too long to fit here; consult CLAUDE.md.
"""

from __future__ import annotations

import base64
import datetime
import hashlib
import http.server
import io
import json
import os
import re
import sys
import threading
import time
import urllib.parse
from dataclasses import dataclass, field
from email.parser import BytesParser
from email.policy import default as email_default_policy

# --- Configuration ---------------------------------------------------------

PORT = int(os.environ.get("STUB_PORT", "8033"))
MODE = os.environ.get("STUB_MODE", "ok").lower()
AUTH_ENABLED = os.environ.get("STUB_AUTH", "on").lower() != "off"
AUTH_USER = os.environ.get("STUB_USER", "u")
AUTH_PASS = os.environ.get("STUB_PASS", "p")
EXPECTED_AUTH = "Basic " + base64.b64encode(
    f"{AUTH_USER}:{AUTH_PASS}".encode("utf-8")
).decode("ascii")

CAP_JSON_BYTES = 16 * 1024 * 1024       # 16 MiB for metadata
CAP_BINARY_BYTES = 10 * 1024 * 1024 * 1024  # 10 GiB for files
CHUNK = 1024 * 1024                     # 1 MiB read buffer

# --- Helpers ---------------------------------------------------------------


def _iso(delta_seconds: float = 0.0) -> str:
    """Fractional-Z ISO-8601 timestamp `<now> + delta_seconds`."""
    when = datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(
        seconds=delta_seconds
    )
    return when.strftime("%Y-%m-%dT%H:%M:%S.") + f"{when.microsecond // 1000:03d}Z"


def _bytes_hash(body: bytes) -> str:
    """§4.1/§4.2 — uppercase hex SHA-256 over the raw bytes (all types)."""
    return hashlib.sha256(body).hexdigest().upper()


def _sniff_image_ext(name: str, head: bytes) -> str | None:
    """Best-effort image format detection for raw `octet-stream` uploads.
    Returns a lowercase extension (`png`, `jpg`, `gif`, `webp`, `bmp`,
    `tif`, `heic`) or None when the bytes don't look like a known image.
    Filename extension is consulted as a tiebreaker."""
    if len(head) >= 8 and head.startswith(b"\x89PNG\r\n\x1a\n"):
        return "png"
    if len(head) >= 3 and head[:3] == b"\xff\xd8\xff":
        return "jpg"
    if len(head) >= 6 and head[:6] in (b"GIF87a", b"GIF89a"):
        return "gif"
    if len(head) >= 12 and head[:4] == b"RIFF" and head[8:12] == b"WEBP":
        return "webp"
    if len(head) >= 2 and head[:2] == b"BM":
        return "bmp"
    if len(head) >= 4 and head[:4] in (b"II*\x00", b"MM\x00*"):
        return "tif"
    if len(head) >= 12 and head[4:8] == b"ftyp" and head[8:12] in (
        b"heic", b"heix", b"heim", b"heis", b"hevc", b"hevx", b"hevm", b"hevs",
        b"mif1", b"msf1",
    ):
        return "heic"
    # Filename-only fallback
    ext = name.rsplit(".", 1)[-1].lower() if "." in name else ""
    if ext in {"png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff", "heic", "heif"}:
        return "jpeg" if ext == "jpg" else ext
    return None


# --- State -----------------------------------------------------------------


@dataclass
class HistoryRecord:
    hash: str
    type: str           # Text / Image / File / Group
    text: str
    hasData: bool
    size: int
    createTime: str
    lastModified: str
    starred: bool = False
    pinned: bool = False
    version: int = 0
    isDeleted: bool = False

    def to_dict(self) -> dict:
        return {
            "hash": self.hash, "type": self.type, "text": self.text,
            "hasData": self.hasData, "size": self.size,
            "createTime": self.createTime, "lastModified": self.lastModified,
            "starred": self.starred, "pinned": self.pinned,
            "version": self.version, "isDeleted": self.isDeleted,
        }


@dataclass
class LiveState:
    clipboard_meta: dict | None = None
    files: dict[str, bytes] = field(default_factory=dict)
    # The desktop's compatibility bridge keeps only the latest history
    # entry; we mirror that. `latest` is None when the clipboard is empty.
    latest: HistoryRecord | None = None
    lock: threading.Lock = field(default_factory=threading.Lock)

    def set_clipboard(self, meta: dict) -> None:
        with self.lock:
            self.clipboard_meta = meta
            self.latest = HistoryRecord(
                hash=meta.get("hash") or "",
                type=meta.get("type", "Text"),
                text=meta.get("text", ""),
                hasData=bool(meta.get("hasData", False)),
                size=int(meta.get("size", 0)),
                createTime=_iso(),
                lastModified=_iso(),
            )


STATE = LiveState()

# --- Seed: fixture-identical clipboard_text_short.json so the existing
# round-trip tests still pass against a freshly-started stub.

_SEED_CLIPBOARD = {
    "type": "Text",
    "hash": "3F4E62D9F184380BAD1B0F94B5518DCBF35ACB79B34F6D6E34F3DAB16CD7BC8F",
    "text": "Hello, SyncClipboard!",
    "hasData": False,
    "size": 21,
}
STATE.set_clipboard(_SEED_CLIPBOARD)

# --- HTTP handler ----------------------------------------------------------


# Profile-id route pattern: /api/history/<type>-<HEXHASH>[/data]
PROFILE_ID_RE = re.compile(r"^/api/history/([^/]+-[0-9A-Fa-f]+)(/data)?$")
# Compat PATCH route: /api/history/<Type>/<Hash>
PATCH_FLAG_RE = re.compile(r"^/api/history/([^/]+)/([0-9A-Fa-f]+)$")


class Handler(http.server.BaseHTTPRequestHandler):
    # --- access log: one line per request ---------------------------------

    def log_message(self, fmt: str, *args) -> None:
        sys.stderr.write(
            f"[stub] {self.command} {self.path} → {args[1] if len(args) > 1 else '?'}\n"
        )
        sys.stderr.flush()

    # --- auth + mode gates ------------------------------------------------

    def _force_mode(self) -> bool:
        """If STUB_MODE is a digit, send that status and return True."""
        if MODE.isdigit():
            self.send_response(int(MODE))
            self.end_headers()
            return True
        return False

    def _auth_ok(self) -> bool:
        if not AUTH_ENABLED:
            return True
        got = self.headers.get("Authorization", "")
        if got == EXPECTED_AUTH:
            return True
        self.send_response(401)
        self.send_header("WWW-Authenticate", 'Basic realm="UniClipboard"')
        self.end_headers()
        return False

    # --- body readers -----------------------------------------------------

    def _read_capped(self, cap: int) -> bytes | None:
        """Read up to `cap` bytes from the request body. Returns None and
        sends 413 if Content-Length advertises more, or if streamed bytes
        exceed `cap`."""
        length = int(self.headers.get("Content-Length", "0") or 0)
        if length > cap:
            self.send_response(413)
            self.end_headers()
            return None
        buf = bytearray()
        remaining = length
        while remaining > 0:
            chunk = self.rfile.read(min(CHUNK, remaining))
            if not chunk:
                break
            buf.extend(chunk)
            if len(buf) > cap:
                self.send_response(413)
                self.end_headers()
                return None
            remaining -= len(chunk)
        return bytes(buf)

    # --- response helpers -------------------------------------------------

    def _json(self, status: int, payload) -> None:
        body = json.dumps(payload, separators=(",", ":")).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)

    def _bytes(self, status: int, body: bytes, ctype: str) -> None:
        self.send_response(status)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)

    def _empty(self, status: int) -> None:
        self.send_response(status)
        self.send_header("Content-Length", "0")
        self.end_headers()

    # --- routing ----------------------------------------------------------

    def do_GET(self):    self._dispatch("GET")
    def do_HEAD(self):   self._dispatch("HEAD")
    def do_PUT(self):    self._dispatch("PUT")
    def do_POST(self):   self._dispatch("POST")
    def do_DELETE(self): self._dispatch("DELETE")
    def do_PATCH(self):  self._dispatch("PATCH")

    def _dispatch(self, method: str) -> None:
        if self._force_mode():
            return
        if not self._auth_ok():
            return
        try:
            self._route(method)
        except Exception as exc:  # never crash the server
            sys.stderr.write(f"[stub] handler error on {method} {self.path}: {exc}\n")
            sys.stderr.flush()
            self._empty(500)

    def _route(self, method: str) -> None:
        path = self.path.split("?", 1)[0]

        # --- Probes -------------------------------------------------------
        if path == "/" and method == "GET":
            return self._bytes(200, b"UniClipboard mobile-LAN stub", "text/plain")
        if path in ("/version", "/api/version") and method == "GET":
            return self._json(200, {"version": "stub-1.0.0"})
        if path == "/api/time" and method == "GET":
            return self._json(200, {"time": _iso()})

        # --- Live clipboard ----------------------------------------------
        if path == "/SyncClipboard.json":
            if method == "GET":
                with STATE.lock:
                    meta = STATE.clipboard_meta
                if meta is None:
                    return self._empty(404)
                return self._json(200, meta)
            if method == "PUT":
                body = self._read_capped(CAP_JSON_BYTES)
                if body is None:
                    return
                try:
                    meta = json.loads(body)
                    if not isinstance(meta, dict) or "type" not in meta:
                        raise ValueError("missing 'type'")
                except Exception as e:
                    return self._json(400, {"error": f"invalid JSON: {e}"})
                STATE.set_clipboard(meta)
                return self._empty(200)

        # --- File endpoints ----------------------------------------------
        if path.startswith("/file/") and len(path) > len("/file/"):
            name = urllib.parse.unquote(path[len("/file/"):])
            if "/" in name or "\\" in name or not name:
                return self._empty(400)
            if method == "GET":
                with STATE.lock:
                    blob = STATE.files.get(name)
                if blob is None:
                    return self._empty(404)
                return self._bytes(200, blob, "application/octet-stream")
            if method == "HEAD":
                with STATE.lock:
                    blob = STATE.files.get(name)
                if blob is None:
                    return self._empty(404)
                self.send_response(200)
                self.send_header("Content-Type", "application/octet-stream")
                self.send_header("Content-Length", str(len(blob)))
                self.end_headers()
                return
            if method == "PUT":
                body = self._read_capped(CAP_BINARY_BYTES)
                if body is None:
                    return
                with STATE.lock:
                    STATE.files[name] = body
                return self._empty(200)

        if path == "/file":
            if method == "GET":
                with STATE.lock:
                    names = sorted(STATE.files.keys())
                return self._json(200, {"files": names})
            if method == "DELETE":
                # Compat-only: ack but don't mutate (matches desktop).
                return self._empty(200)

        # --- History endpoints -------------------------------------------
        if path == "/api/history" and method == "POST":
            return self._post_history()

        if path == "/api/history/query" and method == "POST":
            return self._post_history_query()

        if path == "/api/history/statistics" and method == "GET":
            with STATE.lock:
                count = 1 if STATE.latest else 0
            return self._json(200, {"total": count, "byType": {}})

        if path == "/api/history/clear" and method == "DELETE":
            return self._empty(200)  # compat-only

        m = PROFILE_ID_RE.match(path)
        if m and method == "GET":
            profile_id = m.group(1)
            wants_data = m.group(2) == "/data"
            return self._get_history_by_profile(profile_id, wants_data)

        m = PATCH_FLAG_RE.match(path)
        if m and method == "PATCH":
            return self._empty(200)  # compat-only

        # --- Fallthrough --------------------------------------------------
        self._empty(404)

    # --- History handlers -------------------------------------------------

    def _post_history(self) -> None:
        """Android-compat inbound: text/url-encoded form OR multipart with
        binary attachment. Builds a HistoryRecord, updates state, returns
        the canonical record."""
        ctype = self.headers.get("Content-Type", "")
        if ctype.startswith("multipart/form-data"):
            cap = CAP_BINARY_BYTES
        else:
            cap = CAP_JSON_BYTES
        body = self._read_capped(cap)
        if body is None:
            return
        fields = self._parse_form(ctype, body)
        kind = fields.get("type", "Text") if isinstance(fields.get("type"), str) else "Text"
        text = fields.get("text", "") if isinstance(fields.get("text"), str) else ""
        # File attachment (multipart only): pull name + bytes from the part.
        payload_name = fields.get("__payload_name")
        payload_bytes = fields.get("__payload_bytes")
        if payload_bytes is not None and not isinstance(payload_bytes, bytes):
            payload_bytes = None

        if payload_bytes is not None and payload_name:
            # Reclassify octet-stream → image when bytes look like one.
            if kind in ("File", "Text") and ctype.startswith("multipart/"):
                sniff = _sniff_image_ext(payload_name, payload_bytes[:32])
                if sniff is not None:
                    kind = "Image"
            with STATE.lock:
                STATE.files[payload_name] = payload_bytes
            hash_ = _bytes_hash(payload_bytes)
            entry_meta = {
                "type": kind,
                "hash": hash_,
                "text": payload_name,
                "hasData": True,
                "dataName": payload_name,
                "size": len(payload_bytes),
            }
        else:
            payload_bytes_for_hash = text.encode("utf-8")
            hash_ = _bytes_hash(payload_bytes_for_hash)
            entry_meta = {
                "type": kind,
                "hash": hash_,
                "text": text,
                "hasData": False,
                "size": len(payload_bytes_for_hash),
            }

        STATE.set_clipboard(entry_meta)
        with STATE.lock:
            rec = STATE.latest.to_dict() if STATE.latest else {}
        self._json(200, rec)

    def _post_history_query(self) -> None:
        """Compat: return one record (the latest) on page 1, empty on
        subsequent pages. The iOS client paginates until an empty array."""
        body = self._read_capped(CAP_JSON_BYTES)
        if body is None:
            return
        fields = self._parse_form(self.headers.get("Content-Type", ""), body)
        page_raw = fields.get("page")
        page = 1
        if isinstance(page_raw, str) and page_raw.strip().isdigit():
            page = int(page_raw.strip())
        with STATE.lock:
            latest = STATE.latest
        if page == 1 and latest is not None:
            return self._json(200, [latest.to_dict()])
        return self._json(200, [])

    def _get_history_by_profile(self, profile_id: str, wants_data: bool) -> None:
        """Compat bridge: resolves only if the profile_id matches the
        current latest record. The desktop spec says this endpoint does
        NOT expose older history — only the latest."""
        with STATE.lock:
            latest = STATE.latest
        if latest is None:
            return self._empty(404)
        expected = f"{latest.type}-{latest.hash}"
        if profile_id != expected:
            return self._empty(404)
        if wants_data:
            if not latest.hasData:
                return self._empty(404)
            with STATE.lock:
                blob = STATE.files.get(latest.text)
            if blob is None:
                return self._empty(404)
            return self._bytes(200, blob, "application/octet-stream")
        return self._json(200, latest.to_dict())

    # --- multipart / form parser -----------------------------------------

    def _parse_form(self, ctype: str, body: bytes) -> dict:
        """Returns a dict mixing text fields (str values) and one optional
        file part exposed as `__payload_name` (str) + `__payload_bytes`
        (bytes). The schema is intentionally flat — handlers above pull
        the keys they care about."""
        if not body:
            return {}
        if ctype.startswith("application/x-www-form-urlencoded"):
            try:
                parsed = urllib.parse.parse_qs(body.decode("utf-8"), keep_blank_values=True)
                return {k: (v[0] if v else "") for k, v in parsed.items()}
            except Exception:
                return {}
        if ctype.startswith("multipart/form-data"):
            try:
                # Glue the body onto a synthetic MIME header so the email
                # parser can chew through it without us hand-rolling the
                # boundary state machine.
                raw = b"Content-Type: " + ctype.encode("ascii") + b"\r\n\r\n" + body
                msg = BytesParser(policy=email_default_policy).parsebytes(raw)
                out: dict = {}
                for part in msg.iter_parts():
                    name = part.get_param("name", header="content-disposition")
                    if not name:
                        continue
                    filename = part.get_param("filename", header="content-disposition")
                    payload = part.get_payload(decode=True)
                    if filename:
                        out["__payload_name"] = filename
                        out["__payload_bytes"] = payload if isinstance(payload, (bytes, bytearray)) else b""
                    else:
                        try:
                            out[name] = payload.decode("utf-8") if isinstance(payload, (bytes, bytearray)) else ""
                        except Exception:
                            out[name] = ""
                return out
            except Exception as e:
                sys.stderr.write(f"[stub] multipart parse failed: {e}\n")
                return {}
        return {}


def main() -> None:
    print(
        f"sync-stub-server listening 127.0.0.1:{PORT} mode={MODE} "
        f"auth={'on' if AUTH_ENABLED else 'off'} (user={AUTH_USER})",
        flush=True,
    )
    server = http.server.ThreadingHTTPServer(("127.0.0.1", PORT), Handler)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        sys.exit(0)


if __name__ == "__main__":
    main()
