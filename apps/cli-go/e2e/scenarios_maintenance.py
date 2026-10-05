"""Scenarios for the maintenance group: search, upgrade, debug."""
import datetime
import json
import os
import socket
import re
import time
import zipfile

from scenarios import scenario, pair, init_space, settle_oneshot

# Human search output renders minute-precision UTC dates; the two flavors run
# minutes apart, so mask them (the JSON forms keep full precision via <MS>).
_MINUTE_DATE = re.compile(rb"\b\d{4}-\d{2}-\d{2} \d{2}:\d{2}\b")
# Values that tick between the two flavors' runs: the capture countdown and
# the second-precision stamp in exported archive names.
_VOLATILE = [
    (re.compile(rb"(remainingMs\"?: )\d+"), rb"\1<REMAINING>"),
    (re.compile(rb"diagnostics-\d{8}-\d{6}"), b"diagnostics-<STAMP>"),
    # The daemon's /auth/connect rate-limit message counts down per second.
    (re.compile(rb"retry after \d+ seconds"), b"retry after <N> seconds"),
]


# The status read right after a rebuild request races the background
# rebuild: its timestamps may or may not be set yet.
_REBUILD_RACE = [
    (re.compile(rb"(Last rebuild \w+: )(?:never|<DATE>)"), rb"\1<DATE-OR-NEVER>"),
    (re.compile(rb"(last_rebuild_\w+_at_ms\": )(?:null|\d+)"), rb"\1<MS-OR-NULL>"),
]


def masked(r, label, args, ids=None, extra=(), **kw):
    """Run a step and record it with minute dates and per-run ids masked."""
    step = r.run(label, args, compare=False, **kw)
    for attr in ("out", "err"):
        text = _MINUTE_DATE.sub(b"<DATE>", getattr(step, attr))
        for pattern, repl in [*_VOLATILE, *extra]:
            text = pattern.sub(repl, text)
        for value, name in (ids or {}).items():
            text = text.replace(value.encode(), name.encode())
        setattr(step, attr, text)
    for value, name in (ids or {}).items():
        step.argv = [a.replace(value, name) for a in step.argv]
    r.steps.append(step)
    return step


def rust_json(r, args, profile=None):
    step = r.run("fixture: " + " ".join(args), ["--json", *args], cli="rust", profile=profile, compare=False)
    try:
        return json.loads(step.out)
    except ValueError:
        raise RuntimeError(f"fixture {args} failed: {step.code} {step.err!r}")


def _loads(data):
    try:
        return json.loads(data)
    except ValueError:
        return None


def _find_key(value, keys):
    """Depth-first search for the first string under any of `keys`."""
    if isinstance(value, dict):
        for key in keys:
            if isinstance(value.get(key), str):
                return value[key]
        value = list(value.values())
    if isinstance(value, list):
        for item in value:
            found = _find_key(item, keys)
            if found:
                return found
    return None


def wait_for(predicate, timeout=60, interval=1.0):
    deadline = time.time() + timeout
    while time.time() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(interval)
    return None


def start_daemon(r, profile=None):
    r.run("fixture: start", ["start"], cli="rust", profile=profile, compare=False)


def seed_history(r, texts, profile=None):
    for text in texts:
        r.run("fixture: send", ["send", "--text", text], cli="rust", profile=profile, compare=False)


def search_total(r, args, profile=None):
    try:
        return rust_json(r, ["search", *args], profile=profile).get("total", 0)
    except RuntimeError:
        return 0


@scenario
def maintenance_no_space(r):
    """No space: every maintenance command refuses through the session gate."""
    r.run("search", ["search", "x"])
    r.run("search status (json)", ["--json", "search", "status"])
    r.run("search rebuild", ["search", "rebuild"])
    r.run("upgrade", ["upgrade"])
    r.run("upgrade ack (json)", ["--json", "upgrade", "ack"])
    r.run("debug status", ["debug", "status"])
    r.run("debug capture status", ["debug", "capture", "status"])
    masked(r, "debug export-logs", ["debug", "export-logs"])


