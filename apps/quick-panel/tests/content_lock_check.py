#!/usr/bin/env python3
"""End-to-end check of the daemon-side content lock, on a built macOS app bundle.

Real GUI, real daemon, real quick panel helper, fresh isolated profiles, no synthetic daemon.
Every check is appended to results.txt in the output directory; nothing is deleted afterwards.

What it proves, in the order it runs:

  1. With the GUI unlocked, GUI-class clients (clientType "gui") read history through every
     content route.
  2. After a full restart the GUI is locked. The daemon then refuses every content route to
     GUI-class clients with 423 "content_locked", reads AND mutations, while the CLI (clientType
     "cli", and the real `uniclip` binary) keeps working as before, and the open routes stay open.
     Against the previous build (`--baseline`) the same probe returns 200: that is the reported
     leak, reproduced.
  3. The WebSocket carries no clipboard content to a GUI-class connection while locked, but does to
     a CLI connection (so the silence is the filter, not the absence of an event).
  4. A wrong passphrase leaves it locked; the right one unlocks, and a second client is told.
  5. Revoking withdraws content access without locking the encryption session.
  6. A request in flight when the lock lands never returns content afterwards.
  7. The quick panel drops what it holds when content gets locked, shown or hidden, and refreshes by
     itself when it gets unlocked.
  8. A GUI crash revokes the grant it held; a restart starts locked; the GUI's own Unlock button
     grants again; closing the main window keeps the grant (tray and helper stay).

The lock is a lock on GUI-class sessions. The daemon trusts the OS user that holds its token and
takes the client type from the caller, so this is not a boundary against a local process that
declares itself a CLI.

Needs an awake display and a hands-off user (it drives the desktop with peekaboo and System
Events).

Usage: python3 content_lock_check.py <UniClipboard.app> <output dir> [--uniclip <path>] [--baseline]
"""
import argparse, concurrent.futures, datetime, glob, json, os, signal, subprocess, sys, threading, time, urllib.error, urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import delivery_check as d  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
CANARY = f"LOCKCHK{int(time.time())}"

# (name, method, path) of routes that return or act on history-derived content. {id} is a real
# entry id. Mutations are only sent while locked, and the entries are checked afterwards.
READS = [
    ("search", "GET", "/search/query?query=&limit=5"),
    ("search tags", "GET", "/search/tags"),
    ("entries list", "GET", "/clipboard/entries?limit=5"),
    ("entry detail", "GET", "/clipboard/entries/{id}"),
    ("entry resource", "GET", "/clipboard/entries/{id}/resource"),
    ("entry file", "GET", "/clipboard/entries/{id}/file"),
    ("entry receive", "GET", "/clipboard/entries/{id}/receive"),
    ("blob", "GET", "/clipboard/blobs/does-not-exist"),
    ("thumbnail", "GET", "/clipboard/thumbnails/does-not-exist"),
    ("receives", "GET", "/clipboard/receives"),
    ("stats", "GET", "/clipboard/stats"),
]
MUTATIONS = [
    ("restore to clipboard", "POST", "/clipboard/restore/{id}"),
    ("favorite", "POST", "/clipboard/entries/{id}/favorite"),
    ("delete entry", "DELETE", "/clipboard/entries/{id}"),
    ("clear history", "POST", "/clipboard/entries/clear"),
    ("config export", "POST", "/config/export"),
]
OPEN = [
    ("settings", "GET", "/settings"),
    ("encryption state", "GET", "/encryption/state"),
    ("content lock status", "GET", "/content-lock"),
    ("search status", "GET", "/search/status"),
]


