"""Scenarios for the space command group: init, invite, join (status/cancel,
--switch, --no-wait), reset, change-passphrase, and the legacy top-level
aliases. Pairing scenarios need the production rendezvous service."""
import json
import os
import pty
import re
import select
import signal
import subprocess
import time

from scenarios import scenario, init_space, PASSPHRASE

CODE_RE = re.compile(r"\b\d{3}-\d{3}\b")
ANSI_RE = re.compile(r"\x1b\[[0-9;?]*[A-Za-z]")
WRONG_PASSPHRASE = "wrong-passphrase-123"
NEW_PASSPHRASE = "brand-new-passphrase-42"


def _text(data):
    return data.decode("utf-8", "replace") if isinstance(data, bytes) else data


JOIN_ID_RE = re.compile(r"(join_id\"?:\s*\"?)[A-Za-z0-9_-]{22}")


def _strip_code(text):
    """Normalize invitation codes and random join ids."""
    return JOIN_ID_RE.sub(r"\1<JOIN_ID>", CODE_RE.sub("<CODE>", _text(text)))


def run(r, label, args, **kw):
    """r.run with invitation codes and join ids normalized in the record."""
    step = r.run(label, args, **kw)
    step.out, step.err = _strip_code(step.out).encode(), _strip_code(step.err).encode()
    return step


def _stop_daemon(r, profile=None):
    """Fixture: stop a lingering oneshot daemon so the next step always
    spawns its own (keeps the `Local daemon ready` line deterministic)."""
    r.run("fixture: stop", ["stop"], cli="rust", profile=profile, compare=False)