@scenario
def search_local(r):
    """Queries, filters, pagination and errors on one profile's history."""
    init_space(r)
    start_daemon(r)
    texts = ["alpha bravo charlie", "alpha delta", "echo foxtrot https://example.com/page", "golf notes.md"]
    seed_history(r, texts)
    wait_for(lambda: search_total(r, ["alpha"]) >= 2, timeout=30)
    masked(r, "query", ["search", "alpha"])
    masked(r, "query json", ["--json", "search", "alpha"])
    masked(r, "query detailed", ["search", "alpha", "--detailed"])
    masked(r, "query detailed json", ["--json", "search", "alpha", "--detailed"])
    # The masks above hide dates, so check the human rendering against the
    # JSON timestamps directly: UTC, minute precision.
    raw_json = r.run("date check json", ["--json", "search", "alpha"], compare=False)
    raw_human = r.run("date check human", ["search", "alpha"], compare=False)
    expected = [datetime.datetime.fromtimestamp(i["active_time_ms"] / 1000, datetime.timezone.utc)
                .strftime("%Y-%m-%d %H:%M") for i in json.loads(raw_json.out)["data"]]
    rendered = re.findall(r"^- \[\w+\] (\S+ \S+)  ", raw_human.out.decode(), re.M)
    r.note("human dates are UTC minutes of active_time_ms", {"count": len(expected), "match": expected == rendered})
    masked(r, "page 1", ["search", "alpha", "--limit", "1"])
    masked(r, "page 2 json", ["--json", "search", "alpha", "--limit", "1", "--offset", "1"])
    masked(r, "past the end", ["search", "alpha", "--offset", "10"])
    masked(r, "no match", ["search", "zulu"])
    masked(r, "no match json", ["--json", "search", "zulu"])
    masked(r, "operator or", ["search", "bravo delta", "--operator", "or"])
    masked(r, "operator and", ["--json", "search", "bravo delta", "--operator", "and"])
    masked(r, "bad operator", ["search", "alpha", "--operator", "xor"])
    masked(r, "bad operator json", ["--json", "search", "alpha", "--operator", "xor"])
    masked(r, "time preset today", ["search", "alpha", "--time-preset", "today"])
    masked(r, "time preset yesterday", ["search", "alpha", "--time-preset", "yesterday"])
    masked(r, "bad time preset", ["search", "alpha", "--time-preset", "never"])
    masked(r, "bad time preset json", ["--json", "search", "alpha", "--time-preset", "never"])
    now = int(time.time() * 1000)
    masked(r, "absolute range", ["search", "alpha", "--from-ms", str(now - 3_600_000), "--to-ms", str(now + 3_600_000)])
    masked(r, "range in the past", ["--json", "search", "--from-ms", "+0", "--to-ms", "1000"])
    masked(r, "inverted range", ["search", "alpha", "--from-ms", "2000", "--to-ms", "1000"])
    masked(r, "from without to", ["search", "alpha", "--from-ms", "1"])
    masked(r, "to without from (filter only)", ["--json", "search", "--to-ms", "1"])
    masked(r, "missing query", ["search"])
    masked(r, "missing query json", ["--json", "search"])
    masked(r, "type filter only", ["search", "--type", "text"])
    masked(r, "type filter repeat", ["--json", "search", "--type", "text", "--type", "image"])
    masked(r, "bad type", ["search", "--type", "bogus"])
    masked(r, "tag link", ["search", "--tag", "link", "--detailed"])
    masked(r, "tag favorited", ["--json", "search", "--tag", "favorited"])
    masked(r, "custom tag", ["search", "--tag", "nope"])
    masked(r, "ext filter", ["search", "--ext", "md", "--ext", "txt"])
    masked(r, "empty query string", ["search", ""])
    masked(r, "empty query with filter", ["--json", "search", "", "--type", "text"])
    masked(r, "limit zero", ["search", "alpha", "--limit", "0"])
    masked(r, "huge limit", ["--json", "search", "alpha", "--limit", "4294967295"])
    masked(r, "query with symbols", ["search", "https://example.com/page"])
    masked(r, "unicode query", ["search", "ÄLPHA ünïcode"])
    masked(r, "unknown source device", ["search", "--source-device", "nobody"])
    masked(r, "unknown source device json", ["--json", "search", "alpha", "--source-device", "nobody"])
    masked(r, "local source by name", ["--json", "search", "--source-device", "COMPAT-A"])


