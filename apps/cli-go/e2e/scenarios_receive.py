"""Receive group scenarios: `get`, `watch` and the deprecated `recv`.

Remote content is produced by the Rust CLI on the sponsor profile (`send`),
and the commands under test run on the joiner profile.
"""
import base64
import hashlib
import json
import os
import random
import re
import signal
import subprocess
import time

from scenarios import scenario, pair, init_space, settle_oneshot


class _Step:
    """Same shape as compat.Step (compat is the running __main__ module)."""

    def __init__(self, label, argv, code, out, err):
        self.label, self.argv, self.code, self.out, self.err = label, argv, code, out, err
        self.extra = {}


_VOLATILE_PATTERNS = [
    # Eight-character entry id prefixes (`│  1a2b3c4d: [text] ...`, `entry 1a2b3c4d`).
    (re.compile("(│  )[0-9a-f]{8}(: \\[)".encode()), "\\1<ID8>\\2".encode()),
    (re.compile(rb"((?:entry|Entry|id) )[0-9a-f]{8}\b"), rb"\1<ID8>"),
    # Watch previews of received file URIs are cut at 120 characters, and the
    # cut point depends on the per-flavor HOME length.
    (re.compile("file://\\S*… \\(".encode()), "file://<PATH>… (".encode()),
    # The JSON form carries the daemon's preview of the received file URI and
    # the URI list size, both of which embed the per-flavor HOME path.
    (re.compile(rb'"text":"file://[^"]*"'), rb'"text":"file://<PATH>"'),
    (re.compile(rb"text/uri-list/\d+B"), rb"text/uri-list/<N>B"),
]


def _mask_volatile(r):
    """Mask random short entry ids and HOME-length-dependent truncation, which
    differ between the two flavors' runs by construction."""
    for step in r.steps:
        for pattern, repl in _VOLATILE_PATTERNS:
            step.err = pattern.sub(repl, step.err)
            step.out = pattern.sub(repl, step.out)


def _cooldown():
    """The daemon rate-limits `/auth/connect` (HTTP 429 for 60 s) and every
    CLI request exchanges a session token, so long scenarios pause between
    blocks of commands."""
    time.sleep(61)


def _binary(r, cli="self"):
    return os.path.join(r.rust_dir if cli == "rust" else r.bindir, "uniclip")


def _send(r, profile, args, label):
    """Fixture: send from `profile` with the Rust CLI and require success."""
    step = r.run("fixture: send " + label, ["--json", "send", *args], cli="rust", profile=profile,
                 compare=False, timeout=90)
    if step.code != 0:
        raise RuntimeError(f"send {label} failed: {step.code} {step.err!r}")
    return step


def _sha(data):
    return hashlib.sha256(data).hexdigest()


def _file_sha(path):
    with open(path, "rb") as fh:
        return _sha(fh.read())


def _listing(root):
    """Files under root with sizes and digests, for state probes."""
    rows = {}
    for base, _dirs, files in os.walk(root):
        for name in files:
            path = os.path.join(base, name)
            rows[os.path.relpath(path, root)] = {"size": os.path.getsize(path), "sha256": _file_sha(path)}
    return rows


def _spawn_ready(r, label, args, needle, profile, timeout=60):
    """Start a long-running step and wait until `needle` shows on stderr."""
    handle = r.spawn(label, args, profile=profile)
    handle["err_buf"] = b""
    if needle is None:
        return handle
    fd = handle["proc"].stderr
    os.set_blocking(fd.fileno(), False)
    deadline = time.time() + timeout
    while time.time() < deadline:
        chunk = fd.read() or b""
        handle["err_buf"] += chunk
        if needle.encode() in handle["err_buf"] or handle["proc"].poll() is not None:
            break
        time.sleep(0.1)
    os.set_blocking(fd.fileno(), True)
    return handle


def _finish(r, handle, sig=None, timeout=60):
    step = r.finish(handle, sig=sig, timeout=timeout, compare=False)
    combined = _Step(step.label, step.argv, step.code, step.out, handle.get("err_buf", b"") + step.err)
    r.steps.append(combined)
    return combined


