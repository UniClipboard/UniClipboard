# Desktop UI process lifecycle

## Process ownership

macOS, Windows and Linux use one resident tray host and one on-demand UI child.
Both roles use the same compiled Go program. On macOS, the UI child is packaged in
`Contents/Helpers/UniClipboardUI.app` with an independent `<bundle-id>.ui` identity. Finder and
login items target the outer application. Directly opening the UI helper without inherited
control pipes is refused before Wails application creation. Windows and Linux start the resident
executable with the internal `--ui-child` role. No extra binary or portable directory is required.

The resident host owns the application single-instance lock, tray, daemon startup and shutdown,
login-item reconciliation, native Quick Panel supervisor, system notifications and update
scheduling/downloads. It creates no WebView. The UI child only attaches to the existing daemon.
It owns the main window, updater window, fallback WebView Quick Panel and visual-effects session.
Its single-instance scope is separate from the resident host's scope.

Closing the main window exits the entire UI child, including any updater or fallback-panel
window. The tray host, native Quick Panel helper and daemon continue. Tray Lightweight Mode
also stops the UI child. Tray Open, Settings or a Finder reopen creates one new UI child when
needed. A concurrent reopen waits for the closing child to exit. Child process handles, rather
than process-name searches, govern shutdown and reaping. A UI crash does not spawn an unsolicited
window; the next explicit open can create a fresh child.

The parent and child exchange typed operations over private inherited pipes. No listener,
credential file or persistent runtime-mode flag is added. On macOS and Linux, Go inherits descriptors 3 and 4. Windows uses an explicit inherited-handle
list because Go does not support ExtraFiles there. The child clears handle inheritance. Parent EOF ends the UI child. Full Quit, Cmd-Q, restart, updater install, autostart
and Quick Panel shortcut changes route to the resident owner. The existing frontend host API
and daemon API remain unchanged. Both Go processes append to the existing GUI log with distinct
PID and run_id fields.

The resident also owns the desktop theme preference and palette watcher. UI theme commands use
the private channel. The UI saves the main window size before exit; before a daemon restart,
the resident requests an existing UI to flush its window preference without opening one.

The saved `general.startupMode` is a launch preference owned by the daemon's Desktop settings
store (ADR-013). Closing the UI never writes it. Background update checks and downloads continue,
but scheduled prompts do not create a UI child while the application is windowless. Manual
update checks can open the updater. The native Quick Panel remains resident. The WebView fallback
is created on explicit use; dismissing its last UI closes the child.

The child has its own WebKit storage identity. Existing WebKit caches/localStorage under the
outer bundle identity are not copied or deleted. Daemon-owned settings and encrypted application
data stay in the same profile. The new UI identity starts its own frontend caches. Check legacy
UI-only cached preferences during manual acceptance.

Windows and Linux keep the resident event loop alive without windows. Linux gives the two roles
separate profile-scoped GTK application IDs. The surface program name keeps the product bundle ID.
The default native Quick Panel on Windows stays resident. Linux uses the on-demand WebView panel.
Windows keeps its existing WebView2 data directory. Linux keeps its existing frontend store.
The UI process inherits the AppImage environment and runs inside the resident's mount.
Only a whole-app restart or update launches the outer AppImage.

The shared lifecycle is unconditional. There is no platform flag or old single-host fallback.
Platform-specific code only supplies inherited pipes and the macOS helper-bundle path.

## Why window Close was insufficient

Wails Close removes the native window and registry entry. It does not guarantee that all WebKit
auxiliary processes exit. WebKit's default data store may outlive the last view. Apple also states
that multiple WKProcessPool objects no longer affect process allocation. The new UI-process
boundary ends the owner of those WebKit resources instead of deleting persistent data or calling
private/test process-termination APIs.

Native acceptance must still prove the actual WebContent, GPU and Networking process exit.
A successful build or a reaped UI PID is not that proof. The source research and alternatives are
in `.planning/research/macos-webkit-lifecycle/BRIEF.md`.

## Verification

Build without opening a GUI:

```bash
bun install --frozen-lockfile
bash apps/gui-go/build.sh e2e
```

Run the E2E-only inherited-pipe check. It exits before App.New, daemon bootstrap and profile access:

```bash
target/gui-go/UniClipboardGoE2E.app/Contents/MacOS/gui-go --ui-channel-probe
```

Do not run native interaction unless the user explicitly authorizes it or the machine is dedicated
to testing. The following driver shows windows, clicks the tray and reads native application state:

```bash
python3 apps/gui-go/e2e/ui_process_run.py --allow-session --out target/gui-go/ui-process-evidence
```

The driver uses a temporary HOME, a unique profile and no system clipboard. It checks red-close,
Cmd-W, tray Lightweight Mode, Finder reopen, tray Open, tray Quit and Cmd-Q. It requires a stable
resident host, daemon and native-helper PID, a fresh UI PID on reopen, a working page-to-daemon
round trip, no app-owned WebKit helpers after closing, and an unchanged startup preference.
`ui-process-assertions.json` contains the source/binary hashes and observations. The old
`dock_lifecycle_run.py` entry forwards to this driver. Native interaction and real cross-device
sync have not been executed as part of this change.


For Windows and Linux, build the E2E executable with the correct platform frontend and place
`gui-go[.exe]`, `uniclip[.exe]`, `uniclipd[.exe]` and the native helper when available in one directory.
On an explicitly authorized dedicated graphical host, run:

```bash
UC_GUI_GO_E2E_DEDICATED_HOST=1 python3 apps/gui-go/e2e/ui_process_portable_run.py --allow-session --binaries <directory> --out <new-evidence-directory>
```

Set the same environment variable in PowerShell before running the command on Windows. The driver
copies binaries into a new portable sandbox. It verifies three native Close/reopen cycles, real
page-to-daemon calls, an unchanged startup preference, stable resident/daemon/native-helper identities,
retained tray state and disappearance of positively attributed browser processes. It alternates
second-launch and tray Open handlers. It records a source manifest and binary hashes in
`ui-process-assertions.json`. Native OS tray clicks, keyboard interactions, installer/AppImage updates
and cross-device synchronization remain separate acceptance gates.

The in-page smoke scenario no longer tries to execute JavaScript after closing its own process.
The external lifecycle drivers own those assertions. The helper-request scenario uses an explicit
Hide to verify that helper Open restores an existing window. Hide is not used for main-window Close.
