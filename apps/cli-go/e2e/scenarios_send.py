"""Differential scenarios for `uniclip send`."""
import json
import os
import pty
import signal
import subprocess
import time

from scenarios import scenario, pair, init_space


def _fixture_files(r):
    """Create send fixtures under the scenario HOME (normalized to <HOME>)."""
    root = os.path.join(r.home, "files")
    os.makedirs(os.path.join(root, "folder"), exist_ok=True)
    files = {
        "one": ("one file.txt", b"first file payload\n"),
        "two": ("two.txt", b"second file payload\n"),
        "bin": ("blob.bin", bytes(range(256)) * 8),
        "locked": ("locked.txt", b"secret"),
    }
    paths = {}
    for key, (name, data) in files.items():
        path = os.path.join(root, name)
        with open(path, "wb") as fh:
            fh.write(data)
        paths[key] = path
    os.chmod(paths["locked"], 0)
    paths["dir"] = os.path.join(root, "folder")
    paths["root"] = root
    return paths


def _json(step):
    try:
        return json.loads(step.out)
    except ValueError:
        return None


def _run_with_tty_stdin(r, label, args):
    """Run the flavor's CLI with a pseudo-terminal as stdin only."""
    binary = os.path.join(r.bindir, "uniclip")
    master, slave = pty.openpty()
    try:
        proc = subprocess.run([binary, *args], stdin=slave, capture_output=True, timeout=60, env=r.env())
    finally:
        os.close(slave)
        os.close(master)
    r.note(label, {"exit": proc.returncode, "stdout": proc.stdout.decode(), "stderr": proc.stderr.decode()})


def _list_entries(r, profile):
    step = r.run("probe: list", ["--json", "get", "--list", "--limit", "20"], cli="rust", profile=profile,
                 compare=False)
    return _json(step) or []


@scenario
def send_input_errors(r):
    """Input classification failures are reported before any daemon contact."""
    f = _fixture_files(r)
    r.run("empty stdin", ["send"], stdin=b"")
    r.run("empty stdin (json)", ["--json", "send"], stdin=b"")
    r.run("newline-only stdin", ["send"], stdin=b"\r\n")
    r.run("invalid utf-8 stdin", ["send"], stdin=b"ok\xff\n")
    r.run("empty positional", ["send", ""], stdin=b"")
    r.run("--text empty positional", ["send", "--text", ""], stdin=b"")
    r.run("missing relative path", ["send", "./missing.pdf"], stdin=b"")
    r.run("missing absolute path", ["send", os.path.join(f["root"], "missing.bin")], stdin=b"")
    r.run("missing path (json)", ["--json", "send", "../missing.pdf"], stdin=b"")
    r.run("directory", ["send", f["dir"]], stdin=b"")
    r.run("directory (json)", ["--json", "send", f["dir"]], stdin=b"")
    r.run("--file directory", ["send", "--file", f["dir"]], stdin=b"")
    r.run("--file missing", ["send", "--file", "missing.txt"], stdin=b"")
    r.run("-f missing absolute", ["send", "-f", os.path.join(f["root"], "nope.txt")], stdin=b"")
    r.run("not a regular file", ["send", "/dev/null"], stdin=b"")
    r.run("--file not a regular file", ["send", "--file", "/dev/null"], stdin=b"")
    r.run("unreadable file", ["send", f["locked"]], stdin=b"")
    r.run("--file unreadable file", ["send", "--file", f["locked"]], stdin=b"")
    r.run("path under a file", ["send", f["one"] + "/child"], stdin=b"")
    r.run("--file empty stdin", ["send", "--file"], stdin=b"")
    r.run("--file blank stdin lines", ["send", "--file"], stdin=b"\n\r\n\n")
    r.run("--file invalid utf-8 stdin", ["send", "--file"], stdin=b"\xfe\n")
    r.run("--file stdin with a missing path", ["send", "--file"],
          stdin=(f["one"] + "\n" + os.path.join(f["root"], "missing.txt") + "\n").encode())
    r.run("--file stdin with a directory", ["--json", "send", "--file"], stdin=(f["dir"] + "\n").encode())
    r.run("--file stdin with an unreadable file", ["send", "-f"], stdin=(f["two"] + "\r\n" + f["locked"]).encode())
    r.run("--file stdin backslash path", ["send", "-f"], stdin=b"C:\\Users\\Example\\file name.txt\r\n")
    _run_with_tty_stdin(r, "--file with terminal stdin", ["send", "--file"])
    _run_with_tty_stdin(r, "--file with terminal stdin (json)", ["--json", "send", "-f"])


