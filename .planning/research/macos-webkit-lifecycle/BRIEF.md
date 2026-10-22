# Brief: macOS WebKit process lifetime

Date: 2026-10-22
Status: User accepted the process split; implemented, native acceptance pending

## Goal

Close the main window, release its WebKit processes, retain the tray and daemon, and recreate a
working main window on the next explicit open. Preserve startup preferences. Do not start a GUI,
change production state, or terminate a process during this investigation.

## Findings

1. Wails beta.28 Close sends WindowClosing asynchronously. Its default listener marks the Go window
   destroyed, closes the native window and removes the window registry entry. This does not offer a
   contract that every WebKit auxiliary process terminates.
2. Wails creates WKWebViewConfiguration without choosing a website data store. WebKit's default
   store can outlive the last view. Current upstream WebKit explicitly holds that store in a global
   strong reference. This is source evidence for a lifetime mechanism, not a measurement of this
   machine's exact WebKit implementation.
3. The selected local macOS SDK exposes no public WKWebView operation to terminate all WebContent,
   GPU and Networking processes. A process pool is not an escape route: Apple's SDK states that
   creating and using multiple WKProcessPool instances no longer has any effect since macOS 12.
4. The data-store removal API deletes persistent data. It is not an idle process shutdown API and
   must not be used for window close. Switching to a nonpersistent store would change persistence
   semantics and does not provide a documented all-process-exit guarantee.
5. WebKit has private/test termination selectors. They are not the same compatibility boundary as
   Wails' existing background-material API. They do not provide a supported all-process contract.
6. UniClipboard has other WebViews: the updater and the fallback Quick Panel. Its default native
   Quick Panel uses the existing separate Rust helper and does not require WebKit.
7. Existing local observations showed Networking surviving the absence of UniClipboard WebContent.
   A later observation showed new GPU/WebContent processes. No close/reopen interaction or exact
   trigger was recorded, so those observations do not prove a leak or a complete regression cycle.

## Recommendation

Use an application-owned UI child process to own every macOS WebView. Keep the tray and process
coordination in a resident host. Close the UI child when the application enters windowless mode.
Create one new UI child on the next explicit open. Verify the actual WebKit process exit after the
old child exits; do not report success from a window-registry entry or RSS change alone.

The user accepted this process-architecture change. The resident/UI roles and private-pipe
control are implemented. Native acceptance remains pending. The nested UI app has an independent
identity so LaunchServices routes outer-app reopen requests to the resident host.

## Ownership

| Resource or action | Resident host | UI child | Existing daemon |
|---|---|---|---|
| LaunchServices reopen and app single-instance lock | Own | Forward requests | None |
| Tray and native Quick Panel supervisor | Own | None | None |
| Daemon spawn, recovery, restart and full quit | Own once | Request via private IPC | Own profile lock |
| Background update scheduling and downloads | Own | Render progress and prompts | Unchanged |
| WKWebView, native WebKit store and page sessions | None | Own | None |
| User content, encryption and product settings | Client | Client | Own |

The UI child reads the existing daemon connection through current APIs. It must not run daemon
bootstrap, recovery, login-item reconciliation, update scheduling or tray creation. Child shutdown
must not stop the daemon. Cmd-Q in the child must request full application quit through the host.
The host must not create a hidden WebView for any background function.

Use a private inherited pipe channel for typed lifecycle/navigation messages. Do not add an HTTP
listener, persist a new token, place credentials in command arguments, or use clipboard content as
IPC. Reuse the existing quickpanelhelper supervision conventions; it already owns child handles,
private pipes, cancellation and restart behaviour. Do not turn its native-panel protocol into an
unrelated generic interface merely to share a few process-launch lines.

## Rejected shortcuts

- Hide: retains the view and its document.
- Native Close alone: retains no all-WebKit-process-exit guarantee.
- Kill processes by executable name, display name or cache path: unsafe ownership and PID reuse.
- Private/test WebKit termination selectors: unsupported lifecycle contract.
- Delete a website data store: destroys persistent data.
- Restart the entire tray host: changes tray residency and the existing application lifecycle.

