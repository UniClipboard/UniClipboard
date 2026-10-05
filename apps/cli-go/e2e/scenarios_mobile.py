"""Differential scenarios for `uniclip mobile` (and its `mobile-sync` alias).

Every scenario that enables the LAN listener pins an unusual high port and
ends with `mobile disable`, so no listener outlives it. Per-run credentials
(device id, minted username/password, install URL, QR) are masked: the QR is
replaced by a structural summary, everything else by a placeholder.
"""
import fcntl
import json
import os
import pty
import re
import select
import signal
import socket
import struct
import subprocess
import termios
import time

from scenarios import init_space, scenario

PORT_FLOW = "48721"
PORT_FLOW_URL = "48722"
PORT_BUSY = 48723
PORT_PERSISTENT = "48724"
PORT_PTY = "48725"
PORT_PTY_URL = "48726"
PORT_ALIAS = "48727"
FIXED_USER = "compatuser_1"
FIXED_PASS = "compat-pass-1234"
SETUP = ["mobile", "setup", "--accept-network-risk"]
QR_GLYPHS = set("█▀▄ ")
# A confirm prompt hides the cursor right before it reads keys; typing
# earlier races the switch to raw mode and the tty echoes the key.
CONFIRM_READY = "[y/N]\x1b[?25l"


class _Record:
    """Step-shaped record (label/argv/code/out/err) for compat.render."""

    def __init__(self, label, argv, code, out, err):
        self.label, self.argv, self.code, self.out, self.err = label, argv, code, out, err
        self.extra = {}


def _qr_summary(qr):
    lines = qr.split("\n")
    widths = sorted({len(line) for line in lines})
    glyphs_ok = all(set(line) <= QR_GLYPHS for line in lines)
    return f"<QR lines={len(lines)} widths={widths} glyphs_ok={glyphs_ok}>"


def _mask(r, out, err):
    """Mask per-run credentials in one step's stdout/stderr."""
    out, err = out.decode("utf-8", "replace"), err.decode("utf-8", "replace")
    secrets = {}
    try:
        data = json.loads(out)
    except ValueError:
        data = None
    if isinstance(data, dict) and "qr_code_ascii" in data:
        secrets[data["password"]] = "<PASSWORD>"
        secrets[data["install_url"]] = "<INSTALL_URL>"
        qr = data["qr_code_ascii"]
        out = out.replace(json.dumps(qr, ensure_ascii=False)[1:-1], _qr_summary(qr))
    m = re.search(r"password \(one-time\): (.*)", err)
    if m and m.group(1).strip():
        secrets[m.group(1).strip()] = "<PASSWORD>"
    m = re.search(r"installUrl: (\S+)", err)
    if m:
        secrets[m.group(1)] = "<INSTALL_URL>"
    # Human mode prints the QR alone on stdout, framed by blank lines.
    if out.startswith("\n") and out.endswith("\n\n") and "█" in out:
        out = "\n" + _qr_summary(out[1:-2]) + "\n\n"
    for value, placeholder in secrets.items():
        if value and value != FIXED_PASS:
            out, err = out.replace(value, placeholder), err.replace(value, placeholder)
    for pattern, repl in ((r"did_[0-9a-f]{32}", "<DID>"), (r"mobile_[0-9a-f]{8}", "<USER>")):
        out, err = re.sub(pattern, repl, out), re.sub(pattern, repl, err)
    return out.encode(), err.encode()


def _await_oneshot_exit(r, timeout=30):
    """Wait until the previous step's oneshot daemon has exited, so every
    oneshot step spawns its own daemon. Reusing a lingering one skips the
    spawn-time space check, and attaching to one that is shutting down fails
    the lease; both CLIs race the same way, so the race is removed instead."""
    conn = os.path.join(r.data_root(), "daemon.conn")
    deadline = time.time() + timeout
    while os.path.exists(conn) and time.time() < deadline:
        time.sleep(0.2)