@scenario
def send_without_space(r):
    """A spawned oneshot daemon on a profile without a space refuses sends."""
    r.run("text", ["send", "hello"], stdin=b"")
    r.run("text (json)", ["--json", "send", "hello"], stdin=b"")
    r.run("stdin text", ["send"], stdin=b"from stdin\n")
    r.run("resend", ["send", "--resend", "entry-1"], stdin=b"")
    r.run("connect-timeout 0", ["send", "--connect-timeout", "0", "hello"], stdin=b"")


@scenario
def send_without_peers(r):
    """A space with no members: every send mode reports zero targets."""
    init_space(r)
    f = _fixture_files(r)
    # First send spawns a oneshot daemon; keep one running for the rest.
    r.run("text via oneshot daemon", ["send", "hello"], stdin=b"")
    r.run("fixture: start", ["start"], cli="rust", compare=False)
    r.run("text", ["send", "hello"], stdin=b"")
    r.run("text (json)", ["--json", "send", "hello world"], stdin=b"")
    r.run("bare word is text when no such file", ["--json", "send", "missing.txt"], stdin=b"")
    r.run("--text with an existing path", ["--json", "send", "--text", "/etc/hosts"], stdin=b"")
    r.run("stdin text", ["send"], stdin=b"line one\nline two\r\n")
    r.run("stdin text (json)", ["--json", "send"], stdin="unicode ✓ 你好\n".encode())
    r.run("large stdin text", ["--json", "send"], stdin=b"x" * (2 * 1024 * 1024))
    r.run("connect-timeout 0", ["send", "--connect-timeout", "0", "hello"], stdin=b"")
    r.run("unknown peer, connect-timeout 0", ["send", "--connect-timeout", "0", "--peer", "nobody", "hi"], stdin=b"")
    r.run("unknown peer, connect-timeout 0 (json)",
          ["--json", "send", "--connect-timeout", "0", "--peer", "nobody", "--peer", "other", "hi"], stdin=b"")
    r.run("unknown peer, short timeout", ["send", "--connect-timeout", "1", "--peer", "nobody", "hi"], stdin=b"")
    r.run("unknown peers, short timeout (json)",
          ["--json", "send", "--connect-timeout", "2", "--peer", "a", "--peer", "b", "hi"], stdin=b"")
    r.run("file", ["send", f["one"]], stdin=b"")
    step = r.run("file (json)", ["--json", "send", f["bin"]], stdin=b"")
    r.run("files via stdin (json)", ["--json", "send", "--file"],
          stdin=(f["one"] + "\n\n" + f["two"] + "\r\n" + f["one"] + "\n").encode())
    r.run("one file via stdin (json)", ["--json", "send", "--file"], stdin=(f["two"] + "\n").encode())
    r.run("files via stdin", ["send", "-f"], stdin=(f["one"] + "\n" + f["bin"] + "\n").encode())
    entry = (_json(step) or {}).get("entryId", "missing-entry")
    r.run("resend existing entry", ["send", "--resend", entry], stdin=b"")
    r.run("resend existing entry (json)", ["--json", "send", "--resend", entry], stdin=b"")
    r.run("resend existing entry to unknown peer",
          ["--json", "send", "--resend", entry, "--peer", "nobody"], stdin=b"")
    r.run("resend unknown entry", ["send", "--resend", "00000000-0000-0000-0000-000000000000"], stdin=b"")
    r.run("resend unknown entry (json)", ["--json", "send", "--resend", "not-an-entry"], stdin=b"")
    r.run("resend path-like id", ["send", "--resend", "a/b c"], stdin=b"")


