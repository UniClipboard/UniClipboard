# Desktop end-to-end acceptance

## One entry

```sh
make desktop-e2e
```

Run from the repository root. The command executes the registered native-window, production-process and software-view cases. It prints one `index.html` path under `.build/desktop-e2e/<run-id>/`. Open that report to inspect every case, assertion, screenshot and recording. A selected case failing or not executing makes the command fail.

```sh
make desktop-e2e-list
make desktop-e2e-doctor
make desktop-e2e CASE=history-search-copy
make desktop-e2e SHARD=1/3
```

`SHARD=index/count` selects one disjoint round-robin partition of the sorted catalogue. Run every shard for complete coverage. Do not combine `CASE` and `SHARD`. PR CI runs three shards on separate hosted runners, preserves the thirty-minute limit for each job and uploads distinct reports. Preview a selection with `python3 app/desktop/e2e/run.py list --shard 1/3`.

[CASES.md](CASES.md) is generated from `cases/*.txtar`. Each case states its purpose, actions, expected result, forbidden result and evidence mode. Developers do not set individual `UNIC_*` switches or assemble receipt paths. All 25 prior Desktop acceptance entries are migrated. Original test files, switches and standalone entries are removed. See the [migration map](MIGRATION.md). UI Kit component checks remain outside the Desktop E2E catalogue.

## What the evidence proves

The native case starts the real mygo desktop composition and production service, with a test-only automation plugin and directory clipboard. It submits history through the service, interacts with the running window, checks search results and verifies the full copied text at the directory clipboard. It records continuously sampled window-content frames with actual capture timestamps. It does not assemble a movie from selected checkpoint screenshots. When the window changes size, the recorder centers original-scale frames on a fixed canvas sized to the largest captured window; it does not discard the later history frames.

Capture uses mygo `Window.CapturePage`. This captures the running window's rendered content, including the current view state. It excludes window-manager decorations and desktop compositing. Input is app-scoped automation rather than physical input. System clipboard, global shortcut delivery, OS focus restoration and cross-application paste remain explicitly NOT_RUN. Process cases retain JSON assertions. View cases retain software-rendered PNG checkpoints, including failing subcases. They do not open native windows or produce a continuous recording. The three `gui-*` cases cover the retained legacy GUI view; they do not establish coverage of the active desktop composition.

The `desktop_e2e` build tag adds automation, a unique application identity and an isolated directory clipboard. Release builds do not import the automation plugin and keep the normal system clipboard implementation. Native setup cases additionally use a build-tagged source adapter that discovers only the runner-owned v1 fixture and returns unavailable system credentials. The real passphrase fallback and importer remain active; no personal credential store is accessed. Tests disable telemetry, sync, global shortcut registration and login-startup intent.

## Explicit disposable-guest acceptance

Cases declaring `Environment: disposable-guest` are opt-in. Ordinary runs and CI shards select the isolated catalogue; the report retains guest-only cases as NOT_RUN. `list` and the generated catalogue include both environments. An explicit guest case without `GUEST=<VM UUID>` fails preflight. Guest preflight requires Linux Wayland, a recognized VM and no running user SPICE clipboard agent. A locked native session fails before startup. Native cases request standard session-bus idle inhibition where available. Guest cases require that request to succeed. Cleanup releases the connection. Tests never unlock a session or change lock settings.

The `system-clipboard` case uses the actual production board. It explicitly initializes its private profile through `unic migrate --initialize-empty` with a private XDG discovery directory. It verifies startup baseline exclusion, real Chinese text capture, exact history application and no echo. The separate `system-clipboard-target` case opens a standalone GNOME Text Editor with private XDG configuration, pastes through physical guest input, reads the exact inserted text through AT-SPI and checks saved document bytes against an exact fixture including GtkSourceView's default trailing newline. KDE Spectacle captures the active guest target window, and AT-SPI rechecks its focus. `target-content.txt` and `target-receipt.json` bind the receiving application's content. Key delivery alone cannot pass the target assertion.