@scenario
def search_index_maintenance(r):
    """`search status` and `search rebuild` with human and JSON output."""
    init_space(r)
    start_daemon(r)
    seed_history(r, ["index one", "index two"])
    masked(r, "status", ["search", "status"])
    masked(r, "status json", ["--json", "search", "status"])
    masked(r, "rebuild", ["search", "rebuild"], extra=_REBUILD_RACE)
    wait_for(lambda: rust_json(r, ["search", "status"]).get("state") == "ready", timeout=60)
    masked(r, "rebuild json", ["--json", "search", "rebuild"], extra=_REBUILD_RACE)
    wait_for(lambda: rust_json(r, ["search", "status"]).get("state") == "ready", timeout=60)
    masked(r, "status after rebuild", ["search", "status"])
    masked(r, "status json after rebuild", ["--json", "search", "status"])
    masked(r, "query after rebuild", ["search", "index"])
    masked(r, "subcommand with flag", ["search", "status", "--limit", "3"])
    masked(r, "query and subcommand", ["search", "rebuild", "extra"])


@scenario
def search_oneshot(r):
    """Search through a transient daemon spawned by the command itself."""
    init_space(r)
    r.run("fixture: stop", ["stop"], cli="rust", compare=False)
    masked(r, "query (oneshot)", ["search", "anything"])
    settle_oneshot(r)
    masked(r, "status (oneshot)", ["--json", "search", "status"])
    settle_oneshot(r)
    masked(r, "missing query (oneshot)", ["search"])


@scenario
def search_source_devices(r):
    """`--source-device` resolution against paired peers and mobile devices."""
    sponsor, joiner = pair(r)
    start_daemon(r, profile=sponsor)
    start_daemon(r, profile=joiner)
    seed_history(r, ["from sponsor kilo", "from sponsor lima"], profile=sponsor)
    wait_for(lambda: search_total(r, ["sponsor"], profile=joiner) >= 2, timeout=60)
    members = rust_json(r, ["member", "list"], profile=joiner)
    ids = {}
    for m in members:
        pid = m.get("peerId") or m.get("peer_id") or m.get("deviceId") or m.get("device_id")
        name = m.get("deviceName") or m.get("device_name")
        if pid:
            ids[pid] = f"<ID:{name}>"
    r.note("joiner members", sorted(ids.values()))
    masked(r, "by name", ["search", "sponsor", "--source-device", "compat-sponsor", "--detailed"], ids=ids, profile=joiner)
    masked(r, "by name json", ["--json", "search", "--source-device", "Compat-Sponsor"], ids=ids, profile=joiner)
    sponsor_id = next((k for k, v in ids.items() if v == "<ID:compat-sponsor>"), "missing")
    masked(r, "by id", ["--json", "search", "kilo", "--source-device", sponsor_id], ids=ids, profile=joiner)
    masked(r, "name and id dedup", ["--json", "search", "--source-device", sponsor_id, "--source-device", "COMPAT-SPONSOR"],
           ids=ids, profile=joiner)
    masked(r, "local device has no remote entries", ["search", "sponsor", "--source-device", "compat-joiner"],
           ids=ids, profile=joiner)
    masked(r, "unknown name", ["search", "sponsor", "--source-device", "compat-ghost"], ids=ids, profile=joiner)
    masked(r, "second unknown after a known one", ["search", "--source-device", "compat-sponsor",
                                                   "--source-device", "ghost"], ids=ids, profile=joiner)
    # A mobile-sync device labelled like the sponsor peer makes the name ambiguous.
    # `mobile setup` turns on the LAN listener (on a free port) and registers one device.
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    added = r.run("fixture: mobile setup", ["--json", "mobile", "setup", "--non-interactive", "--accept-network-risk",
                                            "--label", "compat-sponsor", "--ip", "127.0.0.1", "--port", str(port)],
                  cli="rust", profile=joiner, compare=False)
    mobile_id = _find_key(_loads(added.out), ("deviceId", "device_id"))
    if not mobile_id:
        raise RuntimeError(f"mobile setup fixture failed: {added.code} {added.out!r} {added.err!r}")
    ids["mobile_sync:" + mobile_id] = "<ID:mobile>"
    masked(r, "ambiguous name", ["search", "--source-device", "compat-sponsor"], ids=ids, profile=joiner)
    masked(r, "mobile id still resolves", ["--json", "search", "--source-device",
                                           next((k for k, v in ids.items() if v == "<ID:mobile>"), "x")],
           ids=ids, profile=joiner)
    masked(r, "unknown lists mobile too", ["search", "--source-device", "ghost"], ids=ids, profile=joiner)