def _run_pty_stderr(r, label, args, profile):
    """Run a step with stderr on a pseudo-terminal and stdout on a pipe."""
    import pty
    master, slave = pty.openpty()
    proc = subprocess.Popen([_binary(r), *args], stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                            stderr=slave, env=r.env(profile), start_new_session=True)
    os.close(slave)
    err = b""
    while True:
        try:
            chunk = os.read(master, 4096)
        except OSError:
            break
        if not chunk:
            break
        err += chunk
    os.close(master)
    out = proc.stdout.read()
    proc.wait(timeout=60)
    # Show OSC 52 payloads decoded so per-run paths can be normalized.
    err = re.sub(rb"\x1b\]52;c;([A-Za-z0-9+/=]*)\x07",
                 lambda m: b"<OSC52 " + base64.b64decode(m.group(1)) + b">", err)
    step = _Step(label + " (stderr on a pty)", args, proc.returncode, out, err)
    r.steps.append(step)
    return step


def _entry_ids(r, profile):
    listed = r.run("fixture: list ids", ["--json", "get", "--list"], cli="rust", profile=profile, compare=False)
    return [row["entry_id"] for row in json.loads(listed.out)]


@scenario
def receive_without_space(r):
    """No space on the profile. Spawning the oneshot daemon refuses; while
    that daemon idles, later commands reuse it (no setup check on reuse)."""
    r.run("get", ["get"])
    r.run("get --json", ["--json", "get"])
    r.run("get --list", ["get", "--list"])
    h = _spawn_ready(r, "get --wait then Ctrl-C", ["get", "--wait"], "Waiting for the next", r.profile)
    _finish(r, h, sig=signal.SIGINT)
    h = _spawn_ready(r, "watch then Ctrl-C", ["watch"], "WATCH_READY", r.profile)
    _finish(r, h, sig=signal.SIGINT)
    out = os.path.join(r.home, "recv-out", "nested")
    h = _spawn_ready(r, "recv creates out dir, then Ctrl-C", ["recv", "-o", out], "Waiting for incoming file",
                     r.profile)
    _finish(r, h, sig=signal.SIGINT)
    r.note("recv out dir created", os.path.isdir(out))
    blocker = os.path.join(r.home, "blocker")
    with open(blocker, "w") as fh:
        fh.write("x")
    r.run("recv out is a file", ["recv", "-o", blocker])
    r.run("recv out under a file", ["--json", "recv", "--out", os.path.join(blocker, "sub")])


@scenario
def get_empty_history(r):
    """Initialized profile with no entries (oneshot daemon per command)."""
    init_space(r)
    r.run("get", ["get"])
    r.run("fixture: start", ["start"], cli="rust", compare=False)
    r.run("get --json", ["--json", "get"])
    r.run("get --list", ["get", "--list"])
    r.run("get --list --json", ["--json", "get", "--list"])
    r.run("get --list -n 0", ["get", "--list", "-n", "0"])
    for kind in ("image", "file", "text", "link"):
        r.run("get --type " + kind, ["get", "--type", kind])
    r.run("get --id unknown", ["get", "--id", "0123456789abcdef"])
    r.run("get --id short", ["get", "--id", "abc"])
    r.run("get --id unicode", ["get", "--id", "条目条目条目条目条目"])
    r.run("get --copy no match", ["get", "--copy"])
    r.run("get -o - no match", ["get", "-o", "-"])
    # Interrupted waits on a quiet profile.
    h = _spawn_ready(r, "get --wait then Ctrl-C", ["get", "--wait"], "Waiting for the next", r.profile)
    _finish(r, h, sig=signal.SIGINT)
    h = _spawn_ready(r, "get --wait --json then Ctrl-C", ["--json", "get", "--wait"], None, r.profile)
    time.sleep(4)  # --json prints no readiness line
    _finish(r, h, sig=signal.SIGINT)
    h = _spawn_ready(r, "watch then Ctrl-C", ["watch"], "WATCH_READY", r.profile)
    _finish(r, h, sig=signal.SIGINT)
    h = _spawn_ready(r, "watch --json then Ctrl-C", ["--json", "watch"], "WATCH_READY", r.profile)
    _finish(r, h, sig=signal.SIGINT)
    out = os.path.join(r.home, "recv-out")
    h = _spawn_ready(r, "recv then Ctrl-C", ["recv", "-o", out], "Waiting for incoming file", r.profile)
    _finish(r, h, sig=signal.SIGINT)
    h = _spawn_ready(r, "recv --json then Ctrl-C", ["--json", "recv", "-o", out], None, r.profile)
    time.sleep(4)
    _finish(r, h, sig=signal.SIGINT)


