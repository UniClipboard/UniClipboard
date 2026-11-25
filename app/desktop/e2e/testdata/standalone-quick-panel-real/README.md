# Standalone Quick Panel Real-VM E2E Suite (0033)

Checked-in end-to-end verification drivers for Decision `0033` (Standalone Quick Panel) and Wayland automatic paste across all four real desktop environments. Each driver runs the untouched production `unic`, `unicd`, and `unic-desktop` binaries inside an isolated VM with host clipboard sharing disabled, private `UNIC_HOME`/`UNIC_SOCK`/`XDG_*` directories, real OS clipboard servers, real external target editors, and real physical/compositor input, writing a `manifest.json` receipt and `SHA256SUMS`.

## Suites

| Case | Script | Environment | What it verifies |
|---|---|---|---|
| **N01a / N01b** | `linux_wayland_e2e.py` | Linux Wayland (`--mode kwin` / `--mode niri`) | Standalone `unic-desktop --quick-panel` (only `unicd` + `quick-panel`), refocus, Copy fallback & windowless `wl-paste` persistence for UTF-8 text / PNG / managed file after 0-window dismiss, F13 delete-while-held + external `wl-copy` replacement releasing file cache lease, Escape, Tray coexistence (zero echo) & SIGTERM exit preservation; in `--mode niri`, real cross-app `zwp_virtual_keyboard_v1` `Enter` single-paste and `Ctrl+Enter` keep-open second paste into an external PyGTK3 `Gtk.TextView`. |
| **N02** | `linux_x11_e2e.py` | Linux native X11 (`Xvfb +extension XTEST` + EWMH WM e.g. `openbox`, `WAYLAND_DISPLAY` unset) | Standalone `unic-desktop --quick-panel`, repeat refocus, real XTest `Return` cross-app single-paste into an external PyGTK3 `Gtk.TextView`, windowless X11 CLIPBOARD readback for text / PNG / managed file, F13 delete-while-held + external `xsel -ib` replacement releasing file cache lease, Tray coexistence (zero echo) & SIGTERM exit preservation. |
| **N03** | `windows_e2e.ps1` | Windows 11 interactive desktop (`powershell.exe -NoProfile -STA`) | Cold-start & standby hot re-entry `AllowForeground`, standalone processes, `SendInput` `Enter` cross-app single-paste into a maximized external WinForms `TextBox` without unmaximizing (`IsZoomed` preserved), minimized target `SW_RESTORE` + paste, windowless OLE clipboard readback for text / PNG / `FileDropList` managed file, F13 delete-while-held + external OLE replacement releasing file cache lease, Tray coexistence (zero echo) & Tray quit preservation. |
| **N04** | `macos_e2e.go` | macOS VM interactive desktop (host clipboard sharing OFF; zero Xcode/Python dependency) | Standalone headless `unicd` does NOT capture pre-existing `NSPasteboard` on startup; first on-demand activation enables `nativeboard` without broadcasting pre-existing pasteboard; real `NSPasteboard` readback for text / `public.png` / `public.file-url` managed file; F13 delete-while-held + external `pbcopy` replacement releasing file cache lease; Tray coexistence (zero echo) & Tray exit preservation; cross-app `CGEvent` auto-paste into external AppKit `NSTextView` when VM Accessibility (`AXIsProcessTrusted`) is granted. |

## Invocation Examples

### N01 (Linux Wayland, inside Linux VM)

```bash
# KWin (N01a)
python3 linux_wayland_e2e.py --mode kwin --bin-dir /path/to/bin --out-dir /path/to/out/n01a --work-dir /tmp/work-n01a

# Niri virtual-keyboard auto-paste (N01b)
python3 linux_wayland_e2e.py --mode niri --bin-dir /path/to/bin --out-dir /path/to/out/n01b --work-dir /tmp/work-n01b
```

### N02 (Linux X11, inside Linux VM)

```bash
dbus-run-session -- bash -lc '
  export DISPLAY=:97 XDG_SESSION_TYPE=x11 GDK_BACKEND=x11
  unset WAYLAND_DISPLAY
  Xvfb :97 -screen 0 1280x800x24 +extension XTEST -nolisten tcp &
  openbox &
  sleep 1
  python3 linux_x11_e2e.py --bin-dir /path/to/bin --out-dir /path/to/out/n02 --work-dir /tmp/work-n02
'
```

### N03 (Windows 11, inside Windows VM interactive session)

```powershell
# Build windows-native-helper.exe from app/desktop/e2e/testdata/windows-native/helper.go into BinDir alongside unic.exe, unicd.exe, unic-desktop.exe.
powershell.exe -NoProfile -STA -ExecutionPolicy Bypass -File windows_e2e.ps1 `
  -BinDir C:\ut0032\bin -OutDir C:\ut0032\out -WorkDir C:\ut0032\w
```

### N04 (macOS, inside macOS VM interactive session)

```bash
# Build darwin/arm64 binaries + macos-e2e on host, copy to VM, then run inside VM:
GOOS=darwin GOARCH=arm64 go build -o macos-e2e ./app/desktop/e2e/testdata/standalone-quick-panel-real/macos_e2e.go
./macos-e2e --bin-dir /tmp/unic-0032/bin --out-dir /tmp/unic-0032/out --work-dir /tmp/unic-0032/w
```

> **macOS Accessibility note**: On macOS 15/26, TCC hard-denies `kTCCServiceAccessibility` for remote SSH sessions (`responsible=/usr/libexec/sshd-keygen-wrapper`, `Auth Right: Denied (System Set)`). Without `--require-accessibility`, `macos-e2e` verifies real `NSPasteboard` on-demand activation, text/PNG/file readback, F13 lease release, and tray coexistence directly over SSH. To also run physical `CGEvent` cross-app `Cmd+V` auto-paste into `TextEdit.app` (`--require-accessibility`) without re-authorizing ad-hoc rebuilds, grant Accessibility once to Apple-signed `/System/Applications/Utilities/Terminal.app` inside the VM and launch the runner via `open -a Terminal <script.command>`.