def masked(r, label, args, stdin=None, profile=None, timeout=120, oneshot=False):
    """Run a CLI step whose output carries per-run credentials and record it
    masked. Returns the raw step for callers that need the real ids.

    oneshot=True drops the `Local daemon ready` line: whether a step reuses
    the previous oneshot daemon or races its idle exit and respawns is timing,
    not CLI behavior (the Rust CLI alone shows both outcomes run to run)."""
    if oneshot:
        _await_oneshot_exit(r)
    step = r.run(label, args, stdin=stdin, profile=profile, timeout=timeout, compare=False)
    out, err = _mask(r, step.out, step.err)
    if oneshot:
        err = err.replace(" ✓  Local daemon ready\n".encode(), b"")
    argv = [re.sub(r"did_[0-9a-f]{32}", "<DID>", a) for a in args]
    r.steps.append(_Record(label, argv, step.code, out, err))
    return step


def start_daemon(r):
    """Fixture: a persistent daemon, so oneshot idle-exit timing (which makes
    either CLI print `Local daemon ready` nondeterministically) stays out of
    the comparison. Oneshot paths are covered by mobile_oneshot."""
    _await_oneshot_exit(r)
    step = r.run("fixture: start", ["--json", "start"], cli="rust", compare=False)
    if step.code != 0:
        raise RuntimeError(f"start failed: {step.code} {step.err!r}")


def device_ids(r):
    """Read back the paired device ids through the Rust CLI (fixture probe)."""
    return [d["device_id"] for d in _probe(r)["devices"]]


def _probe(r):
    for _ in range(3):
        step = r.run("probe: status", ["--json", "mobile", "status"], cli="rust", stdin=b"", compare=False)
        if step.code == 0:
            return json.loads(step.out)
        time.sleep(1)
    raise RuntimeError("status probe failed: " + step.err.decode(errors="replace"))


def probe_status(r, label):
    """Record the mobile state as read back through the Rust CLI."""
    data = _probe(r)
    data["devices"] = [{**d, "device_id": "<DID>"} for d in data["devices"]]
    r.note(label, data)


def _adopt_pty():
    """Child pre-exec: new session with the pty (fd 0) as controlling
    terminal, so `/dev/tty` and Ctrl-C behave like an interactive shell."""
    os.setsid()
    fcntl.ioctl(0, termios.TIOCSCTTY, 0)


def _adopt_pty_ignoring_sigint():
    """As _adopt_pty, with SIGINT inherited as ignored (a background job)."""
    _adopt_pty()
    signal.signal(signal.SIGINT, signal.SIG_IGN)


def pty_run(r, label, args, script, timeout=90, ignore_sigint=False):
    """Run the flavor's CLI with stdin and stderr on a pty (prompts render)
    and stdout on a pipe. `script` is a list of (expected_text, keys): wait
    until the terminal shows expected_text, then type keys."""
    binary = os.path.join(r.bindir, "uniclip")
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 200, 0, 0))
    proc = subprocess.Popen([binary, *args], stdin=slave, stdout=subprocess.PIPE, stderr=slave,
                            env=r.env(extra={"TERM": "xterm"}),
                            preexec_fn=_adopt_pty_ignoring_sigint if ignore_sigint else _adopt_pty)
    os.close(slave)
    transcript, stdout = b"", b""
    deadline = time.time() + timeout
    pending = list(script)
    seen = 0
    open_fds = {master, proc.stdout.fileno()}
    timed_out = False
    while open_fds:
        if time.time() > deadline:
            timed_out = True
            proc.kill()
            break
        ready, _, _ = select.select(list(open_fds), [], [], 0.2)
        for fd in ready:
            try:
                chunk = os.read(fd, 65536)
            except OSError:  # EIO on the pty master once the child is gone
                chunk = b""
            if not chunk:
                open_fds.discard(fd)
            elif fd == master:
                transcript += chunk
            else:
                stdout += chunk
        if pending and pending[0][0].encode() in transcript[seen:]:
            seen = transcript.index(pending[0][0].encode(), seen) + len(pending[0][0])
            # Type like a person, one key at a time: bytes that arrive while
            # a prompt is between raw-mode reads get echoed by the line
            # discipline (a type-ahead artifact, not CLI output).
            for key in pending.pop(0)[1]:
                time.sleep(0.3)
                os.write(master, bytes([key]))
        if not ready and proc.poll() is not None and master in open_fds:
            # macOS may keep the master readable-less after exit; stop waiting.
            open_fds.discard(master)
    proc.wait()
    os.close(master)
    code = "timeout" if timed_out else proc.returncode
    out, err = _mask(r, stdout, transcript)
    r.steps.append(_Record(label + " [pty]", args, code, out, err))


