"""Differential scenarios for the `member` command group and the deprecated
`members` / `devices` alias."""
import json
import os
import pty
import re
import select
import subprocess
import time

from compat import Step
from scenarios import scenario, pair, init_space


def _json(step):
    try:
        return json.loads(step.out)
    except ValueError:
        return None


def _roster(r, profile=None):
    """Fixture probe: the member roster as seen by the Rust baseline."""
    step = r.run("fixture: member list", ["--json", "member", "list"], cli="rust", profile=profile,
                 compare=False)
    return _json(step) or []


def _mask_ids(r, ids):
    """Replace per-run device ids with stable placeholders in every recorded
    step. Device ids are minted per profile, so they differ between the two
    flavors' runs; only these exact values are rewritten."""
    for step in r.steps:
        for placeholder, value in ids.items():
            if value:
                step.out = step.out.replace(value.encode(), placeholder.encode())
                step.err = step.err.replace(value.encode(), placeholder.encode())
                step.argv = [a.replace(value, placeholder) for a in step.argv]
        # The joiner's join id is random per pairing run.
        step.out = re.sub(rb'("joinId": )"[A-Za-z0-9_-]+"', rb'\1"<JOIN_ID>"', step.out)


def _run_tty(r, label, args, profile=None, keys=b"", timeout=60):
    """Run the flavor's CLI with stdin/stderr on a pseudo-terminal (stdout
    stays a pipe), optionally typing `keys`, and record the transcript."""
    binary = os.path.join(r.bindir, "uniclip")
    master, slave = pty.openpty()
    proc = subprocess.Popen([binary, *args], stdin=slave, stdout=subprocess.PIPE, stderr=slave,
                            env=r.env(profile), start_new_session=True)
    os.close(slave)
    if keys:
        time.sleep(1)
        os.write(master, keys)
    transcript, deadline = b"", time.time() + timeout
    while time.time() < deadline:
        ready, _, _ = select.select([master], [], [], 0.2)
        if ready:
            try:
                chunk = os.read(master, 4096)
            except OSError:
                chunk = b""
            if not chunk:
                break
            transcript += chunk
        elif proc.poll() is not None:
            break
    out = proc.stdout.read()
    try:
        code = proc.wait(timeout=10)
    except subprocess.TimeoutExpired:
        proc.kill()
        code = "timeout"
    os.close(master)
    step = Step(label, args, code, out, transcript)
    r.steps.append(step)
    return step


def _start(r, profile=None):
    """Fixture: run a persistent daemon so steps never race a oneshot
    daemon's idle shutdown."""
    r.run("fixture: start", ["start"], cli="rust", profile=profile, compare=False)


def _stop(r, profile=None):
    """Fixture: stop any daemon so the next step starts from a clean state."""
    r.run("fixture: stop", ["stop"], cli="rust", profile=profile, compare=False)


@scenario
def member_without_space(r):
    """No space: every daemon-backed member command refuses; argument
    validation in `member sync set` still runs first."""
    steps = (("member list", ["member", "list"]),
             ("member list (json)", ["--json", "member", "list"]),
             ("members alias", ["members"]),
             ("member remove", ["member", "remove", "peer-x"]),
             ("trust status", ["--json", "member", "trust", "status"]),
             ("trust choose", ["member", "trust", "choose"]),
             ("sync show", ["member", "sync", "show", "peer-x"]),
             ("sync set without settings", ["member", "sync", "set", "peer-x"]),
             ("sync set without settings (json)", ["--json", "member", "sync", "set", "peer-x"]),
             ("sync set valid", ["--json", "member", "sync", "set", "peer-x", "--send", "off"]))
    for label, args in steps:
        r.run(label, args)
        _stop(r)


@scenario
def member_single_device(r):
    """A one-device space: roster, alias, trust state and every error path
    that needs no peer."""
    init_space(r)
    _start(r)
    local = (_roster(r) or [{}])[0].get("device_id")
    r.run("member list", ["member", "list"])
    r.run("member list (json)", ["--json", "member", "list"])
    r.run("member list --probe", ["member", "list", "--probe"])
    r.run("member list --probe (json)", ["--json", "member", "list", "--probe"])
    r.run("members alias", ["members"])
    r.run("devices alias (json)", ["--json", "devices", "--probe"])
    r.run("trust status", ["member", "trust", "status"])
    r.run("trust status (json)", ["--json", "member", "trust", "status"])
    r.run("trust choose (no issues)", ["member", "trust", "choose"])
    r.run("trust choose (no issues, json)", ["--json", "member", "trust", "choose"])
    r.run("trust choose stale issue", ["member", "trust", "choose", "--issue", "c:stale"])
    r.run("trust choose stale issue (json)", ["--json", "member", "trust", "choose", "--issue", "c:stale",
                                              "--choice", "b:x", "--confirm-local-removal"])
    r.run("trust choose choice only (json)", ["--json", "member", "trust", "choose", "--choice", "b:x"])
    r.run("sync show unknown", ["member", "sync", "show", "no-such-device"])
    r.run("sync show unknown (json)", ["--json", "member", "sync", "show", "no-such-device"])
    r.run("sync show local", ["member", "sync", "show", local or "missing-local"])
    r.run("sync show local (json)", ["--json", "member", "sync", "show", local or "missing-local"])
    r.run("sync set unknown", ["member", "sync", "set", "no-such-device", "--send", "on"])
    r.run("sync set unknown (json)", ["--json", "member", "sync", "set", "no-such-device", "--receive", "off"])
    for label, types in (("unknown type", "text,video"), ("mixed sentinel", "all,text"),
                         ("empty list", " , "), ("empty string", "")):
        r.run(f"sync set {label}", ["member", "sync", "set", "no-such-device", "--send-types", types])
        r.run(f"sync set {label} (json)", ["--json", "member", "sync", "set", "no-such-device",
                                          "--receive-types", types])
    r.run("sync set both types invalid", ["member", "sync", "set", "x", "--send-types", "bogus",
                                          "--receive-types", "nope"])
    r.run("remove unknown", ["member", "remove", "no-such-peer"])
    r.run("remove unknown (json)", ["--json", "member", "remove", "no-such-peer"])
    r.run("remove local", ["member", "remove", local or "missing-local"])
    r.run("remove local (json)", ["--json", "member", "remove", local or "missing-local"])
    r.run("list after removals (json)", ["--json", "member", "list"])
    _run_tty(r, "trust choose (tty, no issues)", ["member", "trust", "choose"])
    _mask_ids(r, {"<LOCAL_ID>": local})


