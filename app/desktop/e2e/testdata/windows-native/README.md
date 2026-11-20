# Windows native Desktop process acceptance

This is an opt-in native E2E driver. It is not a substitute for the canonical
`desktop-native-processes` case. It adds real notification-area menu clicks,
WM_CLOSE and one real global-shortcut chord. It uses the existing production
role IPC, local HTTP API and mygo automation protocol.

Do not run until the current VM owner explicitly hands over that exact guest.
The driver never starts, stops or configures the VM. It never uses any system
clipboard API, changes clipboard sharing, installs software or accesses a personal
profile. It never uses Notepad. Its external hotkey target is a private WinForms
window with synthetic text. Unknown native input outcomes are not replayed.

## Files and guest prerequisites

- Built-in Windows PowerShell 5.1, started with `-NoProfile -STA`.
- Built-in .NET UIAutomationClient, UIAutomationTypes, WinForms and System.Drawing.
- Task binaries `unic.exe`, `unicd.exe`, and `unic-desktop.exe` built from the
  reviewed product source. Desktop must use `-tags desktop_e2e`. Do not set
  `UNIC_DESKTOP_E2E_PROCESS_FIXTURE`; this driver requires native event loops.
- `windows-native-helper.exe`, built from the adjacent standard-library Go helper.
- Copy `run.ps1`, `target.ps1` and `native.cs` together. Both PowerShell files have
  UTF-8 BOMs for Windows PowerShell 5.1 Chinese strings.

Build the helper on the authorized host, matching the guest architecture:

```sh
GOOS=windows GOARCH=arm64 go build -o <private-bin>/windows-native-helper.exe \
  ./app/desktop/e2e/testdata/windows-native/helper.go
```

Use `GOARCH=amd64` only for an x64 guest or an explicitly recorded x64-emulation
run. Product binaries and the helper architecture are recorded independently.
The helper does not import Engine, Desktop modules or clipboard packages.

## Inspect first

Parse the copied scripts first without running them:

```powershell
$TaskTokens=$null; $TaskParseErrors=$null
[void][Management.Automation.Language.Parser]::ParseFile(
  (Resolve-Path .\run.ps1).Path, [ref]$TaskTokens, [ref]$TaskParseErrors)
if($TaskParseErrors.Count){ throw ($TaskParseErrors | Out-String) }
```

Use a fresh output directory for every command:

```powershell
powershell.exe -NoProfile -STA -File .\run.ps1 -Action inspect `
  -BinDir C:\task\bin -ArtifactDir C:\task\inspect-01 -SourceSHA a99ccc0bc
```

Inspection records exact-name notification-area candidates and their RuntimeIds.
It also records a bounded class/AutomationId/control-type skeleton below Explorer
taskbar/overflow roots without collecting unrelated element names.
It does not collect unrelated window text. The default exact icon tooltip is
`跨设备剪贴板同步`. If the icon is hidden, inspect with `-OpenOverflow`; the
script uses the observed `显示隐藏的图标` button with class
`SystemTray.NormalButton` and the `TopLevelWindowForOverflowXamlIsland` root.
It waits for stable bounds and cross-checks the task icon against
`Shell_NotifyIconGetRect`, the exact tooltip and the visible notification root.
It does not treat the fold button rectangle as the task icon. It does not guess
other localized buttons or coordinates. If this guest exposes another UIA structure,
stop and inspect that structure before updating the driver. Do not bypass guards.

Run records candidates both before and after launching its tray. It chooses one
new RuntimeId. Multiple candidates fail without a click. A right click opens a
menu, but a business item is clicked only after its UIA process and native HWND
both match the verified task tray PID. Every business menu action is a native
mouse click. There is no IPC fallback for failed tray activation.

## Run after inspection and guest handover

```powershell
powershell.exe -NoProfile -STA -File .\run.ps1 -Action run -Phase lightweight `
  -BinDir C:\task\bin -ArtifactDir C:\task\native-01 -SourceSHA a99ccc0bc `
  -TaskRoot C:\un-task-01 -OpenOverflow