class Session:
    """One authenticated daemon session of a declared client type."""

    def __init__(self, profile, client_type, pid=None):
        self.profile, self.client_type = profile, client_type
        conn = profile.conn()
        self.base = f"http://{conn['host']}:{conn['port']}"
        self.pid = pid or os.getpid()
        status, raw = self._raw("POST", "/auth/connect", {"pid": self.pid, "clientType": client_type}, {"authorization": "Bearer " + conn["token"]})
        self.token = json.loads(raw)["data"]["sessionToken"]

    def _raw(self, method, path, body, headers):
        req = urllib.request.Request(self.base + path, method=method, data=json.dumps(body).encode() if body is not None else None)
        req.add_header("content-type", "application/json")
        for k, v in headers.items():
            req.add_header(k, v)
        try:
            with urllib.request.urlopen(req, timeout=30) as r:
                return r.status, r.read().decode(errors="replace")
        except urllib.error.HTTPError as e:
            return e.code, e.read().decode(errors="replace")

    def call(self, method, path, body=None):
        if body is None and method in ("POST", "PUT", "PATCH"):
            body = {}
        return self._raw(method, path, body, {"authorization": "Session " + self.token})


def is_locked(status, body):
    return status == 423 and "content_locked" in body


def seed(texts):
    for text in texts:
        subprocess.run(["pbcopy"], input=text.encode(), check=True)
        time.sleep(2.5)


def entry_ids(profile):
    """Entry ids and their text, read as the CLI (which the lock does not apply to)."""
    status, body = Session(profile, "cli").call("GET", "/search/query?query=&limit=50")
    items = json.loads(body)["data"]["items"] if status == 200 else []
    return {item["entryId"]: item.get("textPreview", "") for item in items}


def probe_all(profile, client_type, entry_id, expect_locked, label, mutations=False):
    """Sends every content route as this client type. Returns the routes that did not behave."""
    session = Session(profile, client_type)
    wrong = []
    for name, method, path in READS + (MUTATIONS if mutations else []):
        status, body = session.call(method, path.replace("{id}", entry_id))
        ok = is_locked(status, body) if expect_locked else status != 423
        if not ok:
            wrong.append(f"{name}={status}")
    d.check(f"{label}: {'every' if expect_locked else 'no'} content route "
            f"{'is refused with 423 content_locked' if expect_locked else 'is refused'}"
            f" for a {client_type} client ({len(READS) + (len(MUTATIONS) if mutations else 0)} routes)", not wrong, ", ".join(wrong))
    return wrong


def probe_open(profile, client_type, label):
    session = Session(profile, client_type)
    wrong = [f"{n}={s}" for n, m, p in OPEN for s, _ in [session.call(m, p)] if s != 200]
    d.check(f"{label}: the routes needed to unlock and to draw a locked screen stay open ({client_type})", not wrong, ", ".join(wrong))


def start_ws(profile, client_type, seconds, topics):
    node = subprocess.Popen(["node", os.path.join(HERE, "ws_probe.mjs"), os.path.join(profile.data, "daemon.conn"),
                             client_type, str(os.getpid() + 7), str(seconds), topics], stdout=subprocess.PIPE, text=True)
    return node


def ws_frames(node):
    out, _ = node.communicate(timeout=60)
    frames = []
    for line in out.splitlines():
        try:
            record = json.loads(line)
        except json.JSONDecodeError:
            continue
        if record.get("probe") == "frame":
            try:
                frames.append((record["ts"], json.loads(record["frame"]), record["frame"]))
            except json.JSONDecodeError:
                frames.append((record["ts"], {}, record["frame"]))
    return frames


def daemon_log(profile):
    files = sorted(glob.glob(os.path.expanduser(f"~/Library/Logs/app.uniclipboard.desktop-{profile.name}/uniclipboard-daemon.json.*")))
    lines = []
    for path in files:
        with open(path, errors="replace") as handle:
            for line in handle:
                try:
                    lines.append(json.loads(line))
                except json.JSONDecodeError:
                    pass
    return lines


def stamp(entry):
    return datetime.datetime.fromisoformat(entry["timestamp"].replace("Z", "+00:00")).timestamp()


def searches_since(profile, since):
    return [e for e in daemon_log(profile) if e.get("path") == "/search/query" and e.get("message") == "daemon http request received" and stamp(e) >= since]


def gui_log_text(profile):
    path = os.path.join(d.OUT, f"{profile.name}-gui{profile.count}.log")
    return open(path, errors="replace").read()