def _populate_history(r, sponsor):
    """Fixture: text, link, a multi-line text, a small file, a PNG file and a
    5 MB file, sent from the sponsor. Returns {name: path} of sent files."""
    files = {}
    _send(r, sponsor, ["--text", "first text entry"], "text 1")
    _send(r, sponsor, ["--text", "https://example.com/some/path?q=1"], "link")
    sample = os.path.join(r.home, "sample.txt")
    with open(sample, "w") as fh:
        fh.write("file body\n")
    files["sample.txt"] = sample
    _send(r, sponsor, ["--file", sample], "file")
    png = os.path.join(r.home, "pixel.png")
    with open(png, "wb") as fh:
        fh.write(bytes.fromhex("89504e470d0a1a0a0000000d4948445200000001000000010806000000"
                               "1f15c4890000000d49444154789c6360000002000154a24f5d0000000049454e44ae426082"))
    files["pixel.png"] = png
    _send(r, sponsor, ["--file", png], "png")
    big = os.path.join(r.home, "big.bin")
    with open(big, "wb") as fh:
        fh.write(random.Random(42).randbytes(5 * 1024 * 1024 + 123))
    files["big.bin"] = big
    _send(r, sponsor, ["--file", big], "big file")
    _send(r, sponsor, ["--text", "line one of a long multi-line entry that exceeds forty-eight chars\r\nline two"],
          "multi-line text")
    time.sleep(3)
    return files


