# Desktop quick-panel system contract

Status: independent process integration implemented; native acceptance is a separate gate. Scope: Desktop host; Engine and headless services keep no OS target identity.

## Public entry, admission, and activation

`unic-desktop --quick-panel` is a public Desktop entry that opens or focuses the profile's single Quick Panel process using the same executable and profile-scoped role lock (`quick-panel.lock`) as internal activation.

- **Argument rules**: `--quick-panel` cannot be combined with `--role`, `--command`, `--background`, `--mode`, or positional arguments; invalid combinations fail immediately during local argument parsing with a non-zero exit code without touching sockets, configuration, or lock files. `--version` and `--version --json` continue to execute before initialization.
- **Service borrowing (never implicit launch)**: `--quick-panel` resolves the profile using the shared path rules (`UNIC_HOME`, `UNIC_SOCK`, `portable.dat`, default/dev directories) without creating missing directories, and probes `GET /v1/service` with a 1-second timeout. If no service responds, the socket is unreachable or unrecognized, `service.Directory` does not match the canonical profile, or the service is `retiring` or `cli-temporary`, `--quick-panel` exits non-zero with a direct error and never starts `unicd`, `tray`, or `gui`.
- **Read-only setup and migration admission**: before activating, `--quick-panel` checks `desktop-setup.json`, `cli-migration.json`, and `unic.db` in read-only mode (creating no directory or marker) and probes an online `tray` for migration pause state. Uninitialized profiles, incomplete setup (`complete: false`), pending CLI migration (`status: "pending"`), corrupt state files, or an active tray migration pause are rejected with guidance to finish setup or migration. When an online tray handles a global shortcut during `setup_required`, it preserves its existing behavior of opening the GUI setup window.
- **Unified activation actions**: `--quick-panel`, tray menu, main-window button, and global shortcuts share one activation path with three explicit actions:
  - `open` (`--quick-panel`, tray menu, main-window button): if the panel is already visible, focuses the existing window (`AllowForeground` + `Show` + `Focus`) without recapturing or overwriting the captured paste target and without hiding the window. Simultaneous `--quick-panel` launches converge on one panel PID and one `unicd` generation.
  - `toggle` (global `TogglePanel` shortcut): dismisses an already visible panel (restoring target focus only if `unic` still owns foreground), or captures target and opens when hidden.
  - `favorites` (global `ShowFavorites` shortcut): switches a visible panel to the Favorites filter without recapturing the target, or captures target and opens filtered when hidden.
- **Pre-activation target capture**: when opening a hidden panel, the activator captures the external foreground target before showing/focusing the panel window and rejects any PID belonging to the current profile's Desktop roles (self, `gui`, `quick-panel`, `tray`).
- **Opening main window / settings and lifetimes**: invoking `main` or `settings` from the panel calls the tray's `open-gui` when a tray is online, or invokes the normal Desktop launcher to start `tray` + `gui` and navigate to the requested destination when running standalone. Closing the panel window or exiting the panel process never stops a borrowed `unicd`. When a tray borrowing a standalone `unicd` exits on its own (without explicit full-application Quit, lightweight GUI close, service restart, or migration pause), it releases its global shortcuts and background observer without terminating an independent Quick Panel; subsequent panel copies seamlessly use the no-observer service coordination path.

## Session and commands

The activator (the tray or `unic-desktop --quick-panel`) captures the external foreground target before creating or activating the independent quick-panel OS process. Send the target only through private, profile-bound Desktop IPC. The panel validates and uses that original target before showing the panel. On macOS, Windows, and X11, a target is one process/window instance, never a name-based lookup. On Wayland, where compositors do not expose external window IDs/PIDs to clients, an eligible Wayland input backend captures a compositor-managed focus-return target (with no synthetic external PID) only when `unic` itself does not own focus. Opening from `unic` itself leaves paste unavailable and preserves Copy. Never recapture while the panel is visible. Reopening starts a new generation and invalidates pending work.

Enter pastes. Command+1–9 on macOS and Ctrl+1–9 on Windows/Linux select and paste the corresponding visible result through the same controller (or apply the matching relaxation proposal when the filtered list is empty). Shift+Enter requests the existing builtin.text/plain Action, applies its new history item, then pastes. A stored plain-text representation is required; HTML-only history receives an explicit rejection without a transform or clipboard write. Images/files cannot use plain mode. Command/Ctrl+Enter pastes and reopens the panel with the original target after a 150 ms dispatch interval and a fresh focus check. A user switch during that interval leaves the panel hidden. Copy applies history without synthetic input. Explicit filter prefixes (`#`, `/`, `@`, `on:`, `ext:`) open suggestions with the first candidate highlighted for immediate `↑`/`↓` (`Ctrl+P`/`Ctrl+N`) and `Enter`/`Tab` selection (`Shift+Tab` moves backward in open suggestions or cycles content types including folders when suggestions are closed; `Escape` dismisses suggestions before clearing filters or closing). Full keyboard navigation also covers: `quickPanel.favoritesFilter` (default `Cmd/Ctrl+F`) to toggle the Favorites-only filter; action menu navigation (`↑`/`↓`/`→`/`←`/`Enter`/`Escape`) while blocking search text edits; `PageUp`/`PageDown`/`Home`/`End` list paging and `Alt+←`/`→` image-grid stepping while typing; immediate preview toggle (`Ctrl+Space`), preview scrolling (`Alt+↑`/`↓` and `Shift+PageUp`/`PageDown`), and image 1:1 preview zoom (`Cmd/Ctrl+0`). Auxiliary controls retain input focus so keyboard commands never stall.

