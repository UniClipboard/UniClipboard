# Brief: Quick Panel Layer Shell

Date: 2026-09-08
Status: Locked for implementation

## Recommendation
Keep the existing Tauri-managed WebviewWindow. Install a scoped GTK Application::window-added callback around its synchronous main-thread construction, initialize Layer Shell before realization, then disconnect the hook. Use native Layer Shell positioning and keyboard interaction for supported Wayland compositors.

## Evidence
- GTK3 prototype on Hyprland 0.56.1 confirms window-added sees an unrealized ApplicationWindow, and Layer Shell initialization succeeds.
- Tao 0.35.2 constructs ApplicationWindow with the application property before setting title, size or visibility. The existing panel is hidden at creation.
- Official API requires initialization before realization: https://github.com/wmww/gtk-layer-shell/blob/master/include/gtk-layer-shell.h
- Upstream post-build integration is unresolved: https://github.com/tauri-apps/tao/issues/1315
- GTK signal contract: https://docs.gtk.org/gtk3/signal.Application.window-added.html

## Constraints and ownership
- The creation hook must be one-shot, scoped, disconnected on errors, and verify the exact returned window was initialized. No global unfiltered window conversion.
- GTK/Layer Shell integration belongs in uc-tauri. Hyprland IPC belongs in uc-desktop and must not depend on GUI libraries.
- Dynamically load GTK3 libgtk-layer-shell.so.0 so unsupported desktops and X11 do not acquire a mandatory runtime dependency. Keep its code loaded while GTK can invoke its callbacks.
- Supported Wayland uses Layer Shell exclusively; unavailable library/protocol uses the ordinary desktop backend with an observable capability message.
- Overlay layer, no reserved work area. Keep Exclusive keyboard mode while visible. Owned transparent GTK input surfaces beneath the panel on each output dismiss on outside click; they contain no WebView and are destroyed when the panel hides/dies. Experiments rejected switching to OnDemand (pointer refocus lost keyboard) and Exclusive alone (outside clicks did not dismiss).
- Use output-local logical coordinates and keep the history anchor stable when preview expands; stop using Tauri global window positioning on layer surfaces.
- Hyprland focus/paste must use bounded native Unix socket IPC, validate window addresses, and confirm focus before sending a shortcut. No shell interpolation or clipboard content in IPC commands/logs.
- Existing --quick-panel remains the compositor-owned shortcut entry point; document Omarchy Lua binding and capability limits. Do not silently advertise X11 shortcut registration as working on Wayland.
- Alternative rejected: transplanting WebView into an unmanaged GTK window loses Tauri window ownership/lifecycle. Upstream forks are unnecessary if the scoped GTK hook survives the actual Tauri smoke test.

## Checklist
- [x] GTK lifecycle prototype
- [x] Tauri lifecycle smoke test
- [x] Layer Shell creation/show/hide/layout backend
- [x] Hyprland focus/paste adapter
- [x] Shortcut capability handling and Omarchy integration guidance
- [x] Focused unit tests and isolated host validation (physical pointer/IME/multi-output acceptance remains)