@scenario
def get_history(r):
    """Selection, materialization and output targets on a populated history."""
    sponsor, joiner = pair(r)
    r.run("fixture: start joiner", ["start"], cli="rust", profile=joiner, compare=False)
    files = _populate_history(r, sponsor)
    ids = _entry_ids(r, joiner)

    r.run("list", ["get", "--list"], profile=joiner)
    r.run("list --json", ["--json", "get", "--list"], profile=joiner)
    r.run("list -n 2", ["get", "--list", "-n", "2"], profile=joiner)
    r.run("list --limit 0 --json", ["--json", "get", "--list", "--limit", "0"], profile=joiner)
    r.run("newest", ["get"], profile=joiner)
    r.run("newest --json", ["--json", "get"], profile=joiner)
    r.run("type text", ["get", "--type", "text"], profile=joiner)
    r.run("type link", ["get", "--type", "link"], profile=joiner)
    r.run("type link --json", ["--json", "get", "--type", "link"], profile=joiner)
    r.run("type image", ["get", "--type", "image"], profile=joiner)
    r.run("type image --json", ["--json", "get", "--type", "image"], profile=joiner)
    r.run("type link -n 1", ["get", "--type", "link", "-n", "1"], profile=joiner)

    _cooldown()
    out = os.path.join(r.home, "out")
    r.run("type file to new dir", ["get", "--type", "file", "-o", out], profile=joiner)
    r.run("type file to dir --json", ["--json", "get", "--type", "file", "--out", out], profile=joiner)
    r.note("out dir", _listing(out))
    r.run("type file default cache dir", ["get", "--type", "file"], profile=joiner)
    r.note("cache dir", _listing(os.path.join(r.home, ".cache", "uniclip", "get")))

    step = r.run("type file to stdout", ["get", "--type", "file", "-o", "-"], profile=joiner, compare=False)
    r.note("type file to stdout", {"exit": step.code, "stderr": step.err.decode(), "size": len(step.out),
                                   "matches big.bin": _sha(step.out) == _file_sha(files["big.bin"])})
    r.run("type file to stdout with --json", ["--json", "get", "--type", "file", "-o", "-"], profile=joiner)
    r.run("type file to stdout with --copy", ["get", "--type", "file", "-o", "-", "--copy"], profile=joiner)

    _cooldown()
    # Each file entry by id, to a directory and to stdout.
    for index, entry_id in enumerate(ids):
        r.run(f"id #{index}", ["get", "--id", entry_id, "-o", out], profile=joiner, compare=False)
        step = r.run(f"id #{index} to stdout", ["get", "--id", entry_id, "-o", "-"], profile=joiner, compare=False)
        r.note(f"id #{index} to stdout", {"exit": step.code, "stderr": step.err.decode(), "size": len(step.out),
                                          "sha256": _sha(step.out)})
        r.run(f"id #{index} --json", ["--json", "get", "--id", entry_id, "-o", out], profile=joiner)
        if index == 2:
            _cooldown()
    _cooldown()
    r.note("out dir after ids", _listing(out))
    r.run("id beyond limit", ["get", "--id", ids[-1], "-n", "2"], profile=joiner)
    r.run("id unknown", ["get", "--id", "no-such-entry-id"], profile=joiner)

    # Output directory failures.
    blocker = os.path.join(r.home, "blocker")
    with open(blocker, "w") as fh:
        fh.write("x")
    r.run("out is a file", ["get", "--type", "file", "-o", blocker], profile=joiner)
    r.run("out under a file", ["get", "--type", "file", "-o", os.path.join(blocker, "sub")], profile=joiner)
    clash = os.path.join(r.home, "clash")
    os.makedirs(os.path.join(clash, "big.bin"))
    r.run("target name is a directory", ["get", "--type", "file", "-o", clash], profile=joiner)
    r.run("text ignores --out", ["get", "--type", "text", "-o", blocker], profile=joiner)

    _cooldown()
    # --copy without a terminal, then OSC 52 on a terminal.
    r.run("copy text without a terminal", ["get", "--type", "text", "--copy"], profile=joiner)
    r.run("copy file without a terminal", ["get", "--type", "file", "-o", os.path.join(r.home, "copy-out"), "-c"],
          profile=joiner)
    r.note("file written before copy failed", _listing(os.path.join(r.home, "copy-out")))
    _run_pty_stderr(r, "copy text", ["get", "--type", "text", "--copy"], joiner)
    _run_pty_stderr(r, "copy link --json", ["--json", "get", "--type", "link", "--copy"], joiner)
    _run_pty_stderr(r, "copy file path", ["get", "--type", "file", "-o", os.path.join(r.home, "pty-out"), "--copy"],
                    joiner)
    _run_pty_stderr(r, "list on a pty", ["get", "--list", "-n", "3"], joiner)

    _cooldown()
    # A reader that closes early.
    proc = subprocess.Popen([_binary(r), "get", "--type", "file", "-o", "-"], stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, env=r.env(joiner))
    head = proc.stdout.read(10)
    proc.stdout.close()
    err = proc.stderr.read()
    proc.wait(timeout=60)
    r.note("stdout closed early", {"exit": proc.returncode, "stderr": err.decode(), "head": head.hex()})

    # The sender's own local file entry.
    r.run("sponsor type file", ["get", "--type", "file", "-o", os.path.join(r.home, "sponsor-out")], profile=sponsor)
    r.run("sponsor list", ["get", "--list"], profile=sponsor)
    _mask_volatile(r)