def full_quit(profile, gui):
    """SIGTERM the GUI; a full quit takes the daemon with it."""
    daemon = profile.conn()["pid"] if profile.conn() else None
    os.kill(gui, signal.SIGTERM)
    d.wait_for(lambda: profile.gui.poll() is not None, 20)
    if daemon:
        d.wait_for(lambda: not d.alive(daemon), 20)
    return daemon


def restart_locked(profile):
    """A new GUI (and so a new daemon) for the same profile: the grant starts closed."""
    gui = profile.start()
    d.wait_for(lambda: profile.conn() and d.alive(profile.conn()["pid"]), 90)
    time.sleep(20)
    d.wait_for(lambda: "UniClipboard - ID" in d.sh(d.PEEKABOO, "list", "windows", "--pid", str(gui), "--no-remote"), 30)
    return gui


def press_unlock_in_gui(profile, gui, expect):
    """Clicks the GUI's own Unlock button until the daemon reports the expected answer."""
    for attempt in range(1, 4):
        d.say(f"unlock click attempt {attempt}:", d.click_in_window(gui, 450, 371)[:120])
        if d.wait_for(lambda: json.loads(Session(profile, "gui", pid=gui).call("GET", "/content-lock")[1])["data"]["unlocked"] == expect, 10):
            return True
    return False


def status_of(profile, pid=None):
    return json.loads(Session(profile, "gui", pid=pid).call("GET", "/content-lock")[1])["data"]


def onboard(profile, gui):
    d.shot(f"{profile.name}-onboarding.png")
    d.onboard_in_gui(gui)
    d.shot(f"{profile.name}-after-onboarding.png")
    d.check("space initialised through the GUI", '"initialized":true' in profile.api("GET", "/encryption/state")[1])