@scenario
def upgrade_cursor(r):
    """Fresh install, acknowledge, then no change."""
    init_space(r)
    start_daemon(r)
    r.run("upgrade (bare)", ["upgrade"])
    r.run("upgrade status json", ["--json", "upgrade", "status"])
    r.run("ack", ["upgrade", "ack"])
    r.run("status after ack", ["upgrade", "status"])
    r.run("ack again json", ["--json", "upgrade", "ack"])
    r.run("bare json after ack", ["--json", "upgrade"])
    r.run("ack with arg", ["upgrade", "ack", "now"])


def _write_cursor(r, content):
    """Fixture: rewrite the daemon's plaintext version cursor (daemon stopped)."""
    r.run("fixture: stop", ["stop"], cli="rust", compare=False)
    path = os.path.join(r.data_root(), "upgrade-cursor.json")
    if content is None:
        os.remove(path)
    else:
        with open(path, "w") as fh:
            fh.write(content)


def _cursor(version):
    return json.dumps({"schema_version": 1, "last_seen_version": version}, indent=2)


@scenario
def upgrade_transitions(r):
    """Upgraded, downgraded and unknown-previous-version cursors."""
    init_space(r)
    _write_cursor(r, _cursor("0.9.0"))
    r.run("upgraded", ["upgrade"])
    r.run("upgraded json", ["--json", "upgrade", "status"])
    r.run("ack upgraded", ["upgrade", "ack"])
    r.run("after ack", ["--json", "upgrade"])
    _write_cursor(r, _cursor("99.0.0"))
    r.run("downgraded", ["upgrade", "status"])
    r.run("downgraded json", ["--json", "upgrade"])
    _write_cursor(r, "{not json")
    r.run("corrupt cursor", ["upgrade"])
    r.run("corrupt cursor json", ["--json", "upgrade", "status"])
    _write_cursor(r, None)
    r.run("missing cursor", ["upgrade"])
    r.run("missing cursor json", ["--json", "upgrade", "status"])
    with open(os.path.join(r.data_root(), "upgrade-cursor.json")) as fh:
        r.note("cursor after reads", json.load(fh) if fh.read(1) else None)


@scenario
def upgrade_oneshot(r):
    """Upgrade commands through a transient daemon."""
    init_space(r)
    settle_oneshot(r)
    r.run("status (oneshot)", ["upgrade", "status"])
    settle_oneshot(r)
    r.run("ack (oneshot, json)", ["--json", "upgrade", "ack"])
    settle_oneshot(r)
    r.run("bare (oneshot)", ["upgrade"])


@scenario
def debug_mode(r):
    """Persistent debug mode toggling, read back after each change."""
    init_space(r)
    start_daemon(r)
    r.run("status", ["debug", "status"])
    r.run("status json", ["--json", "debug", "status"])
    r.run("on", ["debug", "on"])
    r.run("status after on", ["--json", "debug", "status"])
    r.run("on again json", ["--json", "debug", "on"])
    r.run("off", ["debug", "off"])
    r.run("status after off", ["debug", "status"])
    r.run("off again json", ["--json", "debug", "off"])
    r.run("restart fixture", ["stop"], cli="rust", compare=False)
    r.run("on (oneshot)", ["debug", "on"])
    settle_oneshot(r)
    r.run("status (oneshot)", ["--json", "debug", "status"])
    settle_oneshot(r)
    r.run("off (oneshot)", ["debug", "off"])