@scenario
def get_wait_remote(r):
    """`get --wait` against remote sends (oneshot daemon on the joiner)."""
    sponsor, joiner = pair(r)
    # Stop the daemon left by the join so the first wait spawns its own.
    r.run("fixture: stop joiner", ["stop"], cli="rust", profile=joiner, compare=False)
    h = _spawn_ready(r, "wait for text", ["get", "--wait"], "Waiting for the next", joiner)
    _send(r, sponsor, ["--text", "waited text"], "text")
    _finish(r, h)
    # The first wait spawned a oneshot daemon. Whether a later command still
    # finds it depends on its idle exit timing, so use a persistent daemon.
    r.run("fixture: start joiner", ["start"], cli="rust", profile=joiner, compare=False)

    out = os.path.join(r.home, "wait-out")
    h = _spawn_ready(r, "wait for a file, skipping text", ["get", "--wait", "--type", "file", "-o", out],
                     "Waiting for the next", joiner)
    _send(r, sponsor, ["--text", "not a file"], "skipped text")
    sample = os.path.join(r.home, "waited.txt")
    with open(sample, "w") as fh:
        fh.write("waited file body\n")
    _send(r, sponsor, ["--file", sample], "file")
    _finish(r, h)
    r.note("wait out dir", _listing(out))

    h = _spawn_ready(r, "wait --json for a link", ["--json", "get", "-w"], None, joiner)
    time.sleep(6)  # --json prints no readiness line
    _send(r, sponsor, ["--text", "https://example.org/waited"], "link")
    _finish(r, h)

    h = _spawn_ready(r, "wait for a big file to stdout", ["get", "--wait", "-o", "-"], "Waiting for the next", joiner)
    big = os.path.join(r.home, "big.bin")
    with open(big, "wb") as fh:
        fh.write(random.Random(7).randbytes(3 * 1024 * 1024))
    _send(r, sponsor, ["--file", big], "big file")
    step = r.finish(h, compare=False)
    r.note("wait big file to stdout", {"exit": step.code, "stderr": (h["err_buf"] + step.err).decode(),
                                       "matches source": _sha(step.out) == _file_sha(big)})

    h = _spawn_ready(r, "wait --type image skips a file", ["get", "--wait", "--type", "image"],
                     "Waiting for the next", joiner)
    other = os.path.join(r.home, "other.txt")
    with open(other, "w") as fh:
        fh.write("another file body\n")
    _send(r, sponsor, ["--file", other], "another file")
    time.sleep(3)
    _finish(r, h, sig=signal.SIGINT)

    h = _spawn_ready(r, "wait --id unknown skips text", ["get", "--wait", "--id", "nope"], "Waiting for the next",
                     joiner)
    _send(r, sponsor, ["--text", "id filter text"], "text for id filter")
    time.sleep(3)
    _finish(r, h, sig=signal.SIGINT)
    _mask_volatile(r)


@scenario
def watch_remote(r):
    """`watch` renders text and file deliveries until Ctrl-C."""
    sponsor, joiner = pair(r)
    h = _spawn_ready(r, "watch", ["watch"], "WATCH_READY", joiner)
    _send(r, sponsor, ["--text", "watched text\nsecond line"], "text")
    sample = os.path.join(r.home, "watched.txt")
    with open(sample, "w") as fh:
        fh.write("watched file\n")
    _send(r, sponsor, ["--file", sample], "file")
    _send(r, sponsor, ["--text", "x" * 130], "long text")
    time.sleep(3)
    _finish(r, h, sig=signal.SIGINT)

    h = _spawn_ready(r, "watch --json", ["--json", "watch"], "WATCH_READY", joiner)
    _send(r, sponsor, ["--text", "json watched <text> & \"quotes\""], "json text")
    other = os.path.join(r.home, "watched-json.txt")
    with open(other, "w") as fh:
        fh.write("watched json file\n")
    _send(r, sponsor, ["--file", other], "json file")
    time.sleep(3)
    _finish(r, h, sig=signal.SIGINT)
    _mask_volatile(r)