@scenario
def mobile_without_space(r):
    """Mobile commands on a profile without a space (oneshot daemon)."""
    masked(r, "status", ["mobile", "status"], oneshot=True)
    masked(r, "status json", ["--json", "mobile", "status"], oneshot=True)
    masked(r, "setup json", ["--json", *SETUP, "--label", "x", "--port", PORT_FLOW], stdin=b"", oneshot=True)
    masked(r, "add", ["mobile", "add", "--label", "x"], stdin=b"", oneshot=True)
    masked(r, "revoke id", ["mobile", "revoke", "did_x"], oneshot=True)
    masked(r, "revoke json no id", ["--json", "mobile", "revoke"], oneshot=True)
    masked(r, "interfaces", ["mobile", "network", "interfaces"], oneshot=True)
    masked(r, "network set", ["mobile", "network", "set", "--url", "https://a.example", "--port", PORT_FLOW,
                          "--accept-network-risk"], oneshot=True)
    masked(r, "network off json", ["--json", "mobile", "network", "off"], oneshot=True)
    masked(r, "disable", ["mobile", "disable"], oneshot=True)


@scenario
def mobile_oneshot(r):
    """With a space but no running daemon each command uses a oneshot daemon."""
    init_space(r)
    r.run("fixture: stop", ["stop"], cli="rust", compare=False)
    masked(r, "status json", ["--json", "mobile", "status"], oneshot=True)
    masked(r, "setup human", [*SETUP, "--non-interactive", "--label", "One", "--port", PORT_FLOW,
                              "--username", FIXED_USER, "--password-stdin"], stdin=FIXED_PASS.encode() + b"\n",
           oneshot=True)
    masked(r, "status human", ["mobile", "status"], oneshot=True)
    ids = device_ids(r)
    masked(r, "revoke", ["mobile", "revoke", ids[0] if ids else "did_missing"], oneshot=True)
    masked(r, "network off", ["--json", "mobile", "network", "off"], oneshot=True)
    masked(r, "disable", ["mobile", "disable"], oneshot=True)
    probe_status(r, "state after disable")


@scenario
def mobile_flag_errors(r):
    """Validation that fails before any daemon contact (no space needed)."""
    r.run("setup json without risk", ["--json", "mobile", "setup", "--label", "x"])
    r.run("setup non-interactive without risk", ["mobile", "setup", "--non-interactive", "--label", "x"])
    r.run("setup non-interactive without label", ["mobile", "setup", "--non-interactive",
                                                  "--accept-network-risk"])
    r.run("setup json without label", ["--json", *SETUP])
    r.run("setup json without label after stdin", ["--json", *SETUP, "--password-stdin"],
          stdin=b"compat-pass-1234\n")
    r.run("setup invalid utf-8 stdin", ["--json", *SETUP, "--label", "x", "--password-stdin"],
          stdin=b"\xff\xfe\n")
    r.run("add invalid utf-8 stdin", ["mobile", "add", "--label", "x", "--password-stdin"], stdin=b"\xff\n")
    r.run("setup human, no tty, no risk flag", ["mobile", "setup", "--label", "x"], stdin=b"")
    r.run("network set json without risk", ["--json", "mobile", "network", "set", "--ip", "192.168.1.5"])
    r.run("network set human, no tty, no risk flag", ["mobile", "network", "set", "--url", "https://a.example"],
          stdin=b"")
    r.run("alias setup json without risk", ["--json", "mobile-sync", "setup", "--label", "x"])


