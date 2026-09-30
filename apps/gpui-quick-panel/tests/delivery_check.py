#!/usr/bin/env python3
"""Delivery check of the native quick panel in a built macOS app bundle.

Runs the real GUI, daemon and helper from the bundle in fresh isolated profiles and does NOT set
UC_GPUI_QUICK_PANEL, so it shows what a first install does. Every check is appended to
results.txt in a per-run directory; nothing is deleted afterwards (profiles, logs, screenshots).

Needs an awake display and a hands-off user: it drives the desktop with peekaboo.
The content-lock gate checked here is a GUI-side safeguard, not a daemon security boundary.

Usage: python3 delivery_check.py <path to UniClipboard.app> [output dir]
"""
import json, os, signal, subprocess, sys, time, urllib.error, urllib.request

APP = os.path.abspath(sys.argv[1])
GUI = os.path.join(APP, "Contents/MacOS/uniclipboard")
HELPER_NAME = "uniclip-quick-panel"
RUN = str(int(time.time()))
OUT = os.path.abspath(sys.argv[2]) if len(sys.argv) > 2 else os.path.join(os.getcwd(), f"delivery-{RUN}")
os.makedirs(OUT, exist_ok=True)
PEEKABOO = "/opt/homebrew/bin/peekaboo"
PASSPHRASE = "DeliveryCheck-2026!"
RESULTS = open(os.path.join(OUT, "results.txt"), "a")
passed = []


def say(*parts):
    line = " ".join(str(p) for p in parts)
    print(line, flush=True)
    RESULTS.write(line + "\n")
    RESULTS.flush()


def check(name, ok, detail=""):
    passed.append(bool(ok))
    say("PASS" if ok else "FAIL", "-", name, ("| " + str(detail)) if detail else "")
    return ok


def sh(*args, timeout=30):
    return subprocess.run(list(args), capture_output=True, text=True, timeout=timeout).stdout


def wait_for(predicate, seconds, step=0.5):
    end = time.time() + seconds
    while time.time() < end:
        value = predicate()
        if value:
            return value
        time.sleep(step)
    return predicate()


def alive(pid):
    if not pid:
        return False
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    state = sh("ps", "-o", "stat=", "-p", str(pid)).strip()
    return bool(state) and not state.startswith("Z")


def children(pid):
    named = []
    for child in sh("pgrep", "-P", str(pid)).split():
        named.append((int(child), os.path.basename(sh("ps", "-o", "comm=", "-p", child).strip())))
    return named


def helper_of(gui_pid):
    found = [pid for pid, name in children(gui_pid) if name == HELPER_NAME]
    return found[0] if found else None


class Profile:
    def __init__(self, label):
        self.name = f"delivery-{label}-{RUN}"
        self.data = os.path.expanduser(f"~/Library/Application Support/app.uniclipboard.desktop-{self.name}")
        self.gui = None
        self.count = 0

    def conn(self):
        try:
            return json.load(open(os.path.join(self.data, "daemon.conn")))
        except (FileNotFoundError, json.JSONDecodeError):
            return None

    def start(self, env_extra=None):
        self.count += 1
        env = dict(os.environ, UC_PROFILE=self.name, UC_DISABLE_SINGLE_INSTANCE="1")
        env.pop("UC_GPUI_QUICK_PANEL", None)
        env.update(env_extra or {})
        log = open(os.path.join(OUT, f"{self.name}-gui{self.count}.log"), "w")
        self.gui = subprocess.Popen([GUI], env=env, stdout=log, stderr=log, stdin=subprocess.DEVNULL)
        return self.gui.pid

    def api(self, method, path, body=None):
        c = self.conn()
        base = f"http://{c['host']}:{c['port']}"

        def call(m, p, b, headers):
            req = urllib.request.Request(base + p, method=m, data=json.dumps(b).encode() if b is not None else None)
            req.add_header("content-type", "application/json")
            for k, v in headers.items():
                req.add_header(k, v)
            try:
                with urllib.request.urlopen(req, timeout=20) as r:
                    return r.status, r.read()
            except urllib.error.HTTPError as e:
                return e.code, e.read()

        status, raw = call("POST", "/auth/connect", {"pid": os.getpid(), "clientType": "cli"}, {"authorization": "Bearer " + c["token"]})
        token = json.loads(raw)["data"]["sessionToken"]
        status, raw = call(method, path, body if body is not None else ({} if method == "POST" else None), {"authorization": "Session " + token})
        return status, raw.decode(errors="replace")

    def stop_daemon(self):
        c = self.conn()
        if c and alive(c["pid"]):
            os.kill(c["pid"], signal.SIGTERM)
            wait_for(lambda: not alive(c["pid"]), 15)


