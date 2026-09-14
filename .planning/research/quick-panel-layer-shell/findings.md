# Findings

- Host: Hyprland 0.56.1, Wayland, Omarchy Lua configuration.
- Quick Panel uses a pre-created hidden Tauri WebviewWindow and two-phase show.
- Existing GUI --quick-panel launch argument routes to a queued toggle through single-instance handling.
- Linux paste currently returns unsupported, and global shortcut registration uses the X11-only global-hotkey backend.
- GTK Layer Shell requires validation of the GTK realization lifecycle before adopting it in Tauri.
- No prior Layer Shell brief found.
- Tauri 2.11.1 / Tao 0.35.2 expose the window too late for a supported post-build Layer Shell conversion (upstream tao#1315). GTK Application::window-added is emitted synchronously while constructing ApplicationWindow; a main-thread scoped signal hook is a candidate that retains Tauri window ownership, pending probe.
- gtk-layer-shell is absent on this host; obtain a temporary test runtime without changing system packages. HTTPS ARM mirror endpoints failed certificate/connection checks; no TLS verification bypass used.
- Official references: https://github.com/tauri-apps/tao/issues/1315 ; https://docs.gtk.org/gtk3/signal.Application.window-added.html ; https://github.com/wmww/gtk-layer-shell/blob/master/include/gtk-layer-shell.h
- The actual locked build uses Tauri 2.11.5 and Tao 0.35.3; the same hook was verified using the real Wry runtime, not only GTK. Hyprland reports namespace uniclipboard-quick-panel and the expected 360x580 collapsed geometry.
- Extended smoke passed: native target window -> focused Layer Shell panel -> preview expansion -> native IPC paste to target -> remapped panel. Synthetic key is intercepted in the target before GTK can read the clipboard.
- Runtime test library was extracted only under a temporary directory from an Arch Linux ARM package whose detached signature verified against the installed pacman keyring. System library installation and personal Hyprland configuration were not changed.
- Repeated final smoke exposed intermittent loss of panel keyboard focus after mapping. Host uses input.follow_mouse=1; the cursor was outside the panel. Hypothesis under investigation: immediately changing Exclusive -> OnDemand can release the launcher's focus on compositor pointer refocus. Added focus in/out diagnostics to the isolated smoke; no production workaround yet.
- Controlled experiment: explicitly focused the isolated target, opened panel, then moved pointer outside via Hyprland cursor dispatcher. Panel emitted focus-in then focus-out and the target became active; the OnDemand transition does not preserve launcher focus in this scenario. Testing the standard Exclusive launcher mode as the single-variable alternative, including outside-click dismissal, before deciding whether another input surface is needed.
- Keeping Exclusive for the whole visible interval passed the same pointer-move test and the isolated paste/remap sequence. Removed the focus-in callback that changed keyboard mode. Outside-click dismissal is the remaining validation for this correction.
- The diagnostic harness also needed a nonzero process exit after a GTK timer assertion; AppHandle::exit(1) alone returns from run() and Rust main would otherwise return success. Failures now set an atomic flag and fail outside the GTK callback, avoiding a panic across the C ABI.

- Final smoke includes GTK signal-driven outside-click callback verification and confirms every dismissal surface is removed after hiding. It filters visible newly-created top levels because WebKit may create hidden auxiliary windows.
- The virtual-pointer fixture moved the compositor cursor but delivered no wl_pointer enter/button events to either the panel or an independent ordinary fullscreen GTK window. Physical pointer delivery is unverified; do not label it a confirmed backdrop defect or a successful integration test.
- Correction to smoke failure handling: Tauri can terminate the process before main resumes, so an atomic flag checked after run() was insufficient. Timer check failures now explicitly exit with status 1 without unwinding through GTK C callbacks.