def _wait_join_settled(r, profile=None, timeout=150):
    """Fixture: poll (with the Rust CLI) until the current join is no longer pending."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        step = r.run("fixture: join status", ["--json", "space", "join", "status"], cli="rust", profile=profile,
                     compare=False)
        try:
            if json.loads(step.out).get("status") not in ("pending", "processing"):
                return
        except ValueError:
            pass
        time.sleep(1)
    raise RuntimeError("join never settled")


def _start_invite(r, profile, cli, json_mode=False, timeout=90):
    """Spawn `space invite` and wait for its invitation code."""
    args = (["--json"] if json_mode else []) + ["space", "invite"]
    handle = r.spawn("invite", args, profile=profile, cli=cli)
    fd = handle["proc"].stdout
    os.set_blocking(fd.fileno(), False)
    buf, code, deadline = b"", None, time.time() + timeout
    while time.time() < deadline and code is None:
        buf += fd.read() or b""
        for line in buf.decode(errors="replace").splitlines():
            if line.startswith("INVITATION_CODE="):
                code = line.split("=", 1)[1].strip()
            elif line.startswith("{"):
                try:
                    event = json.loads(line)
                except ValueError:
                    continue
                if event.get("event") == "invitation_issued":
                    code = event["code"]
        if handle["proc"].poll() is not None and code is None:
            break
        time.sleep(0.3)
    os.set_blocking(fd.fileno(), True)
    handle["prefix"] = buf
    if code is None:
        raise RuntimeError("no invitation code: " + buf.decode(errors="replace"))
    return handle, code


def _finish_invite(r, handle, label):
    """Interrupt a running invite and record its normalized transcript."""
    step = r.finish(handle, sig=signal.SIGINT, timeout=30, compare=False)
    r.note(label, {"exit": step.code, "stdout": _strip_code(handle["prefix"] + step.out),
                   "stderr": _strip_code(step.err)})


def _wait_members(r, profile, count=2, timeout=90):
    deadline, members = time.time() + timeout, []
    while time.time() < deadline:
        listed = r.run("fixture: member list", ["--json", "member", "list"], cli="rust", profile=profile,
                       compare=False)
        try:
            members = json.loads(listed.out)
        except ValueError:
            members = []
        if len(members) >= count:
            return members
        time.sleep(2)
    raise RuntimeError(f"member count never reached {count}: {members!r}")


def _pty_run(r, label, args, answers=(), profile=None, timeout=60, cli="self"):
    """Run a step with stdin/stderr on a pseudo-terminal (stdout stays a
    pipe). `answers` are (prompt-substring, bytes) pairs typed in order once
    the prompt shows. Records the cleaned terminal transcript."""
    binary = os.path.join(r.rust_dir if cli == "rust" else r.bindir, "uniclip")
    master, slave = pty.openpty()
    proc = subprocess.Popen([binary, *args], stdin=slave, stdout=subprocess.PIPE, stderr=slave,
                            env=r.env(profile), start_new_session=True)
    os.close(slave)
    raw, pending, seen, deadline = b"", list(answers), 0, time.time() + timeout
    while time.time() < deadline:
        ready, _, _ = select.select([master], [], [], 0.2)
        if ready:
            try:
                chunk = os.read(master, 4096)
            except OSError:
                chunk = b""
            if not chunk and proc.poll() is not None:
                break
            raw += chunk
        elif proc.poll() is not None:
            break
        if pending and pending[0][0].encode() in raw[seen:]:
            # Let the prompt switch the terminal to raw mode before typing,
            # draining output meanwhile so a TCSADRAIN mode switch completes.
            raw += _drain(master, 1.2)
            # Type one key at a time and keep draining the terminal like a
            # real emulator: the Rust prompt switches raw mode per key with
            # TCSADRAIN, so undrained output would leave the tty in cooked
            # (echoing) mode while the next key arrives.
            for byte in pending.pop(0)[1]:
                os.write(master, bytes([byte]))
                raw += _drain(master, 0.1)
            seen = len(raw)
    if proc.poll() is None:
        proc.kill()
    out = proc.stdout.read()
    proc.wait()
    os.close(master)
    r.note(label, {"exit": proc.returncode if proc.returncode is not None else "timeout",
                   "stdout": _strip_code(out), "terminal": _clean_terminal(raw),
                   "answers_left": len(pending)})


def _drain(master, duration):
    """Read whatever the terminal prints during `duration` seconds."""
    data, end = b"", time.time() + duration
    while time.time() < end:
        ready, _, _ = select.select([master], [], [], max(end - time.time(), 0))
        if not ready:
            break
        try:
            chunk = os.read(master, 4096)
        except OSError:
            break
        if not chunk:
            break
        data += chunk
    return data


def _clean_terminal(raw):
    """Collapse a terminal transcript to its final visible lines: strip
    escape sequences, keep the text after the last carriage return on each
    line, and drop transient spinner frames."""
    text = ANSI_RE.sub("", _text(raw)).replace("\r\n", "\n")
    lines = []
    for line in text.split("\n"):
        line = line.split("\r")[-1]
        if re.match(r"^ [◒◐◓◑]  ", line):
            continue
        lines.append(line)
    return _strip_code("\n".join(lines))


# ---------------------------------------------------------------- init


@scenario
def space_init_flows(r):
    """init input validation, human and JSON success, then already-initialized."""
    run(r, "init empty passphrase", ["space", "init", "--passphrase", "", "--device-name", "a"])
    run(r, "init blank passphrase json", ["--json", "space", "init", "--passphrase", "  "])
    run(r, "init json without passphrase", ["--json", "space", "init", "--device-name", "a"])
    run(r, "init human", ["space", "init", "--passphrase", PASSPHRASE, "--device-name", "compat-a"])
    run(r, "init again human", ["space", "init", "--passphrase", PASSPHRASE, "--device-name", "compat-a"])
    run(r, "init again json", ["--json", "space", "init", "--passphrase", PASSPHRASE, "--device-name", "compat-a"])
    run(r, "legacy init again", ["init", "--passphrase", PASSPHRASE, "--device-name", "compat-a"])
    run(r, "status after init", ["--json", "space", "status"], cli="rust")


@scenario
def space_init_json(r):
    """JSON init on a fresh profile, then the legacy alias in JSON mode."""
    run(r, "init json", ["--json", "space", "init", "--passphrase", PASSPHRASE, "--device-name", "compat-j"])
    run(r, "legacy init json again", ["--json", "init", "--passphrase", PASSPHRASE, "--device-name", "compat-j"])
    run(r, "status after json init", ["--json", "space", "status"], cli="rust")


@scenario
def space_init_running_daemon(r):
    """init against an already running (persistent) daemon."""
    init_space(r)
    r.run("fixture: start", ["start"], cli="rust", compare=False)
    run(r, "init with running daemon", ["space", "init", "--passphrase", PASSPHRASE, "--device-name", "x"])
    run(r, "legacy init json with running daemon", ["--json", "init", "--passphrase", PASSPHRASE])


@scenario
def space_init_tty(r):
    """Interactive init: mismatched confirmation re-prompts, then succeeds."""
    _pty_run(r, "init prompts (mismatch then match)", ["space", "init", "--device-name", "compat-tty"],
             answers=[("New space passphrase", b"abc12345\r"), ("Confirm passphrase", b"zzz\r"),
                      ("New space passphrase", PASSPHRASE.encode() + b"\r"),
                      ("Confirm passphrase", PASSPHRASE.encode() + b"\r")])
    _pty_run(r, "init again on tty (already set up)",
             ["space", "init", "--passphrase", PASSPHRASE, "--device-name", "compat-tty"])


# ---------------------------------------------------------------- invite


@scenario
def space_invite_without_space(r):
    """invite is setup-gated: a fresh profile is refused."""
    run(r, "invite fresh", ["space", "invite"], timeout=60)
    run(r, "invite fresh json", ["--json", "space", "invite"], timeout=60)
    run(r, "legacy invite fresh", ["invite"], timeout=60)


@scenario
def space_invite_interrupt(r):
    """invite under test (human, JSON, legacy) interrupted before anyone joins."""
    init_space(r)
    for label, args in (("human", False), ("json", True)):
        _stop_daemon(r)
        handle, _ = _start_invite(r, r.profile, cli="self", json_mode=args)
        time.sleep(1)
        _finish_invite(r, handle, f"invite {label} interrupted")
    _stop_daemon(r)
    handle = r.spawn("legacy invite", ["invite"])
    time.sleep(8)
    step = r.finish(handle, sig=signal.SIGINT, timeout=30, compare=False)
    r.note("legacy invite interrupted", {"exit": step.code, "stdout": _strip_code(step.out),
                                         "stderr": _strip_code(step.err)})


def _pair_with(r, invite_cli, join_cli, json_invite=False, json_join=False):
    """Init a sponsor, run invite with `invite_cli`, join with `join_cli`.
    Returns (sponsor, joiner, invite handle, join step)."""
    sponsor, joiner = r.profile, r.profile + "-b"
    r.extra_profiles.append(joiner)
    init_space(r, profile=sponsor, name="compat-sponsor")
    handle, code = _start_invite(r, sponsor, cli=invite_cli, json_mode=json_invite)
    join_args = (["--json"] if json_join else []) + ["space", "join", "--code", code, "--passphrase", PASSPHRASE,
                                                     "--device-name", "compat-joiner"]
    step = run(r, "join", join_args, cli=join_cli, profile=joiner, compare=False, timeout=150)
    return sponsor, joiner, handle, step


def _note_join(r, label, step):
    r.note(label, {"exit": step.code, "stdout": _strip_code(step.out), "stderr": _strip_code(step.err)})


@scenario
def space_invite_pairing(r):
    """invite under test (human) while a Rust joiner joins; it keeps waiting
    for `setup.pairingCompleted` and is then interrupted."""
    sponsor, joiner, handle, step = _pair_with(r, "self", "rust")
    r.note("fixture join exit", step.code)
    _wait_members(r, sponsor)
    time.sleep(3)
    r.note("invite still waiting after join", {"running": handle["proc"].poll() is None})
    _finish_invite(r, handle, "invite interrupted after join")


@scenario
def space_invite_pairing_json(r):
    """invite --json under test while a Rust joiner joins."""
    sponsor, joiner, handle, step = _pair_with(r, "self", "rust", json_invite=True)
    _wait_members(r, sponsor)
    time.sleep(3)
    r.note("invite still waiting after join", {"running": handle["proc"].poll() is None})
    _finish_invite(r, handle, "invite json interrupted after join")


# ---------------------------------------------------------------- join


@scenario
def space_join_input_errors(r):
    """Join argument validation that fails before any daemon call."""
    run(r, "join empty code", ["space", "join", "--code", "", "--passphrase", PASSPHRASE])
    run(r, "join blank code json", ["--json", "space", "join", "--code", "  "])
    run(r, "join empty passphrase", ["space", "join", "--code", "123456", "--passphrase", ""])
    run(r, "legacy join empty passphrase", ["join", "--code", "123456", "--passphrase", " "])
    h = r.spawn("join switch no tty no yes", ["space", "join", "--switch", "--code", "1 2 3 4 5 6",
                                              "--passphrase", PASSPHRASE, "--device-name", "x"])
    r.finish(h, timeout=30)
    h = r.spawn("join switch json no tty no yes", ["--json", "space", "join", "--switch", "--code", "123-456",
                                                   "--passphrase", PASSPHRASE])
    r.finish(h, timeout=30)


@scenario
def space_join_bad_code(r):
    """Join a fresh profile with codes that do not exist (human, JSON, --no-wait)."""
    run(r, "join unknown code", ["space", "join", "--code", "000 001", "--passphrase", PASSPHRASE,
                                "--device-name", "compat-j"], timeout=180)
    _stop_daemon(r)
    run(r, "join unknown code json", ["--json", "space", "join", "--code", "000001", "--passphrase", PASSPHRASE,
                                     "--device-name", "compat-j"], timeout=180)
    _stop_daemon(r)
    run(r, "join malformed code no-wait", ["space", "join", "--code", "abc-de", "--passphrase", PASSPHRASE,
                                          "--device-name", "compat-j", "--no-wait"], timeout=180)
    _stop_daemon(r)
    run(r, "legacy join unknown code json", ["--json", "join", "--code", "999-998", "--passphrase", PASSPHRASE,
                                            "--device-name", "compat-j", "--no-wait"], timeout=180)
    _wait_join_settled(r)
    run(r, "join status after failures", ["--json", "space", "join", "status"])
    run(r, "join status after failures human", ["space", "join", "status"])


@scenario
def space_join_status_cancel_empty(r):
    """join status / cancel with nothing pending, on fresh and initialized profiles."""
    _stop_daemon(r)
    run(r, "status fresh", ["space", "join", "status"])
    _stop_daemon(r)
    run(r, "status fresh json", ["--json", "space", "join", "status"])
    _stop_daemon(r)
    run(r, "cancel fresh", ["space", "join", "cancel"])
    _stop_daemon(r)
    run(r, "cancel fresh json", ["--json", "space", "join", "cancel"])
    init_space(r)
    _stop_daemon(r)
    run(r, "status initialized", ["space", "join", "status"])
    _stop_daemon(r)
    run(r, "cancel initialized json", ["--json", "space", "join", "cancel"])
    _stop_daemon(r)
    run(r, "legacy join status", ["join", "status"])
    _stop_daemon(r)
    run(r, "legacy join cancel json", ["--json", "join", "cancel"])


@scenario
def space_join_pairing(r):
    """join under test (human) redeems a Rust invitation; then status/cancel
    report the finished join, and change-passphrase is refused."""
    sponsor, joiner, handle, step = _pair_with(r, "rust", "self")
    _note_join(r, "join human", step)
    _wait_members(r, sponsor)
    r.finish(handle, sig=signal.SIGINT, timeout=30, compare=False)
    run(r, "join status after join", ["space", "join", "status"], profile=joiner)
    run(r, "join status after join json", ["--json", "space", "join", "status"], profile=joiner)
    run(r, "join cancel after join", ["space", "join", "cancel"], profile=joiner)
    run(r, "join cancel after join json", ["--json", "space", "join", "cancel"], profile=joiner)
    run(r, "change-passphrase paired", ["space", "change-passphrase", "--passphrase", NEW_PASSPHRASE],
          profile=sponsor)
    run(r, "change-passphrase paired json", ["--json", "space", "change-passphrase", "--passphrase",
                                            NEW_PASSPHRASE], profile=joiner)


@scenario
def space_join_pairing_json(r):
    """join --json under test redeems a Rust invitation."""
    sponsor, joiner, handle, step = _pair_with(r, "rust", "self", json_join=True)
    _note_join(r, "join json", step)
    _wait_members(r, sponsor)
    r.finish(handle, sig=signal.SIGINT, timeout=30, compare=False)


@scenario
def space_join_wrong_passphrase(r):
    """join under test with a valid code but the wrong passphrase, then the
    status of the failed join, then a JSON retry with the wrong passphrase."""
    sponsor, joiner = r.profile, r.profile + "-b"
    r.extra_profiles.append(joiner)
    init_space(r, profile=sponsor, name="compat-sponsor")
    handle, code = _start_invite(r, sponsor, cli="rust")
    step = run(r, "join wrong passphrase", ["space", "join", "--code", code, "--passphrase", WRONG_PASSPHRASE,
                                           "--device-name", "compat-joiner"], profile=joiner, compare=False,
                 timeout=150)
    _note_join(r, "join wrong passphrase", step)
    run(r, "join status after rejection", ["space", "join", "status"], profile=joiner)
    run(r, "join cancel after rejection json", ["--json", "space", "join", "cancel"], profile=joiner)
    step = run(r, "join wrong passphrase json", ["--json", "space", "join", "--code", code, "--passphrase",
                                                WRONG_PASSPHRASE, "--device-name", "compat-joiner"],
                 profile=joiner, compare=False, timeout=150)
    _note_join(r, "join wrong passphrase json", step)
    r.finish(handle, sig=signal.SIGINT, timeout=30, compare=False)


@scenario
def space_join_switch(r):
    """--switch on an initialized device with --yes against an unknown code."""
    init_space(r)
    _stop_daemon(r)
    run(r, "switch unknown code", ["space", "join", "--switch", "--yes", "--code", "000-002", "--passphrase",
                                  PASSPHRASE, "--device-name", "ignored"], timeout=180)
    _stop_daemon(r)
    run(r, "switch unknown code json no-wait", ["--json", "space", "join", "--switch", "--yes", "--no-wait",
                                               "--preserve-unreadable-history", "--code", "000003",
                                               "--passphrase", PASSPHRASE], timeout=180)
    run(r, "switch on fresh profile", ["space", "join", "--switch", "--yes", "--code", "000-004", "--passphrase",
                                      PASSPHRASE], profile=r.profile + "-fresh", timeout=120)
    r.extra_profiles.append(r.profile + "-fresh")


@scenario
def space_join_tty(r):
    """Interactive join prompts: code and passphrase, and the switch confirmation."""
    _pty_run(r, "join prompts empty code", ["space", "join"], answers=[("Invitation code", b"\r")])
    _pty_run(r, "join prompts empty passphrase", ["space", "join"],
             answers=[("Invitation code", b"12 3456\r"), ("Space passphrase", b"   \r")])
    init_space(r)
    _pty_run(r, "switch confirm declined", ["space", "join", "--switch", "--code", "123-456", "--passphrase",
                                            PASSPHRASE], answers=[("Switch to the new space now?", b"n")])
    _pty_run(r, "switch confirm default", ["space", "join", "--switch", "--code", "123-456", "--passphrase",
                                           PASSPHRASE], answers=[("Switch to the new space now?", b"\r")])
    _pty_run(r, "join unknown code on tty", ["space", "join", "--code", "000-005", "--passphrase", PASSPHRASE,
                                             "--device-name", "compat-tty", "--no-wait"], timeout=180)


@scenario
def space_join_interrupt(r):
    """Ctrl+C while redeem is dialing a code that does not resolve."""
    for label, json_mode in (("human", False), ("json", True)):
        args = (["--json"] if json_mode else []) + ["space", "join", "--code", "000-006", "--passphrase",
                                                    PASSPHRASE, "--device-name", "compat-j"]
        h = r.spawn(f"join interrupted {label}", args)
        time.sleep(4)
        r.finish(h, sig=signal.SIGINT, timeout=30)


# ---------------------------------------------------------------- reset / change-passphrase


@scenario
def space_reset_flows(r):
    """reset on a fresh profile, a single-device space, and after pairing."""
    run(r, "reset fresh", ["space", "reset", "--yes"])
    run(r, "reset fresh json", ["--json", "space", "reset", "--yes"])
    init_space(r)
    run(r, "reset single device", ["space", "reset", "--yes"])
    run(r, "reset single device json", ["--json", "space", "reset", "--yes"])
    run(r, "status after reset", ["--json", "space", "status"], cli="rust")


@scenario
def space_reset_paired(r):
    """reset after pairing drops the joiner from the sponsor's member list."""
    sponsor, joiner, handle, step = _pair_with(r, "rust", "rust")
    _wait_members(r, sponsor)
    r.finish(handle, sig=signal.SIGINT, timeout=30, compare=False)
    _stop_daemon(r, sponsor)
    run(r, "reset paired sponsor", ["space", "reset", "--yes"], profile=sponsor)
    listed = run(r, "member list after reset", ["--json", "member", "list"], cli="rust", profile=sponsor,
                   compare=False)
    try:
        r.note("sponsor member count after reset", len(json.loads(listed.out)))
    except ValueError:
        r.note("sponsor member list after reset", _text(listed.out) + _text(listed.err))
    _stop_daemon(r, sponsor)
    run(r, "change-passphrase after reset", ["--json", "space", "change-passphrase", "--passphrase",
                                            NEW_PASSPHRASE], profile=sponsor)


@scenario
def space_change_passphrase_flows(r):
    """change-passphrase validation, single-device success, and fresh profile."""
    run(r, "change empty", ["space", "change-passphrase", "--passphrase", ""])
    run(r, "change json without passphrase", ["--json", "space", "change-passphrase"])
    run(r, "change fresh profile", ["space", "change-passphrase", "--passphrase", NEW_PASSPHRASE])
    init_space(r)
    run(r, "change single device", ["space", "change-passphrase", "--passphrase", NEW_PASSPHRASE])
    run(r, "change single device json", ["--json", "space", "change-passphrase", "--passphrase", PASSPHRASE])
    _pty_run(r, "change prompts", ["space", "change-passphrase"],
             answers=[("New space passphrase", NEW_PASSPHRASE.encode() + b"\r"),
                      ("Confirm passphrase", NEW_PASSPHRASE.encode() + b"\r")])
    run(r, "status after change", ["--json", "space", "status"], cli="rust")