def _capture_id(step):
    try:
        return json.loads(step.out)["capture"]["captureId"]
    except (ValueError, KeyError, TypeError):
        return None


@scenario
def debug_capture(r):
    """Bounded detailed capture: start, status, stop with good and bad ids."""
    init_space(r)
    start_daemon(r)
    masked(r, "status idle", ["debug", "capture", "status"])
    r.run("status idle json", ["--json", "debug", "capture", "status"])
    r.run("stop unknown id", ["debug", "capture", "stop", "nope"])
    r.run("stop unknown id json", ["--json", "debug", "capture", "stop", "nope"])
    started = masked(r, "start 1 minute json", ["--json", "debug", "capture", "start", "--minutes", "1"])
    capture = _capture_id(started) or "missing"
    ids = {capture: "<CAPTURE>"}
    masked(r, "start again reuses", ["debug", "capture", "start"], ids=ids)
    masked(r, "status active", ["debug", "capture", "status"], ids=ids)
    masked(r, "status active json", ["--json", "debug", "capture", "status"], ids=ids)
    masked(r, "stop other id", ["debug", "capture", "stop", "other"], ids=ids)
    masked(r, "stop other id json", ["--json", "debug", "capture", "stop", "other"], ids=ids)
    masked(r, "stop", ["debug", "capture", "stop", capture], ids=ids)
    masked(r, "stop again json", ["--json", "debug", "capture", "stop", capture], ids=ids)
    masked(r, "stop again", ["debug", "capture", "stop", capture], ids=ids)
    masked(r, "status after stop", ["debug", "capture", "status"], ids=ids)
    masked(r, "start 15 minutes human", ["debug", "capture", "start", "--minutes", "15"])
    masked(r, "status after human start json", ["--json", "debug", "capture", "status"])


def _archive_entries(path):
    try:
        with zipfile.ZipFile(path) as zf:
            return sorted(zf.namelist())
    except (OSError, zipfile.BadZipFile) as exc:
        return f"unreadable: {exc.__class__.__name__}"


_DATED = re.compile(r"\d{8}-\d{6}|\d{4}-\d{2}-\d{2}(?:[T_ -]?\d{2}[-:]?\d{2}[-:]?\d{2})?")


def _normalize_names(names):
    if not isinstance(names, list):
        return names
    return sorted({_DATED.sub("<DATE>", n) for n in names})


@scenario
def debug_export_logs(r):
    """Log export to $HOME/Downloads: reported files and archive entries."""
    init_space(r)
    start_daemon(r)
    downloads = os.path.join(r.home, "Downloads")
    step = r.run("export json", ["--json", "debug", "export-logs", "--since-hours", "1"], compare=False)
    try:
        result = json.loads(step.out)
        r.note("export json shape", {
            "exit": step.code,
            "keys": list(result.keys()),
            "preparation_keys": list(result["enginePreparation"].keys()),
            "collection_keys": list(result["collection"].keys()),
            "path_in_downloads": os.path.dirname(result["path"]) == downloads,
            "included": _normalize_names(result["includedFiles"]),
            "archive": _normalize_names(_archive_entries(result["path"])),
        })
    except (ValueError, KeyError, TypeError) as exc:
        r.note("export json failed", {"exit": step.code, "err": step.err.decode(errors="replace"), "exc": repr(exc)})
    # The daemon's diagnostic counters depend on runtime event timing.
    counters = [(re.compile(rb'("\w+(?:Count|Records)": )"\d+"'), rb'\1"<N>"')]
    masked(r, "export json full", ["--json", "debug", "export-logs", "--since-hours", "2"], extra=counters)
    masked(r, "export human", ["debug", "export-logs"])
    masked(r, "export zero hours", ["debug", "export-logs", "--since-hours", "0"])
    r.note("downloads listing", _normalize_names(sorted(os.listdir(downloads)) if os.path.isdir(downloads) else []))