@scenario
def member_paired(r):
    """Two paired profiles: roster on both sides, sync preferences, device
    name resolution (TTY only), trust state, then removal read back on both
    sides."""
    sponsor, joiner = pair(r)
    _start(r, sponsor)
    _start(r, joiner)
    roster = _roster(r, sponsor)
    sponsor_id = next((m["device_id"] for m in roster if m["is_local"]), None)
    joiner_id = next((m["device_id"] for m in roster if not m["is_local"]), None)
    ids = {"<SPONSOR_ID>": sponsor_id, "<JOINER_ID>": joiner_id}

    r.run("sponsor list", ["member", "list"], profile=sponsor)
    r.run("sponsor list (json)", ["--json", "member", "list"], profile=sponsor)
    r.run("joiner list (json)", ["--json", "member", "list"], profile=joiner)
    r.run("joiner list --probe", ["member", "list", "--probe"], profile=joiner)
    r.run("sponsor list --probe (json)", ["--json", "member", "list", "--probe"], profile=sponsor)
    r.run("sponsor trust status", ["member", "trust", "status"], profile=sponsor)
    r.run("joiner trust status (json)", ["--json", "member", "trust", "status"], profile=joiner)

    r.run("sync show peer", ["member", "sync", "show", joiner_id], profile=sponsor)
    r.run("sync show peer (json)", ["--json", "member", "sync", "show", joiner_id], profile=sponsor)
    r.run("sync show by name (non-tty)", ["member", "sync", "show", "compat-joiner"], profile=sponsor)
    r.run("sync show by name (json)", ["--json", "member", "sync", "show", "COMPAT-JOINER"], profile=sponsor)
    r.run("sync set by name (non-tty)", ["member", "sync", "set", "compat-joiner", "--send", "off"],
          profile=sponsor)
    _run_tty(r, "sync show by name (tty)", ["member", "sync", "show", "Compat-Joiner"], profile=sponsor)
    _run_tty(r, "sync show unknown name (tty)", ["member", "sync", "show", "nobody"], profile=sponsor)
    r.run("sync set send off, receive text,image", ["member", "sync", "set", joiner_id, "--send", "off",
                                                    "--receive-types", "Text, image ,text"], profile=sponsor)
    r.run("sync show after set (json)", ["--json", "member", "sync", "show", joiner_id], profile=sponsor)
    r.run("sync set all/none (json)", ["--json", "member", "sync", "set", joiner_id, "--send", "on",
                                       "--receive", "off", "--send-types", "none", "--receive-types", "all"],
          profile=sponsor)
    r.run("sync set rich-text,code-snippet,file,link", ["member", "sync", "set", joiner_id, "--send-types",
                                                        "code-snippet,rich-text,file,link"], profile=sponsor)
    _run_tty(r, "sync set by name (tty)", ["member", "sync", "set", "compat-joiner", "--receive", "on",
                                           "--send-types", "all"], profile=sponsor)
    r.run("sync show after tty set (json)", ["--json", "member", "sync", "show", joiner_id], profile=sponsor)
    r.run("joiner sync show sponsor (json)", ["--json", "member", "sync", "show", sponsor_id], profile=joiner)
    r.run("sync set invalid type with valid device", ["member", "sync", "set", joiner_id, "--send-types",
                                                      "text,gif"], profile=sponsor)

    r.run("sponsor removes joiner", ["member", "remove", joiner_id], profile=sponsor)
    time.sleep(3)
    r.run("sponsor list after removal (json)", ["--json", "member", "list"], profile=sponsor)
    r.run("sponsor list after removal", ["member", "list"], profile=sponsor)
    r.run("joiner list after removal (json)", ["--json", "member", "list"], profile=joiner)
    r.run("sponsor trust status after removal", ["member", "trust", "status"], profile=sponsor)
    r.run("sponsor sync show removed (json)", ["--json", "member", "sync", "show", joiner_id], profile=sponsor)
    r.run("sponsor removes joiner again (json)", ["--json", "member", "remove", joiner_id], profile=sponsor)
    _mask_ids(r, ids)