@scenario
def mobile_setup_flow(r):
    """Setup, add, status, revoke and disable against oneshot daemons."""
    init_space(r)
    start_daemon(r)
    masked(r, "status before setup", ["mobile", "status"])
    masked(r, "status json before setup", ["--json", "mobile", "status"])
    masked(r, "revoke human, no devices", ["mobile", "revoke"])
    first = masked(r, "setup json fixed creds", ["--json", *SETUP, "--label", "Phone One", "--port", PORT_FLOW,
                                                 "--username", FIXED_USER, "--password-stdin"],
                   stdin=FIXED_PASS.encode() + b"\n")
    probe_status(r, "state after setup")
    masked(r, "status json after setup", ["--json", "mobile", "status"])
    masked(r, "status after setup", ["mobile", "status"])
    masked(r, "setup again, human, auto creds, port kept", ["mobile", "setup", "--non-interactive",
                                                             "--accept-network-risk", "--label", "Phone Two"],
           stdin=b"")
    probe_status(r, "state after second setup")
    masked(r, "add json fixed user", ["--json", "mobile", "add", "--label", "Phone Three", "--username",
                                      "compatuser_2"], stdin=b"")
    masked(r, "add human auto", ["mobile", "add", "--label", "Phone Four"], stdin=b"")
    masked(r, "add pw without newline", ["--json", "mobile", "add", "--label", "Phone Five",
                                         "--password-stdin"], stdin=b"compat-pass-5678")
    masked(r, "add duplicate username", ["mobile", "add", "--label", "dup", "--username", FIXED_USER], stdin=b"")
    masked(r, "add invalid username", ["--json", "mobile", "add", "--label", "bad", "--username", "1abc"], stdin=b"")
    masked(r, "add short password", ["mobile", "add", "--label", "short", "--password-stdin"], stdin=b"short\n")
    masked(r, "add empty stdin password", ["mobile", "add", "--label", "empty", "--password-stdin"], stdin=b"")
    masked(r, "add empty label", ["mobile", "add", "--label", ""], stdin=b"")
    masked(r, "setup json invalid username", ["--json", *SETUP, "--label", "x", "--username", "ab"], stdin=b"")
    masked(r, "setup json invalid ip", ["--json", *SETUP, "--label", "x", "--ip", "999.1.1.1", "--port", PORT_FLOW],
          stdin=b"")
    probe_status(r, "state after rejected adds")
    masked(r, "status human with devices", ["mobile", "status"])
    ids = device_ids(r)
    first_id = json.loads(first.out)["device_id"] if first.code == 0 else "did_missing"
    r.note("first device listed", {"listed": first_id in ids, "count": len(ids)})
    masked(r, "revoke json", ["--json", "mobile", "revoke", first_id])
    masked(r, "revoke human", ["mobile", "revoke", ids[1] if len(ids) > 1 else "did_missing"])
    masked(r, "revoke same id again", ["mobile", "revoke", first_id])
    masked(r, "revoke unknown json", ["--json", "mobile", "revoke", "did_" + "0" * 32])
    masked(r, "revoke id needing encoding", ["mobile", "revoke", "a b/c"])
    masked(r, "revoke dot", ["mobile", "revoke", "."])
    masked(r, "revoke json without id", ["--json", "mobile", "revoke"])
    masked(r, "revoke human without id, no tty", ["mobile", "revoke"], stdin=b"")
    probe_status(r, "state after revokes")
    masked(r, "disable json", ["--json", "mobile", "disable"])
    masked(r, "disable human", ["mobile", "disable"])
    probe_status(r, "state after disable")


@scenario
def mobile_network_flow(r):
    """`network interfaces|set|off` and how they patch the persisted fields."""
    init_space(r)
    start_daemon(r)
    masked(r, "interfaces", ["mobile", "network", "interfaces"])
    masked(r, "interfaces json", ["--json", "mobile", "network", "interfaces"])
    masked(r, "set url json", ["--json", "mobile", "network", "set", "--url", "https://clip.example.com",
                           "--port", PORT_FLOW_URL, "--accept-network-risk"])
    probe_status(r, "state after set url")
    masked(r, "status after set url", ["mobile", "status"])
    listed = r.run("probe: interfaces", ["--json", "mobile", "network", "interfaces"], cli="rust", compare=False)
    candidates = json.loads(listed.out) if listed.code == 0 else []
    ip = candidates[0]["ipv4"] if candidates else None
    r.note("lan interface available", ip is not None)
    if ip:
        masked(r, "set ip human", ["mobile", "network", "set", "--ip", ip, "--port", PORT_FLOW,
                               "--accept-network-risk"])
        probe_status(r, "state after set ip")
        masked(r, "setup keeps pinned ip", ["--json", *SETUP, "--label", "Pinned", "--ip", ip, "--port",
                                            PORT_FLOW], stdin=b"")
    masked(r, "set invalid ip", ["mobile", "network", "set", "--ip", "999.1.1.1", "--port", PORT_FLOW,
                             "--accept-network-risk"])
    masked(r, "set public ip", ["--json", "mobile", "network", "set", "--ip", "8.8.8.8", "--port", PORT_FLOW,
                            "--accept-network-risk"])
    masked(r, "set url without scheme", ["mobile", "network", "set", "--url", "clip.example.com", "--port",
                                     PORT_FLOW_URL, "--accept-network-risk"])
    masked(r, "set url ftp", ["--json", "mobile", "network", "set", "--url", "ftp://clip.example.com", "--port",
                          PORT_FLOW_URL, "--accept-network-risk"])
    masked(r, "set port zero", ["mobile", "network", "set", "--url", "https://clip.example.com", "--port", "0",
                            "--accept-network-risk"])
    probe_status(r, "state after rejected sets")
    masked(r, "off json", ["--json", "mobile", "network", "off"])
    probe_status(r, "state after off")
    masked(r, "off human", ["mobile", "network", "off"])
    masked(r, "status after off", ["mobile", "status"])
    masked(r, "disable", ["--json", "mobile", "disable"])