@scenario
def recv_remote(r):
    """Deprecated `recv`: skips text, saves the first file, then exits."""
    sponsor, joiner = pair(r)
    out = os.path.join(r.home, "recv-out")
    settle_oneshot(r, profile=joiner)
    h = _spawn_ready(r, "recv", ["recv", "-o", out], "Waiting for incoming file", joiner)
    _send(r, sponsor, ["--text", "not a file"], "text")
    sample = os.path.join(r.home, "received.txt")
    with open(sample, "w") as fh:
        fh.write("received body\n")
    _send(r, sponsor, ["--file", sample], "file")
    _finish(r, h)
    r.note("recv out dir", _listing(out))

    settle_oneshot(r, profile=joiner)
    h = _spawn_ready(r, "recv --json", ["--json", "recv", "--out", out], None, joiner)
    time.sleep(6)  # --json prints no readiness line
    big = os.path.join(r.home, "recv-big.bin")
    with open(big, "wb") as fh:
        fh.write(random.Random(9).randbytes(2 * 1024 * 1024))
    _send(r, sponsor, ["--file", big], "big file")
    _finish(r, h)
    r.note("recv out dir after json", _listing(out))
    _mask_volatile(r)


_LOST = " ⚠  Daemon connection lost — reconnecting...\n"
_RECONNECTED = " ⚠  Reconnected — events during daemon restart may have been missed\n"
_NOT_READY = "WS handshake failed: HTTP error: 503 Service Unavailable"


def _reconnect_outcome(r, label, step, branches):
    """The restarted daemon passes the health probe the reconnect waits for
    before its WebSocket stops answering 503, so each run (in either flavor)
    ends in one of two outcomes. Record which exact outcome text matched
    instead of the raw step, which would flip between runs."""
    err = re.sub(r"\[[0-9a-f-]{36}\]", "[<UUID>]", step.err.decode(errors="replace"))
    observed = (step.code, step.out.decode(errors="replace"), err)
    r.note(label, {"matches an expected outcome": observed in branches.values(),
                   "observed": None if observed in branches.values() else list(observed)})


@scenario
def get_wait_reconnect(r):
    """`get --wait` and `watch` across a daemon restart."""
    sponsor, joiner = pair(r)
    r.run("fixture: start joiner", ["start"], cli="rust", profile=joiner, compare=False)
    h = _spawn_ready(r, "wait across a restart", ["get", "--wait"], "Waiting for the next", joiner)
    stop = r.run("fixture: stop joiner", ["stop"], cli="rust", profile=joiner, compare=False)
    r.note("stop while waiting", {"exit": stop.code})
    time.sleep(1)
    r.run("fixture: restart joiner", ["start"], cli="rust", profile=joiner, compare=False)
    time.sleep(3)
    _send(r, sponsor, ["--text", "after restart"], "text")
    step = r.finish(h, compare=False)
    step.err = h["err_buf"] + step.err
    waiting = " │  status: Waiting for the next synced entry — press Ctrl-C to stop\n"
    _reconnect_outcome(r, "wait across a restart", step, {
        "reconnected": (0, "after restart", waiting + _LOST + _RECONNECTED),
        "daemon not ready": (1, "", waiting + _LOST
                             + f" ✗  Failed to re-acquire lease after reconnect: {_NOT_READY}\n"),
    })

    h = _spawn_ready(r, "watch across a restart", ["watch"], "WATCH_READY", joiner)
    r.run("fixture: stop joiner again", ["stop"], cli="rust", profile=joiner, compare=False)
    time.sleep(1)
    r.run("fixture: restart joiner again", ["start"], cli="rust", profile=joiner, compare=False)
    time.sleep(3)
    _send(r, sponsor, ["--text", "watched after restart"], "text 2")
    time.sleep(3)
    step = r.finish(h, sig=signal.SIGINT, compare=False)
    step.err = h["err_buf"] + step.err
    ready = ("\n ◆  Watch inbound clipboard\n ✓  Subscribed via daemon WS\n"
             " │  status: Listening via daemon — press Ctrl-C to stop\n │\nWATCH_READY\n")
    _reconnect_outcome(r, "watch across a restart", step, {
        "reconnected": (0, "", ready + _LOST + _RECONNECTED
                        + " │  ·: [<UUID>] watched after restart (new_entry)\n └  Stopped\n"),
        "daemon not ready": (1, "", ready + _LOST + f" ✗  Failed to re-subscribe after reconnect: {_NOT_READY}\n"),
    })