`system-clipboard-formats` checks external HTML/PNG/file offers, exact stored and restored payloads, physical Enter and no echo. KDE's additional text alternative must match an external read. `system-clipboard-receive` uses two approved production services but only one native clipboard. `system-wayland-panel` checks automatic-paste refusal, Copy and Escape. `system-window-lifecycle` requires capture while hidden; `system-second-launch` independently checks reopening without replacing the service. Passing the latter cannot override a hidden-capture failure. `system-managed-cleanup` checks a real received-file URI, excluded tags, native clipboard holders, reclamation and preserved user-owned source files. `system-offline-backlog` replaces a normal owner with a standalone service, receives offline history, then reopens Desktop and tests a fresh delivery. It refreshes restarted peer endpoints with real tickets because test profiles disable rendezvous. `system-linux-ui-light` and `system-linux-ui-dark` retain the actual Linux skin in a narrow native window; semantic Chinese field values do not prove IME input.

For a QEMU VM registered in UTM, run the host-side input adapter after exporting the reviewed source and installing the repository Go toolchain and existing dependencies in an owned guest workspace:

```sh
python3 app/desktop/e2e/utm.py --host <SSH-alias> --guest <UTM-UUID> \
  --workspace /var/tmp/<owned-task> --out .build/<new-control-receipt>
```

The workspace contains `source/`, `go/bin/`, `go-cache/`, `go-mod/` and `gopath/`. The adapter uses the current guest desktop environment and invokes `make desktop-e2e`; it does not replace the canonical runner. Each physical input request has a unique identity. The adapter sends only supported UTM guest scan codes and acknowledges each request once. An unknown result is never replayed. Enter requires the owned Desktop window to have OS focus. Paste and Save require the exact standalone editor process to own focus. Keep guest/host clipboard sharing disabled until owned clipboard providers exit. Restore previous sharing and VM states after the run.

System clipboard and target insertion verdicts are reported separately. A passing format roundtrip does not prove receiver rendering/insertion. A headless peer does not prove two-desktop synchronization. Global portal approval, supported-platform target races, IME and OS decorations require their own native evidence.

## Run structure

```text
<run-id>/
  index.html                 one report for the run
  manifest.json              aggregate verdict, source/binary identity and cleanup
  CASES.md                   catalogue from the frozen source
  SHA256SUMS                 checksums of retained evidence
  cases/<case-id>/
    result.json              assertions, steps, times, verdict and cleanup
    01-started.png           native-window checkpoints, when applicable
    recording.mp4            continuous native-window-content recording
    recording-frames.json    actual frame capture times; temporary raw frames are deleted
    scenario-receipt.json    detailed scenario receipt, when applicable
    view-*.png               software-view checkpoints, for view cases
```

Source, binaries, profiles, raw logs and intermediate frames live in a private temporary directory. The runner freezes source before building and verifies that snapshot again before deleting it. Edits to the developer checkout do not change a running acceptance. The runner retains failure screenshots/recording when the window remains capturable. Cleanup failure fails acceptance. Existing output directories are rejected, so an older result cannot masquerade as a new run.

## Requirements

Use the repository Go toolchain, Python 3.10+, ffmpeg and ffprobe. Native cases need a logged-in desktop session. On Linux, install mygo's native GTK and tray indicator runtime dependencies and use a real display or `xvfb-run`. CI supplies Xvfb; that proves native window operation in a virtual display, not physical desktop policy.

The ordinary runner opens only test-owned windows with synthetic content and a unique profile. It never reads or replaces the user's system clipboard. Explicit guest cases use the task-owned guest clipboard under the requirements above. Do not run two native Desktop acceptance commands against the same physical desktop concurrently.

Recording requires an available H.264 encoder. The recorder uses `libx264`, or `libopenh264` when the distribution supplies it. Each native result records the selected encoder. Recording failure is appended to the original scenario failure.

## Add a case

1. Write the failure inventory and expected user behavior before changing production code.
2. Add `cases/<lowercase-id>.txtar` with all six headers: Case, Mode, Purpose, Steps, Expected and Forbidden. Use English documentation.
3. Use `Mode: native` for real-window UI cases. `start` creates the private desktop/service and starts continuous recording. Use `Mode: process` for production-process scenarios and `Mode: view` for software-rendered view scenarios.
4. Add semantic UI actions and observable assertions. Match roles and labels from the native accessibility tree. Labels are exact by default; prefix a label with `~` for a Go regular expression when a preview is intentionally variable. Ambiguous action targets fail.
5. Add named `screenshot` checkpoints. A native case without screenshots, a playable recording, a window or assertions cannot pass.
6. Regenerate the catalogue and execute the case:

   ```sh
   python3 app/desktop/e2e/run.py catalog --update
   make desktop-e2e CASE=<lowercase-id>
   ```

