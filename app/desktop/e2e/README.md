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
```

[CASES.md](CASES.md) is generated from `cases/*.txtar`. Each case states its purpose, actions, expected result, forbidden result and evidence mode. Developers do not set individual `UNIC_*` switches or assemble receipt paths. All 25 prior Desktop acceptance entries are migrated. Original test files, switches and standalone entries are removed. See the [migration map](MIGRATION.md). UI Kit component checks remain outside the Desktop E2E catalogue.

## What the evidence proves

The native case starts the real mygo desktop composition and production service, with a test-only automation plugin and directory clipboard. It submits history through the service, interacts with the running window, checks search results and verifies the full copied text at the directory clipboard. It records continuously sampled window-content frames with actual capture timestamps. It does not assemble a movie from selected checkpoint screenshots.

Capture uses mygo `Window.CapturePage`. This captures the running window's rendered content, including the current view state. It excludes window-manager decorations and desktop compositing. Input is app-scoped automation rather than physical input. System clipboard, global shortcut delivery, OS focus restoration and cross-application paste remain explicitly NOT_RUN. Process cases retain JSON assertions. View cases retain software-rendered PNG checkpoints, including failing subcases. They do not open native windows or produce a continuous recording. The three `gui-*` cases cover the retained legacy GUI view; they do not establish coverage of the active desktop composition.

The `desktop_e2e` build tag adds automation, a unique application identity and an isolated directory clipboard. Release builds do not import the automation plugin and keep the normal system clipboard implementation. Tests disable telemetry, sync, global shortcut registration and login-startup intent.

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

Use the repository Go toolchain, Python 3.10+, ffmpeg and ffprobe. Native cases need a logged-in desktop session. On Linux, install mygo's GTK/WebKit runtime dependencies and use a real display or `xvfb-run`. CI supplies Xvfb; that proves native window operation in a virtual display, not physical desktop policy.

The runner opens only test-owned windows with synthetic content and a unique profile. It never reads or replaces the user's system clipboard. Do not run two native Desktop acceptance commands against the same physical desktop concurrently.

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
| `start` | Start an owned native desktop with real service and recording. |
| `submit <id> <text>` | Seed synthetic history through the production service. |
| `ui-wait <role> <label>` | Require a stable matching accessible element. |
| `ui-absent <role> <label>` | Require stable absence of matching elements. |
| `ui-click <role> <label>` | Click the unique matching element through app-scoped window input. |
| `ui-set <role> <label> <text>` | Set the unique matching input through its accessibility action. |
| `ui-key <accelerator>` | Send a key to the owned window, not the global desktop. |
| `board-equals <text>` | Require exact text at the test directory clipboard. |
| `screenshot <name>` | Save a validated window-content PNG checkpoint. |

The `acceptance <package> <case-id>` command dispatches a package-local scenario through `TestDesktopAcceptance`. Allowed package keys are `desktopui`, `quickpanel`, `gui` and `startup`. Scenario functions preserve private view access without exposing test APIs in the product. Register a function in that package's `acceptance_test.go` and a matching txtar case. Preflight rejects any registry/catalogue mismatch. The runner validates Go JSON events; skipped, missing or failing subcases fail acceptance. View cases must emit decodable screenshots.

Existing assertions were migrated into scenario functions, rather than duplicated behind a second test entry. Ordinary `go test` checks catalogue freshness and skips the explicit acceptance registries. Detailed measurements and subcase results appear in the unified report.

The [Desktop E2E contract](../../../docs/contracts/desktop-e2e-contract.md) defines evidence and isolation requirements.