def run_baseline(app):
    """The previous build: the GUI is locked and a GUI-class client still reads history."""
    d.say("BASELINE (the build before the daemon-side lock)")
    p = d.Profile("baseline")
    gui = p.start()
    d.check("baseline: daemon came up", bool(d.wait_for(p.conn, 90)))
    time.sleep(20)
    onboard(p, gui)
    seed([f"{CANARY}-baseline-a", f"{CANARY}-baseline-b"])
    ids = entry_ids(p)
    d.check("baseline: entries were captured", len(ids) >= 2, len(ids))
    full_quit(p, gui)
    gui = restart_locked(p)
    d.shot("baseline-gui-locked.png")
    status, body = Session(p, "gui").call("GET", "/search/query?query=&limit=5")
    d.check("baseline REPRODUCED: the GUI shows its lock screen and a GUI-class client still reads history (200)", status == 200 and CANARY in body, f"{status} {body[:80]}")
    session = Session(p, "gui")
    leaked = [name for name, method, path in READS
              if not is_locked(*session.call(method, path.replace("{id}", next(iter(ids), "x"))))]
    d.check("baseline REPRODUCED: none of the content routes refused the GUI-class client", len(leaked) == len(READS), ", ".join(leaked))
    full_quit(p, gui)
    p.stop_daemon()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("app")
    parser.add_argument("out")
    parser.add_argument("--uniclip", default=None)
    parser.add_argument("--baseline", action="store_true")
    args = parser.parse_args()
    d.configure(args.app, args.out)
    d.say("content lock check", d.RUN, "| app:", d.APP, "| canary:", CANARY)
    d.say("host:", d.sh("sw_vers", "-productVersion").strip(), d.sh("uname", "-m").strip())
    d.check("display is awake", d.display_awake(), "wake it first (caffeinate -u -d) and keep hands off")
    if args.baseline:
        run_baseline(args.app)
        d.say(f"RESULT {sum(d.passed)}/{len(d.passed)} checks passed")
        return 0 if all(d.passed) else 1

    d.check("the daemon in the package carries the content lock", b"content-lock" in open(os.path.join(d.APP, "Contents/MacOS/uniclipd"), "rb").read())
    p = d.Profile("lock")
    gui = p.start()
    d.check("daemon came up", bool(d.wait_for(p.conn, 90)))
    time.sleep(20)

    # 0. Nothing is set up: locked, and GUI-class clients get nothing.
    status, body = Session(p, "gui").call("GET", "/search/query?query=&limit=5")
    d.check("fresh profile: a GUI-class client is refused (nothing is set up)", is_locked(status, body), f"{status} {body[:80]}")

    # 1. Set up in the GUI (the user's first-run flow) and capture some history.
    onboard(p, gui)
    seed([f"{CANARY}-alpha", f"{CANARY}-beta", f"{CANARY}-gamma"])
    ids = entry_ids(p)
    d.check("history was captured", len(ids) >= 3, len(ids))
    entry = next(iter(ids))
    probe_all(p, "gui", entry, False, "GUI unlocked")
    d.check("GUI unlocked: search returns the captured text to a GUI-class client", CANARY in Session(p, "gui").call("GET", f"/search/query?query={CANARY}&limit=5")[1])
    d.check("the daemon reports unlocked", status_of(p)["unlocked"] is True)

    # 2. A full restart: the daemon is new, the grant starts closed, the GUI shows its lock screen.
    old_daemon = full_quit(p, gui)
    gui = restart_locked(p)
    new_daemon = p.conn()["pid"]
    d.check("a restart means a new daemon process", new_daemon != old_daemon, f"{old_daemon} -> {new_daemon}")
    d.shot("gui-locked-after-restart.png")
    d.check("after the restart the daemon says locked", status_of(p)["unlocked"] is False)
    probe_all(p, "gui", entry, True, "GUI locked", mutations=True)
    probe_all(p, "helper", entry, True, "GUI locked")
    probe_open(p, "gui", "GUI locked")
    d.check("...while the encryption session itself is ready (background work is untouched)", '"sessionReady":true' in p.api("GET", "/encryption/state")[1])
    remaining = entry_ids(p)
    d.check("the refused mutations changed nothing: every entry is still there", set(ids) <= set(remaining), f"{len(remaining)} of {len(ids)}")
    # The product semantics of other clients are unchanged.
    probe_all(p, "cli", entry, False, "GUI locked")
    d.check("GUI locked: the CLI client still finds the history", CANARY in Session(p, "cli").call("GET", f"/search/query?query={CANARY}&limit=5")[1])
    if args.uniclip:
        env = dict(os.environ, UC_PROFILE=p.name)
        result = subprocess.run([args.uniclip, "search", CANARY, "--json"], env=env, capture_output=True, text=True, timeout=120)
        open(os.path.join(d.OUT, "uniclip-search-while-locked.txt"), "w").write(result.stdout + "\n" + result.stderr)
        d.check("GUI locked: the real `uniclip search` still works", result.returncode == 0 and CANARY in result.stdout, f"exit {result.returncode}")

    # 3. The WebSocket while locked.
    gui_ws = start_ws(p, "gui", 14, "clipboard,file-transfer,content-lock")
    cli_ws = start_ws(p, "cli", 14, "clipboard,content-lock")
    time.sleep(4)
    seed([f"{CANARY}-leak-canary-while-locked"])
    time.sleep(1)
    gui_frames, cli_frames = ws_frames(gui_ws), ws_frames(cli_ws)
    open(os.path.join(d.OUT, "ws-gui-while-locked.jsonl"), "w").write("\n".join(f[2] for f in gui_frames))
    open(os.path.join(d.OUT, "ws-cli-while-locked.jsonl"), "w").write("\n".join(f[2] for f in cli_frames))
    d.check("WS while locked: a GUI-class connection is told content is locked (snapshot)", any(f[1].get("topic") == "content-lock" and f[1].get("payload", {}).get("unlocked") is False for f in gui_frames))
    d.check("WS while locked: a CLI connection does receive the clipboard event", any(f[1].get("type") == "clipboard.new_content" for f in cli_frames))
    d.check("WS while locked: the GUI-class connection receives no clipboard or file-transfer event", not any(f[1].get("topic") in ("clipboard", "file-transfer") for f in gui_frames))
    d.check("WS while locked: no frame of the GUI-class connection carries a preview, an entry id or the canary", not any(k in f[2] for f in gui_frames for k in (CANARY, "preview", "entryId")))

    # 4. Unlocking: wrong passphrase first, then the right one; a second client is told.
    listener = start_ws(p, "gui", 12, "clipboard,content-lock")
    time.sleep(3)
    session = Session(p, "gui")
    status, body = session.call("POST", "/content-lock/unlock", {"passphrase": "definitely-not-it"})
    d.check("a wrong passphrase is rejected", status in (401, 403) and "WRONG" in body.upper(), f"{status} {body[:80]}")
    d.check("...and leaves content locked", status_of(p)["unlocked"] is False)
    d.check("...so a GUI-class client is still refused", is_locked(*Session(p, "gui").call("GET", "/search/query?query=&limit=1")))
    status, body = session.call("POST", "/content-lock/unlock", {"passphrase": d.PASSPHRASE})
    d.check("the right passphrase unlocks", status == 200 and json.loads(body)["data"]["unlocked"] is True, f"{status} {body[:80]}")
    probe_all(p, "gui", entry, False, "after the passphrase")
    time.sleep(1)
    seed([f"{CANARY}-after-unlock"])
    time.sleep(2)
    frames = ws_frames(listener)
    open(os.path.join(d.OUT, "ws-second-client-across-unlock.jsonl"), "w").write("\n".join(f[2] for f in frames))
    d.check("another client is told: content_lock.changed unlocked=true", any(f[1].get("type") == "content_lock.changed" and f[1].get("payload", {}).get("unlocked") is True for f in frames))
    d.check("...and clipboard events reach it again after the unlock", any(f[1].get("type") == "clipboard.new_content" for f in frames))

    # 5. Revoking withdraws content access but does not lock the encryption session.
    status, body = session.call("POST", "/content-lock/revoke")
    d.check("revoke answers locked", status == 200 and json.loads(body)["data"]["unlocked"] is False, f"{status} {body[:80]}")
    d.check("after revoke a GUI-class client is refused", is_locked(*Session(p, "gui").call("GET", "/search/query?query=&limit=1")))
    d.check("...the encryption session is still ready", '"sessionReady":true' in p.api("GET", "/encryption/state")[1])
    d.check("...the CLI still reads", Session(p, "cli").call("GET", "/search/query?query=&limit=1")[0] == 200)

    # 6. A request in flight when the lock lands must not return content afterwards.
    session.call("POST", "/content-lock/unlock", {"passphrase": d.PASSPHRASE})
    results, lock = [], threading.Lock()
    worker = Session(p, "gui")

    def one(index):
        time.sleep(index * 0.012)
        started = time.time()
        status, _ = worker.call("GET", "/search/query?query=&limit=50")
        with lock:
            results.append((started, time.time(), status))

    revoked = {}

    def revoke_midway():
        time.sleep(0.5)
        revoked["start"] = time.time()
        Session(p, "gui").call("POST", "/content-lock/revoke")
        revoked["done"] = time.time()

    with concurrent.futures.ThreadPoolExecutor(max_workers=24) as pool:
        futures = [pool.submit(one, i) for i in range(120)] + [pool.submit(revoke_midway)]
        for future in futures:
            future.result()
    late = [r for r in results if r[2] == 200 and r[1] > revoked["done"] + 0.02]
    before = [r for r in results if r[2] == 200 and r[1] < revoked["start"]]
    after = [r for r in results if r[2] == 423 and r[0] > revoked["done"]]
    open(os.path.join(d.OUT, "in-flight.json"), "w").write(json.dumps({"revoke": revoked, "results": results}, indent=1))
    d.check("in flight: some requests were served before the lock and some refused after it (the test had teeth)", bool(before) and bool(after), f"{len(before)} served before, {len(after)} refused after")
    d.check("in flight: no request returned content after the lock landed", not late, f"{len(late)} late 200s of {len(results)}")

    # 7. The quick panel: drops content on lock, refreshes on unlock.
    session.call("POST", "/content-lock/unlock", {"passphrase": d.PASSPHRASE})
    d.hide_gui(gui)
    helper = d.helper_of(gui)
    d.check("the quick panel helper is running", bool(helper), f"pid {helper}")
    d.hotkey("cmd,ctrl,v")
    time.sleep(3)
    d.shot("panel-unlocked.png")
    d.check("panel is on screen", bool(d.panel_windows(helper)))
    before_drop = gui_log_text(p).count("Quick panel dropped its content")
    Session(p, "gui").call("POST", "/content-lock/revoke")
    d.check("panel shown, content locked: it says it dropped rows, images, previews and options", bool(d.wait_for(lambda: gui_log_text(p).count("Quick panel dropped its content") > before_drop, 10)))
    time.sleep(1)
    d.shot("panel-locked.png")
    unlock_time = time.time()
    session.call("POST", "/content-lock/unlock", {"passphrase": d.PASSPHRASE})
    time.sleep(3)
    refreshed = searches_since(p, unlock_time - 1)
    d.shot("panel-unlocked-again.png")
    d.check("panel shown, content unlocked: the panel searches again by itself", bool(refreshed), f"{len(refreshed)} search requests after the unlock")
    d.hotkey("cmd,ctrl,v")
    time.sleep(1)
    # Hidden panel: the same drop happens.
    before_drop = gui_log_text(p).count("Quick panel dropped its content")
    Session(p, "gui").call("POST", "/content-lock/revoke")
    d.check("panel hidden, content locked: it drops its content too", bool(d.wait_for(lambda: gui_log_text(p).count("Quick panel dropped its content") > before_drop, 10)))
    d.check("...and the helper was not killed to do it", d.alive(helper))

    # 8. GUI crash, restart, the GUI's own Unlock button, closing the main window.
    d.front(gui)
    time.sleep(4)
    d.shot("gui-locked-after-revoke.png")
    d.check("the GUI's own Unlock button grants content access", press_unlock_in_gui(p, gui, True))
    d.check("...a GUI-class client is served", Session(p, "gui").call("GET", "/search/query?query=&limit=1")[0] == 200)
    daemon_pid = p.conn()["pid"]
    os.kill(gui, signal.SIGKILL)
    d.wait_for(lambda: p.gui.poll() is not None, 10)
    revoked_after_crash = d.wait_for(lambda: is_locked(*Session(p, "gui", pid=os.getpid() + 11).call("GET", "/search/query?query=&limit=1")), 12)
    d.check("GUI crash: the daemon revokes the access the GUI held", bool(revoked_after_crash))
    d.check("...the daemon survives the crash", d.alive(daemon_pid))
    log_text = "\n".join(e.get("message", "") for e in daemon_log(p))
    d.check("...and logged why", "the process that granted content access is gone" in log_text)
    d.check("...the helper did not outlive the GUI", bool(d.wait_for(lambda: not d.alive(helper), 10)), f"pid {helper}")
    gui = p.start()
    time.sleep(25)
    d.wait_for(lambda: "UniClipboard - ID" in d.sh(d.PEEKABOO, "list", "windows", "--pid", str(gui), "--no-remote"), 30)
    d.check("GUI restart after the crash starts locked", is_locked(*Session(p, "gui").call("GET", "/search/query?query=&limit=1")))
    d.shot("gui-locked-after-crash-restart.png")
    d.check("the GUI's Unlock button grants again", press_unlock_in_gui(p, gui, True))
    helper = d.wait_for(lambda: d.helper_of(gui), 20)
    d.check("...and the helper of the new GUI is running", bool(helper), f"pid {helper}")
    d.say("close click:", d.click_in_window(gui, 16, 20)[:120])
    time.sleep(5)
    windows, items = d.ax_counts(gui)
    d.check("main window closed: WebView destroyed, tray item stays", windows == 0 and bool(items), f"windows={windows} status items={items}")
    d.check("...content access is kept (its holder, the GUI process, is still there)", Session(p, "gui").call("GET", "/search/query?query=&limit=1")[0] == 200)
    d.check("...and the helper keeps running", d.alive(helper))

    os.kill(gui, signal.SIGTERM)
    d.check("GUI exits on SIGTERM", bool(d.wait_for(lambda: p.gui.poll() is not None, 20)))
    p.stop_daemon()
    d.say(f"RESULT {sum(d.passed)}/{len(d.passed)} checks passed")
    return 0 if all(d.passed) else 1


if __name__ == "__main__":
    sys.exit(main())