def rss_mb(pid):
    kb = sh("ps", "-o", "rss=", "-p", str(pid)).strip()
    return round(int(kb) / 1024, 1) if kb else None


def cpu(pid):
    return sh("ps", "-o", "pcpu=", "-p", str(pid)).strip()


def display_awake():
    swift = 'import CoreGraphics; print(CGDisplayIsAsleep(CGMainDisplayID()))'
    return sh("swift", "-e", swift).strip() == "0"


def panel_windows(pid):
    """Windows of the helper that are on screen (by CGWindowList)."""
    swift = (
        'import CoreGraphics;import Foundation;'
        'let w=CGWindowListCopyWindowInfo([.optionOnScreenOnly],kCGNullWindowID) as! [[String:Any]];'
        f'for i in w where (i[kCGWindowOwnerPID as String] as? Int32)==Int32({pid}) {{'
        'let b=i[kCGWindowBounds as String] as? [String:Any] ?? [:];'
        'print("\\(i[kCGWindowName as String] ?? "")|\\(i[kCGWindowLayer as String] ?? 0)|\\(b["Width"] ?? 0)x\\(b["Height"] ?? 0)|\\(i[kCGWindowAlpha as String] ?? 1)")}'
    )
    lines = [l for l in sh("swift", "-e", swift, timeout=60).splitlines() if l.strip()]
    return [l for l in lines if "|" in l and not l.split("|")[2].startswith("0x") and not l.startswith("|")]


def shot(name):
    path = os.path.join(OUT, name)
    sh(PEEKABOO, "image", "--mode", "screen", "--path", path, "--no-remote")
    return path


def hotkey(keys):
    sh(PEEKABOO, "hotkey", keys, "--no-remote")


def gui_visible(pid):
    return sh("osascript", "-e", f'tell application "System Events" to get visible of (first process whose unix id is {pid})').strip() == "true"


def hide_gui(pid):
    sh("osascript", "-e", f'tell application "System Events" to set visible of (first process whose unix id is {pid}) to false')
    time.sleep(1.5)


def front(pid):
    """Makes this process the frontmost one; after a SIGKILL the dead instance can still hold it."""
    sh("osascript", "-e", f'tell application "System Events" to set frontmost of (first process whose unix id is {pid}) to true')
    time.sleep(1)


def ax_counts(pid):
    """(windows, status bar items) of a process, from the accessibility tree."""
    script = (
        f'tell application "System Events" to tell (first process whose unix id is {pid})\n'
        'return ((count of windows) as text) & "," & ((count of menu bar items of menu bar 2) as text)\nend tell'
    )
    out = subprocess.run(["osascript", "-e", script], capture_output=True, text=True).stdout.strip()
    try:
        windows, items = (int(v) for v in out.replace(" ", "").split(","))
        return windows, items
    except ValueError:
        return None, None


def click_in_window(pid, x, y):
    """Clicks at a point relative to the process's first window, through System Events. peekaboo
    refuses coordinate clicks after a SIGKILL because LaunchServices still names the dead
    instance as the frontmost one."""
    front(pid)
    script = (
        f'tell application "System Events" to tell (first process whose unix id is {pid})\n'
        f'set p to position of window 1\nclick at {{(item 1 of p) + {x}, (item 2 of p) + {y}}}\nend tell'
    )
    result = subprocess.run(["osascript", "-e", script], capture_output=True, text=True)
    return (result.stdout + result.stderr).strip().replace("\n", " ")


