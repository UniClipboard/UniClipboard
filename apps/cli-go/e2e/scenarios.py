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
                 ["space", "join", "help", "status"], ["mobile-sync"], ["devices", "--bogus"]):
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
