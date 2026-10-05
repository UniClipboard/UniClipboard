"""Differential scenarios. Each takes a compat.Runner and records steps.

Fixture steps that only prepare state run with cli="rust" (identical for both
flavors) and compare=False; the steps under test use the flavor's own CLI.
"""
import json
import os
import signal
import time

PASSPHRASE = "hunter22hunter22"
ALL = {}


def scenario(fn):
    ALL[fn.__name__] = fn
    return fn


def init_space(r, profile=None, name="compat-a"):
    r.run("fixture: space init", ["space", "init", "--passphrase", PASSPHRASE, "--device-name", name],
          cli="rust", profile=profile, compare=False)


def pair(r, sponsor=None, joiner=None, timeout=90):
    """Fixture: init `sponsor` and pair `joiner` into its space with the Rust
    CLI, exactly like scripts/e2e/pair.sh. Returns (sponsor, joiner) profiles.
    Needs the production rendezvous service (network)."""
    sponsor = sponsor or r.profile
    joiner = joiner or r.profile + "-b"
    r.extra_profiles.append(joiner)
    init_space(r, profile=sponsor, name="compat-sponsor")
    invite = r.spawn("fixture: invite", ["space", "invite"], profile=sponsor, cli="rust")
    code, buf, deadline = None, b"", time.time() + timeout
    fd = invite["proc"].stdout
    os.set_blocking(fd.fileno(), False)
    while time.time() < deadline and code is None:
        chunk = fd.read() or b""
        buf += chunk
        for line in buf.decode(errors="replace").splitlines():
            if line.startswith("INVITATION_CODE="):
                code = line.split("=", 1)[1].strip()
        if invite["proc"].poll() is not None and code is None:
            break
        time.sleep(0.3)
    if code is None:
        raise RuntimeError("no invitation code: " + buf.decode(errors="replace"))
    join = r.run("fixture: join", ["space", "join", "--code", code, "--passphrase", PASSPHRASE,
                                  "--device-name", "compat-joiner"], cli="rust", profile=joiner,
                 compare=False, timeout=timeout)
    if join.code != 0:
        raise RuntimeError(f"join failed: {join.code} {join.err!r}")
    # The baseline `space invite` keeps waiting after a successful join, so
    # confirm membership on the sponsor instead, then interrupt the invite.
    deadline, members = time.time() + timeout, []
    while time.time() < deadline:
        listed = r.run("fixture: member list", ["--json", "member", "list"], cli="rust", profile=sponsor,
                       compare=False)
        try:
            members = json.loads(listed.out)
        except ValueError:
            members = []
        if len(members) >= 2:
            break
        time.sleep(2)
    os.set_blocking(fd.fileno(), True)
    r.finish(invite, sig=signal.SIGINT, timeout=30, compare=False)
    if len(members) < 2:
        raise RuntimeError(f"sponsor does not list the joiner: {members!r}")
    return sponsor, joiner


@scenario
def argument_errors(r):
    """Parse-time errors never reach the daemon; exit 2 and clap text."""
    for args in (["frobnicate"], ["send", "--nope"], ["send", "--peer"], ["space", "reset"],
                 ["get", "--type", "foo"], ["send", "--connect-timeout", "abc"],
                 ["debug", "capture", "start", "--minutes", "16"], ["debug", "capture", "start", "--minutes=0"],
                 ["space", "frob"], ["member", "sync", "set"], ["search", "--limit", "-1"],
                 ["join", "--preserve-unreadable-history"], ["space", "join", "--preserve-unreadable-history"],
                 ["get", "--list", "--id", "x"], ["send", "a", "b"], ["stop", "extra"], ["send", "--text=x"],
                 ["-vv", "stop"], ["space", "join", "status", "--code", "x"], ["help", "nope"],
                 ["member", "sync", "set", "d", "--send", "maybe"], ["get", "--wait", "--limit", "3"],
                 ["send", "--peer=a", "--peer", "b", "--resend", "x", "hi"], ["get", "-n", "abc"],
                 ["get", "-c", "--list"], ["mobile", "network", "set", "--port", "5"],
                 ["mobile", "network", "set", "--ip", "1", "--url", "2"], ["mobile", "add"],
                 ["search", "--from-ms", "x"], ["search", "--limit", "99999999999"],
                 ["mobile", "setup", "--port", "70000"], ["space"], ["member"], ["space", "--json"],
                 ["--profile"], ["--version"], ["-V"], [], ["help"], ["help", "space", "join"],
                 ["space", "join", "help", "status"], ["mobile-sync"], ["devices", "--bogus"],
                 # clap "did you mean" suggestions
                 ["sned"], ["space", "stauts"], ["member", "lsit"], ["get", "--wiat"], ["search", "--limt", "3"],
                 ["--jsn", "stop"], ["devic"], ["hlp"], ["space", "join", "stauts"], ["mobile", "netwrk"],
                 ["debug", "captur", "strt"], ["stop", "--jsonn"], ["send", "--pear", "x"], ["get", "--tpye", "text"],
                 ["help", "sned"], ["mobile-sync", "statu"], ["membrs"], ["send", "--text", "--nope"],
                 ["space", "join", "--cod", "x"], ["get", "--wait", "--tpye", "x"], ["search", "--json", "--limt", "3"],
                 ["--profile", "x", "sned"], ["member", "sync", "set", "dev", "--sned", "on"]):
        r.run("args " + " ".join(args), args, timeout=20)


@scenario
def lifecycle_without_space(r):
    """No profile data: stop, status and start must refuse cleanly."""
    r.run("stop with no daemon", ["stop"])
    r.run("stop with no daemon (json)", ["--json", "stop"])
    r.run("status before init", ["space", "status"])
    r.run("start before init", ["start"])
    r.run("start before init (json)", ["--json", "start"])
    r.run("stop after refusals", ["--json", "stop"])


@scenario
def lifecycle_with_space(r):
    """start/stop/status against an initialized profile."""
    init_space(r)
    r.run("status (oneshot)", ["space", "status"])
    r.run("status json (oneshot)", ["--json", "space", "status"])
    r.run("legacy status", ["status"])
    r.run("start", ["--json", "start"])
    r.run("start again", ["start"])
    r.run("status (persistent)", ["--json", "space", "status"])
    r.run("stop", ["stop"])
    r.run("stop again", ["--json", "stop"])
    r.run("start human", ["start"])
    r.run("stop json", ["--json", "stop"])


@scenario
def foreground_start(r):
    """`start --foreground` streams the daemon and exits after SIGINT."""
    init_space(r)
    h = r.spawn("start --foreground --json", ["--json", "start", "--foreground"])
    deadline = time.time() + 60
    conn = os.path.join(r.data_root(), "daemon.conn")
    while time.time() < deadline and not os.path.exists(conn):
        time.sleep(0.2)
    time.sleep(2)
    r.run("start while foreground runs", ["--json", "start"])
    step = r.finish(h, sig=signal.SIGINT, compare=False)
    r.note("foreground ended by SIGINT", {"exit": step.code, "stdout": step.out.decode()})


@scenario
def pairing_fixture(r):
    """Smoke test of the shared pairing fixture."""
    sponsor, joiner = pair(r)
    r.run("sponsor status", ["--json", "space", "status"], profile=sponsor)
    r.run("joiner status", ["--json", "space", "status"], profile=joiner)


def _load_group_scenarios():
    """Register scenarios from scenarios_<group>.py files next to this one."""
    import importlib
    import pathlib
    for path in sorted(pathlib.Path(__file__).parent.glob("scenarios_*.py")):
        importlib.import_module(path.stem)


_load_group_scenarios()