def click(pid, x, y):
    front(pid)
    sh(PEEKABOO, "click", "--pid", str(pid), "--coords", f"{x},{y}", "--foreground", "--no-remote")
    time.sleep(1.2)


def onboard_in_gui(pid):
    """The first-run flow a user sees: first device, passphrase twice, create, later. The
    coordinates are for the default 900 x 600 main window."""
    click(pid, 450, 262)
    click(pid, 430, 378)
    sh(PEEKABOO, "type", PASSPHRASE, "--no-remote")
    click(pid, 430, 486)
    sh(PEEKABOO, "type", PASSPHRASE, "--no-remote")
    click(pid, 602, 546)
    time.sleep(6)
    click(pid, 434, 405)


def main():
    say("delivery check", RUN, "| app:", APP)
    say("host:", sh("sw_vers", "-productVersion").strip(), sh("uname", "-m").strip())
    check("display is awake", display_awake(), "wake it first (caffeinate -u -d) and keep hands off")

    # 0. The package carries the helper.
    helper_exe = os.path.join(APP, "Contents/MacOS", HELPER_NAME)
    check("package contains the helper next to the app executable", os.access(helper_exe, os.X_OK), helper_exe)
    check("package contains the daemon", os.access(os.path.join(APP, "Contents/MacOS/uniclipd"), os.X_OK))
    open(os.path.join(OUT, "package-contents.txt"), "w").write(sh("ls", "-la", os.path.join(APP, "Contents/MacOS")))

    # 1. A first install: brand-new profile, no environment variable. Nothing is set up, so
    #    content is locked and no helper may run.
    p = Profile("first")
    gui = p.start()
    check("fresh profile: daemon came up", bool(wait_for(p.conn, 90)))
    time.sleep(20)
    check("fresh profile (nothing set up, locked): no helper is running", helper_of(gui) is None)

    # 2. Setting up in the GUI unlocks content: the helper starts by itself, with the default
    #    settings. (A setup done through the daemon API alone does not give the GUI its grant.)
    shot("first-run-onboarding.png")
    onboard_in_gui(gui)
    shot("first-run-after-onboarding.png")
    check("space initialised through the GUI", p.api("GET", "/encryption/state")[1].find('"initialized":true') >= 0)
    helper = wait_for(lambda: helper_of(gui), 20)
    check("first install default: the helper starts once content is unlocked", bool(helper), f"pid {helper}")
    if helper:
        time.sleep(3)
        port = p.conn()["port"]
        check("helper is connected to this profile's daemon", f":{port}" in sh("lsof", "-a", "-p", str(helper), "-i", "tcp", "-n", "-P"))

    # 3. Locking while the panel is hidden stops the helper; unlocking starts it again.
    status, _ = p.api("POST", "/encryption/lock")
    check("daemon session locked", status == 200)
    check("locked while hidden: the helper is stopped", bool(helper) and bool(wait_for(lambda: not alive(helper), 12)), f"pid {helper}")
    check("locked: no helper remains under the GUI", helper_of(gui) is None)
    status, _ = p.api("POST", "/encryption/unlock")
    helper = wait_for(lambda: helper_of(gui), 15)
    check("unlocked again: the helper starts again", bool(helper), f"pid {helper}")

    # 4. Locking while the panel is shown stops the helper and clears what it showed.
    hide_gui(gui)
    time.sleep(2)
    hotkey("cmd,ctrl,v")
    time.sleep(3)
    shown = panel_windows(helper) if helper else []
    open(os.path.join(OUT, "panel-windows-shown.txt"), "w").write("\n".join(shown))
    shot("shown-before-lock.png")
    check("panel is on screen before locking", bool(shown), shown)
    p.api("POST", "/encryption/lock")
    check("locked while shown: the helper is stopped", bool(helper) and bool(wait_for(lambda: not alive(helper), 12)))
    time.sleep(1)
    shot("shown-after-lock.png")
    check("locked while shown: no panel window is left on screen", bool(shown) and not panel_windows(helper))
    p.api("POST", "/encryption/unlock")
    helper = wait_for(lambda: helper_of(gui), 15)
    check("unlocked: helper back", bool(helper))

    # 5. GUI crash: the helper must not outlive it. The daemon keeps running by design.
    daemon_pid = p.conn()["pid"]
    os.kill(gui, signal.SIGKILL)
    gui_gone = wait_for(lambda: p.gui.poll() is not None, 10)
    check("GUI killed (SIGKILL)", gui_gone is not None)
    check("no helper is left after a GUI crash", bool(helper) and bool(wait_for(lambda: not alive(helper), 10)), f"pid {helper}")
    check("daemon survives a GUI crash (by design)", alive(daemon_pid))

    # 6. GUI restart: the GUI grant starts closed (auto unlock is off), so no helper until the
    #    user unlocks in the GUI. Then the real Unlock button starts it.
    gui = p.start()
    time.sleep(25)
    wait_for(lambda: "UniClipboard - ID" in sh(PEEKABOO, "list", "windows", "--pid", str(gui), "--no-remote"), 30)
    check("restart with content locked in the GUI: no helper is started", helper_of(gui) is None)
    check("...while the daemon session itself is ready (why the gate is GUI-side)", p.api("GET", "/search/query?query=&limit=1")[0] == 200)
    shot("restart-locked-gui.png")
    helper = None
    for attempt in range(1, 4):
        say(f"unlock click attempt {attempt}:", click_in_window(gui, 450, 371)[:200])
        helper = wait_for(lambda: helper_of(gui), 10)
        if helper:
            break
    check("Unlock button in the GUI starts the helper", bool(helper), f"pid {helper}")
    shot("restart-unlocked-gui.png")

    # 7. Closing the main window destroys its WebView; the tray and the helper stay.
    if helper:
        rss_gui_before, rss_helper_before = rss_mb(gui), rss_mb(helper)
        say("close click:", click_in_window(gui, 16, 20)[:200])
        time.sleep(5)
        gui_log = open(os.path.join(OUT, f"{p.name}-gui{p.count}.log"), errors="replace").read()
        check("main window closed: the app stays resident", alive(gui))
        check("main window closed: the log says the webview was destroyed", "webview destroyed, app stays in tray" in gui_log)
        check("main window closed: the helper keeps running", alive(helper))
        windows, items = ax_counts(gui)
        check("main window closed: the GUI has no window left (webview destroyed)", windows == 0, f"windows={windows}")
        check("main window closed: the tray item is still there", bool(items), f"status items={items}")
        time.sleep(10)
        say("footprint after close | GUI rss MB:", rss_gui_before, "->", rss_mb(gui), "| helper rss MB:", rss_helper_before, "->", rss_mb(helper),
            "| helper cpu%:", cpu(helper))
        hotkey("cmd,ctrl,v")
        time.sleep(3)
        shot("panel-with-main-window-closed.png")
        check("main window closed: the shortcut still opens the panel", bool(panel_windows(helper)))
        hotkey("cmd,ctrl,v")
    os.kill(gui, signal.SIGTERM)
    check("GUI exits on SIGTERM", bool(wait_for(lambda: p.gui.poll() is not None, 20)))
    check("clean quit: the helper is stopped", bool(helper) and bool(wait_for(lambda: not alive(helper), 10)))
    p.stop_daemon()

    # 8. Opt-out: UC_GPUI_QUICK_PANEL=0 keeps the WebView panel and never starts the helper.
    q = Profile("optout")
    gui = q.start({"UC_GPUI_QUICK_PANEL": "0"})
    check("opt-out profile: daemon came up", bool(wait_for(q.conn, 90)))
    time.sleep(10)
    onboard_in_gui(gui)
    check("opt-out profile: space initialised through the GUI", q.api("GET", "/encryption/state")[1].find('"initialized":true') >= 0)
    time.sleep(10)
    check("UC_GPUI_QUICK_PANEL=0: no helper even with content unlocked", helper_of(gui) is None)
    os.kill(gui, signal.SIGTERM)
    wait_for(lambda: q.gui.poll() is not None, 20)
    q.stop_daemon()

    say(f"RESULT {sum(passed)}/{len(passed)} checks passed")
    return 0 if all(passed) else 1


sys.exit(main())