Before handing back keyboard focus, validate the captured target and native permission. Prepare an authorized Wayland `RemoteDesktop` session if required. Apply history through the service-coordinated clipboard path and confirm one tracked write through read-back verification. On macOS this path uses the daemon adapter in `unicd` with on-demand opt-in. On Windows and Linux it uses the panel's mygo board authorized by `unicd`'s `/v1/desktop/panel-apply/*` endpoints.

- **Windows**: Request activation of the captured window while the panel process still owns the foreground. Hide the panel and preview after the activation request succeeds. A denied activation sends no paste input and leaves the panel available while it retains focus. The final focus check can wait through a transient empty foreground.
- **macOS**: The Quick Panel is a nonactivating window. It can hold keyboard focus while the captured application remains frontmost. Hide the panel and preview to relinquish keyboard focus. Verify the same application is still frontmost before sending input.
- **X11 and Wayland**: Hide the panel and preview first. On X11, activate the captured window. On Wayland, let the compositor return focus after unmap and confirm the panel lost keyboard focus.

Use a one-second deadline to wait for restored focus and released trigger keys and modifiers. Recheck the target and clipboard immediately before sending one paste chord. An operating-system send acknowledgement confirms dispatch. Acceptance also requires evidence that the target inserted the expected content. The panel displays operation failures while visible and retains the latest failure from a hidden operation until the next opening.

Cancellation, external blur, a newer session, shutdown, failed apply, uncertain apply completion, missing permission, target exit/reuse, focus theft, clipboard replacement and partial input injection must never retry paste or replay the clipboard write. Clipboard application may already have completed after cancellation; do not roll it back or report it as pasted. Explicit Escape dismissal can restore focus only while unic still owns the foreground. External blur never restores focus. Hide/reopen and mutation completion must remain generation-bound.

## Shortcut preference

The tray registers system-wide shortcuts with mygo.GlobalShortcut while Desktop
integration runs. The default panel binding is Command+Ctrl+V on macOS and
Ctrl+Alt+V on Windows/Linux. The optional Show Main Window and Open Favorites
in Quick Panel actions are unbound by default. Opening favorites on a visible
panel changes its filter without recapturing the target. The normal panel
shortcut still dismisses an already visible panel.

Settings stores recorded alternatives in `desktop.shortcuts`. Each binding
contains one or two space-separated key combinations. A two-step sequence
expires one second after its first combination. An empty array disables an
action. An absent entry uses the action default. For panel activation only, the
existing `desktop.quick_panel_shortcut` preset remains the fallback. Settings
rejects overlapping bindings and prefixes within each application window and
within system-wide bindings. A step claimed by a system-wide binding cannot
also belong to a window binding. History and main window scopes overlap.
The two independent windows may share bindings.

The shortcut editor opens beside the selected binding. Unmodified Enter
confirms the recorded sequence and Escape dismisses the editor. Pressing Enter
without a recorded combination leaves the binding unchanged. The row menu
provides clear and reset actions.

The tray registers the complete replacement set before releasing superseded
registrations. If registration fails, it rolls back added registrations and
reports the previous effective panel binding. A saved preference can differ
from an effective binding. The user can retry registration. On Wayland a mygo
portal request is asynchronous and does not establish user approval. The tray
drops its registrations on pause, explicit quit or lightweight retirement.

`desktop.quick_panel_double_tap` independently enables two standalone presses
of Alt, Ctrl or the platform Super key. The setting defaults to off. The tray
samples only the selected modifier and the presence of other keys while this
feature is active. macOS requires Accessibility permission. Windows and native
X11 support the feature. Wayland cannot observe the global modifier gesture.

The quick-panel process registers no global shortcuts and acquires no background observer adapter session. It opts in on demand and uses the native adapter in `unicd` for tracked writes and clipboard checks. On Linux sessions where the daemon cannot enable the adapter, explicit copy can still use the local Desktop board under `unicd`'s bounded application protocol. The panel checks its local write mark if it used that path.