## Implemented scope and remaining acceptance

1. Add a macOS UI-process owner near the Go desktop host. Serialize start/open/close/reap, hold the
   exact child process handle, and coalesce opens so no duplicate UI child can start.
2. Split host bootstrap from UI attachment in main.go. The UI role attaches to the existing daemon
   and creates only the requested UI. Keep one authoritative app-scope single-instance route.
3. Route showMainWindow, Settings, updater UI and full-quit/restart actions to the correct owner.
   Keep the existing frontend host-command API; place process-boundary adaptation in Go.
4. Move fallback Quick Panel creation out of the resident host. A later explicit Quick Panel
   request can start the UI child again. Resolve the policy for an already-open updater/fallback
   panel before declaring that closing main always exits every WebKit process.
5. Update macOS packaging and signing to include an identifiable UI executable/role. Preserve
   existing app identity, acceptance isolation, autostart and updater contracts.
6. Add a real lifecycle E2E before the production change. Exercise window close, lightweight,
   Finder reopen, tray Open, Settings, rapid repeated opens, UI crash, app Quit and update restart.

## Acceptance

- Stable resident-host PID, daemon PID and native Quick Panel PID across three round trips.
- One UI child while open; no UI child while windowless; a new UI-child start identity on reopen.
- No app-owned WebContent, GPU or Networking process after a bounded native cleanup interval.
- The process count assertion must distinguish other applications and profiles. Do not kill
  uncertain/shared processes to force a passing result.
- Reopened pages attach to the same daemon, reconstruct a fresh document and support normal
  operations. Verify synthetic sync while the UI child is absent on isolated paired test profiles.
- Saved startupMode remains unchanged. Full Quit stops the app-owned processes exactly once.
- Store source identity, binary hashes, owned process identities and the complete assertion JSON.

The new build and real inherited-pipe probe passed. No native E2E has been run. The earlier
Close-only artifacts do not validate the implemented process split.

## Private-channel failure modes before isolated verification

- A log line corrupts the protocol stream. Use inherited descriptors, not stdout.
- The child reads stdin instead of the assigned pipe descriptors.
- Startup readiness races the first window request.
- A synchronous reverse request blocks the only reply reader.
- Concurrent replies reach the wrong request or overwrite a reply channel.
- A timed-out request poisons the next request.
- A structured command error becomes a plain string, or a text error becomes an object.
- Parent EOF leaves an orphan child, or child exit blocks a pending call.
- A blocked write prevents shutdown. Closing the owned pipe must unblock it.
- A direct UI-role invocation reaches App.New without inherited control pipes.

The E2E-only channel probe must execute the real channel over inherited pipes between two
processes of the built executable. It must stop before the GUI entry point and must never
construct a Wails application, connect to a daemon or read a profile. This verifies transport
only; it does not substitute for native WebKit process-exit acceptance.

## Cross-platform extension

The user then requested one architecture for macOS, Windows and Linux. The shared UI-process
owner now runs on all three systems. Windows and Linux use their resident executable's internal
UI role. macOS keeps the separately identified nested UI app required by LaunchServices.
Unix platforms inherit descriptors; Windows uses an explicit inherited-handle list. The child
clears inheritance. Linux scopes GTK application IDs by profile and role. Both Windows and Linux
keep their event loop alive without windows. Existing portable roots and WebView storage paths
remain unchanged on Windows and Linux. The unified contract is documented in
`docs/architecture/gui-go-ui-process.md`.

The Windows/Linux external E2E is `apps/gui-go/e2e/ui_process_portable_run.py`. It requires an
explicitly authorized dedicated graphical session. No native platform acceptance was executed.
Linux native compilation is still unverified because the local host lacks a Linux GTK/WebKitGTK
build environment; the Docker service is unavailable. Cross-compilation and transport results
must not be presented as proof that browser subprocesses exit.