@scenario
def mobile_port_in_use(r):
    """setup on a port another process holds; the bind failure surfaces in status."""
    init_space(r)
    start_daemon(r)
    holder = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    holder.bind(("0.0.0.0", PORT_BUSY))
    holder.listen(1)
    try:
        masked(r, "setup json on busy port", ["--json", *SETUP, "--label", "Busy", "--port", str(PORT_BUSY)],
              stdin=b"")
        masked(r, "setup human on busy port", [*SETUP, "--label", "Busy", "--port", str(PORT_BUSY)], stdin=b"")
        masked(r, "status json shows listener error", ["--json", "mobile", "status"])
        masked(r, "status human shows listener error", ["mobile", "status"])
        probe_status(r, "no device registered")
    finally:
        holder.close()
        r.run("disable", ["--json", "mobile", "disable"], cli="rust", compare=False)


@scenario
def mobile_persistent_daemon(r):
    """Same commands against a running `uniclip start` daemon."""
    init_space(r)
    start_daemon(r)
    try:
        masked(r, "setup json", ["--json", *SETUP, "--label", "Live", "--port", PORT_PERSISTENT], stdin=b"")
        masked(r, "status", ["mobile", "status"])
        masked(r, "status json listener", ["--json", "mobile", "status"])
        masked(r, "add human", ["mobile", "add", "--label", "Live Two"], stdin=b"")
        ids = device_ids(r)
        masked(r, "revoke", ["--json", "mobile", "revoke", ids[0] if ids else "did_missing"])
        masked(r, "network off", ["mobile", "network", "off"])
        masked(r, "disable", ["mobile", "disable"])
        masked(r, "disable again json", ["--json", "mobile", "disable"])
        probe_status(r, "state after disable")
    finally:
        r.run("fixture: stop", ["stop"], cli="rust", compare=False)


@scenario
def mobile_deprecated_alias(r):
    """`mobile-sync` warns on stderr and otherwise behaves like `mobile`."""
    init_space(r)
    start_daemon(r)
    masked(r, "alias status", ["mobile-sync", "status"])
    masked(r, "alias status json", ["--json", "mobile-sync", "status"])
    masked(r, "alias setup json", ["--json", "mobile-sync", "setup", "--accept-network-risk", "--label", "Old",
                                   "--port", PORT_ALIAS], stdin=b"")
    masked(r, "alias add json", ["--json", "mobile-sync", "add", "--label", "Old Two"], stdin=b"")
    masked(r, "alias network interfaces json", ["--json", "mobile-sync", "network", "interfaces"])
    masked(r, "alias network set", ["mobile-sync", "network", "set", "--ip", "192.168.77.5", "--port", PORT_ALIAS,
                                    "--accept-network-risk"])
    masked(r, "alias network off", ["mobile-sync", "network", "off"])
    masked(r, "alias revoke json without id", ["--json", "mobile-sync", "revoke"])
    masked(r, "alias disable", ["mobile-sync", "disable"])