@scenario
def send_to_paired_peer(r):
    """Real deliveries to a paired, online peer, verified on the receiver."""
    sponsor, joiner = pair(r)
    f = _fixture_files(r)
    r.run("fixture: start joiner", ["start"], cli="rust", profile=joiner, compare=False)
    r.run("fixture: start sponsor", ["start"], cli="rust", profile=sponsor, compare=False)
    members = _json(r.run("probe: members", ["--json", "member", "list"], cli="rust", profile=sponsor,
                          compare=False)) or []
    joiner_id = next((m.get("device_id") for m in members if isinstance(m, dict) and not m.get("is_local")
                      and m.get("device_id")), "unknown-joiner")

    r.run("text", ["send", "--connect-timeout", "60", "hello from sender"], stdin=b"")
    r.run("text to listed peer (json)", ["--json", "send", "--connect-timeout", "60", "--peer", joiner_id,
                                         "listed peer text"], stdin=b"")
    r.run("stdin text (json)", ["--json", "send", "--connect-timeout", "60"], stdin="multi\nline ✓\n".encode())
    r.run("duplicate text", ["send", "--connect-timeout", "60", "hello from sender"], stdin=b"")
    r.run("file", ["send", "--connect-timeout", "60", f["one"]], stdin=b"")
    r.run("binary file (json)", ["--json", "send", "--connect-timeout", "60", f["bin"]], stdin=b"")
    r.run("files via stdin (json)", ["--json", "send", "--connect-timeout", "60", "--file"],
          stdin=(f["one"] + "\n" + f["two"] + "\n").encode())
    r.run("files via stdin", ["send", "--connect-timeout", "60", "-f"], stdin=(f["two"] + "\n" + f["bin"]).encode())

    time.sleep(3)
    received = _list_entries(r, joiner)
    r.note("receiver entries", [{k: e.get(k) for k in ("content_type", "preview", "lost")} for e in received])
    for index, entry in enumerate(received):
        args = ["get", "--id", entry["entry_id"]]
        if entry.get("content_type") != "text":
            args += ["--out", "-"]
        got = r.run("probe: get", args, cli="rust", profile=joiner, compare=False)
        r.note(f"receiver entry {index}", {"exit": got.code, "bytes": got.out.hex() if got.code == 0 else None})

    sent = _list_entries(r, sponsor)
    text_entry = next((e["entry_id"] for e in sent if e.get("content_type") == "text"), "missing")
    r.run("resend existing entry", ["send", "--resend", text_entry], stdin=b"")
    r.run("resend existing entry to peer (json)", ["--json", "send", "--resend", text_entry, "--peer", joiner_id],
          stdin=b"")
    remote_entry = received[0]["entry_id"] if received else "missing"
    r.run("resend remote-origin entry on receiver", ["send", "--resend", remote_entry], profile=joiner, stdin=b"")
    r.run("resend remote-origin entry on receiver (json)", ["--json", "send", "--resend", remote_entry],
          profile=joiner, stdin=b"")

    # Receiver goes offline.
    r.run("fixture: stop joiner", ["stop"], cli="rust", profile=joiner, compare=False)
    time.sleep(3)
    r.run("offline, connect-timeout 0", ["send", "--connect-timeout", "0", "while offline"], stdin=b"")
    r.run("offline, connect-timeout 0 (json)", ["--json", "send", "--connect-timeout", "0", "while offline 2"],
          stdin=b"")
    r.run("offline, short timeout", ["send", "--connect-timeout", "2", "never sent"], stdin=b"")
    r.run("offline listed peer, short timeout (json)",
          ["--json", "send", "--connect-timeout", "1", "--peer", joiner_id, "--peer", "nobody", "never"], stdin=b"")
    r.run("offline file, connect-timeout 0", ["send", "--connect-timeout", "0", f["two"]], stdin=b"")
    r.run("offline file, connect-timeout 0 (json)", ["--json", "send", "--connect-timeout", "0", "-f", f["one"]],
          stdin=b"")
    r.run("offline resend", ["send", "--resend", text_entry], stdin=b"")
    r.run("offline resend (json)", ["--json", "send", "--resend", text_entry], stdin=b"")
    h = r.spawn("Ctrl-C while waiting for the peer", ["send", "--connect-timeout", "60", "interrupted"], stdin=b"")
    time.sleep(4)
    r.finish(h, sig=signal.SIGINT)
    after = _list_entries(r, sponsor)
    r.note("sender history after offline sends", [{k: e.get(k) for k in ("content_type", "preview")} for e in after])