7. Inspect the report and run `make desktop-e2e` before considering the change accepted. The ordinary Go test command checks catalogue freshness, but skips the explicit acceptance run.

### Native commands

| Command | Meaning |
|---|---|
| `start` | Start an owned native desktop with completed setup, real service and recording. |
| `start-setup complete\|partial` | Start a fresh native setup with an isolated encrypted v1 source; recording starts before service startup. |
| `observe <seconds>` | Keep recording the current live state for 1–5 seconds for human review. |
| `service-absent` | Assert that setup has not started a service. |
| `history-count <expected>` | Assert the exact migrated-history count through production IPC. |
| `submit <id> <text>` | Seed synthetic history through the production service. |
| `ui-wait <role> <label> [seconds]` | Require a stable matching accessible element; optional timeout is 1–120 seconds (default 20). |
| `ui-idle` | Require the native window to settle; future timers are checked separately by `event-wakeup`. |
| `ui-absent <role> <label>` | Require stable absence of matching elements. |
| `ui-click <role> <label>` | Click the unique matching element through app-scoped window input. |
| `ui-enabled <role> <label>` | Wait until the matching live control is enabled before acting. |
| `ui-set <role> <label> <text>` | Set the unique matching input through its accessibility action. |
| `ui-key <accelerator>` | Send a key to the owned window, not the global desktop. |
| `board-equals <text>` | Require exact text at the test directory clipboard. |
| `screenshot <name>` | Save a validated window-content PNG checkpoint. |

Built-in plugin cases also use read-only `config-equals <path> <json-value>`, `restart-required <key>` and `restart-clear` assertions. `plugin-probe <kind> <disposition-or-error>` submits synthetic content to the real Engine. `connect-policy-peer` starts a second owned production service and pairs it with exact fingerprint/revision approvals; `stop-policy-peer` stops that service. `plugin-auto-probe <file|directory> <auto_file_too_large|queued>` observes content through the private directory clipboard and checks receiver-side absence or matching payload SHA-256 values. A blocked copy requires fresh local history, 1.5 seconds without receiver changes and a subsequently delivered text barrier. Its directory contains two 600 KiB files to distinguish a total-byte threshold from a per-file threshold. `processor-probe <enabled|disabled>` checks new history tags and available text Actions. `restart-desktop` quits the owned Desktop through its actual Quit shortcut, waits for its service to stop, and reopens the same profile without rewriting configuration. Recording pauses across the quit/reopen gap; all sampled window frames remain in the continuous recording with their capture times.

The `acceptance <package> <case-id>` command dispatches a package-local scenario through `TestDesktopAcceptance`. Allowed package keys are `desktopui`, `quickpanel`, `gui` and `startup`. Scenario functions preserve private view access without exposing test APIs in the product. Register a function in that package's `acceptance_test.go` and a matching txtar case. Preflight rejects any registry/catalogue mismatch. The runner validates Go JSON events; skipped, missing or failing subcases fail acceptance. View cases must emit decodable screenshots.

Existing assertions were migrated into scenario functions, rather than duplicated behind a second test entry. Ordinary `go test` checks catalogue freshness and skips the explicit acceptance registries. Detailed measurements and subcase results appear in the unified report.

The [Desktop E2E contract](../../../docs/contracts/desktop-e2e-contract.md) defines evidence and isolation requirements.

### Tray style parity

`make desktop-e2e CASE=tray-icon-settings` drives the real settings composition through the software tester, production unicd/CLI processes and the production configuration watcher. It observes generated PNGs at the tray image API boundary and verifies both styles, persistence, immediate delivery, invalid-value rejection, duplicate suppression, repaint and failed-delivery retry. It opens no native window. Its receipt does not establish OS-tray visibility, native menu preservation or actual theme notification delivery.