In normal mode, the tray asynchronously prepares a windowless panel process after the Desktop runtime is ready. An activation can join an in-progress start. When the process is unavailable, an activation (from tray or `unic-desktop --quick-panel`) starts it on demand. Each opening starts a view scope and the first history read while the main window is created. The search field accepts input while history loads. The preview flyout is created when a preview or action menu needs it.

Dismissal destroys both windows without clearing the system clipboard, cancels the history view scope, and drops its caches and timers while the windowless process stays ready. The daemon owns native clipboard data lifetime for adapter writes. A Linux local write can keep its GTK selection source in the windowless panel process until ownership ends. When such a local write holds managed files (`has_files: true`), the panel attaches event-driven ownership/change monitoring (`OnRelease` and `OnChanged` without background polling timers) and calls `/v1/desktop/panel-apply/release` once the clipboard no longer holds those file paths or on process exit; standby without held files retains zero clipboard listeners. A pending paste retains the view only through its focus checks and optional guarded reopen. The next explicit activation starts a new view scope and reads fresh history. Search, filter and zoom may survive in process memory; each new opening resets selection to the first result. History payloads and target identity are released from the standby state.

Normal GUI close retains the tray. Lightweight close and lightweight login
keep only unicd. Manual activation reopens the GUI.

## Platforms and privacy

macOS uses NSWorkspace/NSRunningApplication target identity, Accessibility permission and process-directed CoreGraphics paste input. Windows uses HWND/PID/process creation identity and foreground-checked SendInput. X11 uses the current client window and XTest with foreground checks. Wayland supports automatic paste through sanctioned compositor-managed focus return plus pure-Go input injection without external CLI helpers or root/uinput access: when the Wayland compositor advertises `zwp_virtual_keyboard_manager_v1` + `wl_seat` (such as Niri, Hyprland, Sway, River, Labwc), the panel injects `Ctrl+V` via `zwp_virtual_keyboard_v1`; otherwise, when the session bus exposes `org.freedesktop.portal.RemoteDesktop` with keyboard device support (such as GNOME and KDE Plasma), the panel requests user-consented keyboard input through the portal and stores its opaque `restore_token` (0600, profile-scoped) for subsequent launches. When neither Wayland mechanism is available, or the user declines portal consent, or the panel was opened from `unic` itself, automatic paste falls back cleanly to Copy (including windowless selection serving after dismissal without a clipboard manager) without bypassing desktop security policy.

Never log target names, IDs, window titles, clipboard material, portal restore tokens, or raw native errors. No paste-success analytics is added. Native calls stay behind one Desktop-owned module. Reuse purego/x/sys, godbus/dbus, and mygo. X11 adds github.com/jezek/xgb v1.3.1 to use checked protocol errors and XTest instead of process-global Xlib error handlers or external command-line programs.

## Failure scenarios specified before runtime code

Shortcut conflict/replacement/disable/retry/quit; invalid `--quick-panel` flag/positional combinations; missing/unreachable/mismatched-profile/retiring/`cli-temporary` service rejected without implicit launch; uninitialized/incomplete-setup/pending-migration/paused-tray admission rejected read-only; concurrent and repeated `--quick-panel` convergence without hiding visible panel or overwriting target; standalone panel opening main window/settings without tray; macOS on-demand daemon opt-in without pre-existing clipboard broadcast; Windows/Linux standalone panel copy (text, image, file, plain Action) with and without background observer; zero duplicate write or capture/sync echo alongside tray; tray exit preserving standalone panel over borrowed standalone `unicd`; windowless selection serving after dismissal; managed file lease retention and release upon external clipboard replacement; interrupted/timed-out/stale-generation write completion rejected with zero paste injection and zero replay; external target captured before panel focus; self target rejected; Copy with no target; exact multi-format apply; plain Action removes rich format; missing adapter; duplicate Enter; cancellation during slow apply; session superseded; target exit/reuse; permission denied; held modifier timeout; delayed activation; user focus change; clipboard changed after apply; injected-input failure; keep-open target preservation; external blur without focus theft; Wayland unsupported auto-paste with successful Copy; no renderer or OS clipboard in process fixtures.

Process receipts must bind source and production-service binary hashes, exact directory-clipboard bytes and ordered platform-fixture events. Native OS acceptance remains separate.

## Native references

- [Apple Accessibility trust](https://developer.apple.com/documentation/applicationservices/1460720-axisprocesstrusted) and [CoreGraphics event dispatch](https://developer.apple.com/documentation/coregraphics/cgevent).
- [Windows foreground activation](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setforegroundwindow) and [SendInput](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput).
- [XTest protocol](https://www.x.org/docs/Xext/xtest.pdf) and [XGB XTest bindings](https://pkg.go.dev/github.com/jezek/xgb/xtest).

Focus/clipboard verification immediately precedes input dispatch. The operating system offers no atomic focus-and-paste transaction. The keep-open interval reduces queue/focus interference; it does not acknowledge content insertion. Verify actual receiving applications and keyboard layouts through native acceptance.
