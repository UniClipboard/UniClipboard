# Desktop startup and process lifetime acceptance

## Scope

The shared configuration now stores `desktop.mode`, `desktop.launch_at_login`, and
`desktop.start_in_background`. Normal desktop startup replaces and owns the profile service.
Lightweight startup leaves only a standalone service. Background startup creates the normal window hidden.
See [startup architecture](../architecture/headless.md#desktop-startup-settings)
and the [local service contract](../contracts/local-service-contract.md#service-lifecycle).

## Repeatable process acceptance

```sh
make desktop-startup-e2e
```

The command builds race-instrumented production `unic`, `unicd`, and `unic-desktop`,
plus a nonvisual executable using the same production lifecycle package as the GUI.
It opens no GUI and changes no real login item.
The file-backed login fixture validates persisted intent, external disable, explicit retry and native failure handling.
It does not prove macOS, Linux or Windows native registration.

Artifacts are written to `.cache/desktop-startup-e2e/manifest.json` and adjacent process logs.
The manifest binds source SHA-256 values, executable SHA-256 values, platform, process identifiers,
checks, race instrumentation and explicit native acceptance gaps.
The source files must remain unchanged throughout the process checks.

Required checks:

- Replace an existing standalone service before normal ownership; Engine submissions work.
- Save all desktop settings through the production CLI and reject an invalid mode.
- Reject a stale service-generation shutdown request.
- Refuse lightweight reuse of a live normal-owned service.
- Normal owner termination stops its service and preserves another profile's generation.
- Lightweight production launch exits and repeated launch reuses the same service generation.
- Persisted history survives normal/lightweight replacement.
- Abrupt owner death closes the parent pipe; the owned service exits.
- Service termination ends the normal owner rather than adopting a new generation.
- Immediate startup after owner death waits for the prior service to retire.
- Invalid startup configuration and a missing sibling executable leave the existing service untouched.
- A socket/profile mismatch cannot stop either profile.
- File-backed login enable/disable, preserved external disable, retry, and failure recovery work across processes.
- Standalone shutdown works through generation-scoped IPC.

Unix acceptance also checks service process state after owner exit, beyond socket removal.
Windows process-exit and native behavior require a native host.

## Additional checks

- Build the native production desktop, CLI and service.
- Cross-build the production programs for Linux and Windows; cross-builds do not prove native execution.
- Run `go vet` and the repository-pinned lint tool for the changed packages.
- Run the existing production headless E2E to cover unchanged clipboard adapter and service behavior.
- Run `go run ./tools/docscheck` after updating architecture verification markers.

## Native manual acceptance

These steps remain user-owned and were not run by the agent.

1. Install an application bundle on macOS, or the native desktop package on Windows/Linux. Keep sibling `unicd` beside `unic-desktop`.
2. Use the default profile. Enable login startup in Settings. Confirm both the requested setting and actual system state.
3. Log out and log in. Confirm that normal mode starts one desktop and one owned service.
4. Enable background startup. Quit and manually reopen. Confirm no main window appears. Open it through the tray, then repeat through a second normal launch.
5. Close the main window. Confirm the desktop and service remain running. Choose Quit. Confirm both processes exit.
6. Force-kill the normal desktop. Confirm its service exits.
7. Select lightweight mode and quit. Launch again. Confirm only `unicd` remains. Log out/in and confirm the same mode is selected.
8. Run `unic-desktop --mode normal` to reopen Settings. Confirm the existing lightweight service is replaced.
9. Disable the login item in system settings. Reopen normally. Confirm the application reports the actual disabled state and does not reenable it.
10. Use the explicit retry in Settings. Confirm native permission errors remain visible and do not falsely report success.

macOS flat binary archives cannot register a login item because mygo requires an application bundle.
The native login API cannot persist `UNIC_HOME`/`UNIC_SOCK`; registration with these overrides is rejected.
A login-setting edit made without a normal desktop session waits for the next normal session.

## Verification record

Native build and Linux/Windows amd64 cross-builds passed on the development Mac.
The pinned golangci-lint v2.15.0 was run with Go 1.27.3; using the automatic Go 1.26 toolchain is rejected by this repository's version requirement.
Ordinary documentation checking passes with existing staleness warnings.
Both architecture marker update attempts were rejected because source paths remain uncommitted.
The existing markers were preserved, including paths with unrelated user changes.
The final process results and source identity are recorded in the manifests above.