```

`TaskRoot` must not exist. The driver gives only the current user full access to
its newly created task and artifact directories. It never changes existing ACLs.
The default root is a new short directory in the current user's Temp. A socket
path of 104 UTF-8 bytes or longer fails before app launch; select a shorter new
root. Child processes get explicit profiles and cleared inherited `UNIC_*` and
`MYGO_*` settings. Private APPDATA/LOCALAPPDATA isolate legacy discovery.

Every product must embed the requested VCS revision. For binaries built with VCS
stamping disabled, pass `-SourceReceipt <file>` containing the canonical shape
`source_head` and `binary_sha256` entries for `unic`, `unicd`, and `unic-desktop`.
The script checks each executable digest against that receipt. Dirty-tree state
is recorded; the driver never silently treats it as a clean source. Product
source identity and new helper/script hashes remain separate.

## Phases and evidence

The default `-Phase lightweight` is the first native acceptance gate. Run it
before requesting the longer `-Phase full` matrix. Each phase needs a fresh task
root and artifact directory. A phase verdict applies only to the scenarios whose
`scenario_status` entries say PASS. The remaining entries stay NOT_RUN.

The lightweight phase uses one private profile and an independently started S0:

1. Save lightweight mode before launch. Manually launch Desktop with no arguments.
   Require a distinct GUI process, a visible Win32 GUI window and its actual
   foreground PID. Role IPC status alone cannot satisfy these assertions.
2. Send WM_CLOSE to the verified GUI HWND. Require GUI, tray and panel processes
   and role endpoints to exit. Require the same standalone S0 process/generation.
3. Manually launch with no arguments again. Require a new GUI OS identity and a
   visible foreground GUI. Repeat close and manual reopening. S0 must remain.
4. With every UI role absent, execute `unic-desktop --background`. Wait for that
   launcher to finish without binding a tray. Require all UI roles to remain
   absent and S0 to remain unchanged. Only this explicit background launch can
   omit the GUI in the initialized lightweight profile.
5. Stop the exact driver-created S0 generation through service IPC during driver
   cleanup. No native tray click or real hotkey is claimed by this short phase.

After this first gate passes, run a separate `-Phase full` command. The full
matrix uses three private profiles:

1. Start independent S0 in a normal profile. Manually open a visible foreground
   GUI, then send WM_CLOSE. Require the GUI process to exit while tray and S0
   remain. Do not create a panel through role IPC or a tray menu first.
2. Show the private WinForms target and verify its exact foreground HWND/PID and
   released modifiers. Inject Ctrl+Alt+V once while no GUI process exists.
   Require an independent panel process, a visible Win32 panel window and the
   panel's actual foreground PID. App-scoped Escape then tests restoration of
   the same target focus. Escape is labeled separately from native key input.
3. Use the actual native tray menu to reopen GUI. Save lightweight mode through
   production `unic config apply`. WM_CLOSE must now end all three UI roles and
   preserve S0. A manual no-argument lightweight restart must open a new visible
   GUI. Open a panel through the native tray menu and check four distinct PIDs.
   Close GUI, repeat manual reopening, and check explicit background core-only
   startup. Driver cleanup stops S0 only after every preservation assertion.
4. In another lightweight profile, manually open GUI without a preexisting
   service. Record desktop-owned S1. Open an actual visible foreground panel.
   GUI WM_CLOSE must end every UI role and turn the same S1 PID/generation into
   standalone. Manually reopen GUI against S1. Native menu Quit must preserve
   this already detached S1. Driver cleanup then stops its exact generation.
5. In a fresh third profile, open GUI and panel before any ownership detach.
   Native menu Quit must end every UI role and the newly tray-owned S2 service.

The earlier `native18` run is RED evidence: first panel activation reported
`role.visible=true`, but the owned Win32 panel HWND remained invisible. That
observation is distinct from the product's subsequent explicit Show/Focus/
Invalidate change. The change does not turn native18 into a pass. The full phase
must rerun the first-open native window assertions to establish new GREEN
Windows evidence. The lightweight phase does not execute or establish this fix.

The driver sets per-monitor DPI awareness for its own thread before UI queries,
mouse actions and capture. It restores the previous context and cursor position.
No system scaling setting changes. Screenshots use only a just-verified foreground owned
window/menu rectangle, never the full desktop. Capture uses a cropped screen read,
so a successful but black GPU PrintWindow response cannot masquerade as evidence. Every role records PID, Windows
process creation time, executable path/hash and authenticated role identity.
The receipt distinguishes native mouse, WM_CLOSE, SendInput, production CLI,
private role IPC and app-scoped automation. Input acknowledgement is not paste
insertion. System clipboard and cross-application paste remain NOT_RUN.

Artifacts include cropped PNGs, build identity, candidate observations, an action
timeline, `receipt.json` and `SHA256SUMS`. Successful cleanup removes private
profiles. A cleanup failure retains the task root and fails the verdict. It
never kills a PID to hide a failure or stops an unrelated process.

This driver has not been executed merely because its Go helper cross-compiles.
Windows PowerShell parsing, native UIA layout, menu ownership and input behavior
must be verified in the handed-over guest. Record the actual OS, architecture,
VM identity and pre/post VM state in the outer acceptance receipt.
