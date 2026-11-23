# Desktop quick-panel system contract

Status: independent process integration implemented; native acceptance is a separate gate. Scope: Desktop host; Engine and headless services keep no OS target identity.

## Session and commands

The persistent tray captures the external foreground target before creating or activating the independent quick-panel OS process. Send the target only through private, profile-bound Desktop IPC. The panel validates and uses that original target before showing the panel. A target is one process/window instance, never a name-based lookup. Opening from unic itself leaves paste unavailable and preserves Copy. Never recapture while the panel is visible. Reopening starts a new generation and invalidates pending work.

Enter pastes. Shift+Enter requests the existing builtin.text/plain Action, applies its new history item, then pastes. A stored plain-text representation is required; HTML-only history receives an explicit rejection without a transform or clipboard write. Images/files cannot use plain mode. Command/Ctrl+Enter pastes and reopens the panel with the original target after a 150 ms dispatch interval and a fresh focus check. A user switch during that interval leaves the panel hidden. Copy applies history without synthetic input. Preserve current search, chip, preview and keyboard UX.

Order: validate target and native permission; delegate application through private Desktop IPC to the tray's real clipboard adapter; confirm that exactly one tracked clipboard write completed; hide the panel and preview; activate the same target; wait for focus and all command modifiers to be released; recheck target and clipboard; send one paste chord. A one-second focus/modifier deadline bounds the final stage. An OS send acknowledgement is not proof that the receiving application inserted content.

Cancellation, external blur, a newer session, shutdown, failed apply, missing permission, target exit/reuse, focus theft, clipboard replacement and partial input injection must never retry paste. Clipboard application may already have completed after cancellation; do not roll it back or report it as pasted. Explicit Escape dismissal can restore focus only while unic still owns the foreground. External blur never restores focus. Hide/reopen and mutation completion must remain generation-bound.

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

The quick-panel process registers no global shortcuts and owns no clipboard
adapter. It starts on first activation and remains hidden between requests.
Normal GUI close retains the tray. Lightweight close and lightweight login
keep only unicd. Manual activation reopens the GUI.

## Platforms and privacy

macOS uses NSWorkspace/NSRunningApplication target identity, Accessibility permission and process-directed CoreGraphics paste input. Windows uses HWND/PID/process creation identity and foreground-checked SendInput. X11 uses the current client window and XTest with foreground checks. Wayland does not expose equivalent arbitrary target capture/injection; keep Copy and the mygo portal shortcut, and report automatic paste unavailable. Do not bypass desktop security policy.

Never log target names, IDs, window titles, clipboard material or raw native errors. No paste-success analytics is added. Native calls stay behind one Desktop-owned module. Reuse purego/x/sys and mygo. X11 adds github.com/jezek/xgb v1.3.1 to use checked protocol errors and XTest instead of process-global Xlib error handlers or external command-line programs.

## Failure scenarios specified before runtime code

Shortcut conflict/replacement/disable/retry/quit; external target captured before panel focus; self target rejected; Copy with no target; exact multi-format apply; plain Action removes rich format; missing adapter; duplicate Enter; cancellation during slow apply; session superseded; target exit/reuse; permission denied; held modifier timeout; delayed activation; user focus change; clipboard changed after apply; injected-input failure; keep-open target preservation; external blur without focus theft; no renderer or OS clipboard in process fixtures.

Process receipts must bind source and production-service binary hashes, exact directory-clipboard bytes and ordered platform-fixture events. Native OS acceptance remains separate.

## Native references

- [Apple Accessibility trust](https://developer.apple.com/documentation/applicationservices/1460720-axisprocesstrusted) and [CoreGraphics event dispatch](https://developer.apple.com/documentation/coregraphics/cgevent).
- [Windows foreground activation](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setforegroundwindow) and [SendInput](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput).
- [XTest protocol](https://www.x.org/docs/Xext/xtest.pdf) and [XGB XTest bindings](https://pkg.go.dev/github.com/jezek/xgb/xtest).

Focus/clipboard verification immediately precedes input dispatch. The operating system offers no atomic focus-and-paste transaction. The keep-open interval reduces queue/focus interference; it does not acknowledge content insertion. Verify actual receiving applications and keyboard layouts through native acceptance.