@scenario
def mobile_interactive(r):
    """Prompts on a terminal answered with single keys: risk confirmation,
    the wizard's [Enter for auto] prompts, and the revoke picker abort."""
    init_space(r)
    start_daemon(r)
    pty_run(r, "setup decline risk", ["mobile", "setup", "--port", PORT_PTY], [(CONFIRM_READY, b"n")])
    pty_run(r, "setup wizard, auto creds", ["mobile", "setup", "--label", "Pty Phone", "--port", PORT_PTY],
            [(CONFIRM_READY, b"y"), ("Username", b"\r"), ("Password", b"\r")])
    pty_run(r, "setup wizard, risk flag", ["mobile", "setup", "--accept-network-risk", "--label", "Pty Two",
                                           "--port", PORT_PTY], [("Username", b"\r"), ("Password", b"\r")])
    probe_status(r, "state after wizard")
    pty_run(r, "network set confirm", ["mobile", "network", "set", "--url", "https://clip.example.com",
                                       "--port", PORT_PTY_URL], [(CONFIRM_READY, b"y")])
    pty_run(r, "network set default answer", ["mobile", "network", "set", "--url", "https://clip.example.com",
                                              "--port", PORT_PTY_URL], [(CONFIRM_READY, b"\r")])
    pty_run(r, "revoke picker abort", ["mobile", "revoke"], [("Pick device", b"\r")])
    probe_status(r, "state after picker abort")
    r.run("disable", ["--json", "mobile", "disable"], cli="rust", compare=False)


@scenario
def mobile_interactive_typed(r):
    """Multi-key answers. Known DIFF: the Rust prompts read only the first key
    in raw mode, so the tty echoes the rest (the password in cleartext); the Go
    prompts stay raw. Kept to document the gap in the shared ui prompts."""
    init_space(r)
    start_daemon(r)
    pty_run(r, "setup wizard typed", ["mobile", "setup", "--accept-network-risk", "--port", PORT_PTY],
            [("Device label", b"\r"), ("Device label", b"Pty Two\r"), ("Username", b" ptyuser_1 \r"),
             ("Password", b"pty-pass-12\r")])
    pty_run(r, "setup wizard rejected username", ["mobile", "setup", "--accept-network-risk", "--port", PORT_PTY],
            [("Device label", b"Pty Bad\r"), ("Username", b"x\r"), ("Password", b"\r")])
    masked(r, "fixture: second device", [*SETUP, "--json", "--label", "Other", "--port", PORT_PTY], stdin=b"")
    pty_run(r, "revoke picker", ["mobile", "revoke"],
            [("Pick device", b"9\r"), ("expected", b"abc\r"), ("expected 1..2", b"1\r")])
    probe_status(r, "state after picker")
    r.run("disable", ["--json", "mobile", "disable"], cli="rust", compare=False)


@scenario
def mobile_interactive_interrupt(r):
    """Ctrl-C at a prompt: the process dies by SIGINT with no trailing output."""
    init_space(r)
    start_daemon(r)
    r.run("fixture: device", ["--json", *SETUP, "--label", "Pty", "--port", PORT_PTY], cli="rust",
          stdin=b"", compare=False)
    pty_run(r, "setup ctrl-c at risk prompt", ["mobile", "setup", "--port", PORT_PTY], [(CONFIRM_READY, b"\x03")])
    pty_run(r, "revoke ctrl-c at picker", ["mobile", "revoke"], [("Pick device", b"\x03")])
    probe_status(r, "state after interrupts")
    r.run("disable", ["--json", "mobile", "disable"], cli="rust", compare=False)


@scenario
def mobile_interactive_interrupt_ignored(r):
    """Ctrl-C at a prompt while SIGINT is inherited as ignored: the prompt
    reports an interrupted read and the command aborts with exit 1."""
    init_space(r)
    start_daemon(r)
    r.run("fixture: device", ["--json", *SETUP, "--label", "Pty", "--port", PORT_PTY], cli="rust",
          stdin=b"", compare=False)
    pty_run(r, "setup ctrl-c at risk prompt", ["mobile", "setup", "--port", PORT_PTY], [(CONFIRM_READY, b"\x03")],
            ignore_sigint=True)
    pty_run(r, "revoke ctrl-c at picker", ["mobile", "revoke"], [("Pick device", b"\x03")], ignore_sigint=True)
    probe_status(r, "state after interrupts")
    r.run("disable", ["--json", "mobile", "disable"], cli="rust", compare=False)
