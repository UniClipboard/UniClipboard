# Desktop E2E contract

Status: implemented in the working tree; execution evidence is recorded in the [receipt](../exec/desktop-e2e.md).

## Entry and catalogue

`make desktop-e2e` is the canonical developer entry. It runs all registered cases and prints one HTML report path. `CASE=<id>` selects one case. `make desktop-e2e-list` lists cases. The runner owns environment switches, binary assembly, private profiles and evidence paths; callers do not compose individual test flags.

Cases live in `app/desktop/e2e/cases/*.txtar` and use the existing testscript dependency. Each case declares Case, Mode, Purpose, Steps, Expected and Forbidden. Mode is native, process or view. The generated CASES.md must match these declarations. Native scripts use semantic accessibility selection and app-scoped input through the pinned mygo automation plugin. All previous Desktop acceptance functions are scenarios in package-local `TestDesktopAcceptance` registries. The `acceptance` command dispatches them through one controlled interface. Preflight requires exact registry/catalogue correspondence. Original test entries, switches and standalone scripts are removed. See the [migration map](../../app/desktop/e2e/MIGRATION.md).

## Execution identity and isolation

Freeze tracked and nonignored source files before any build. Build and execute from that snapshot. Record source HEAD, dirty state, per-file SHA256, build tags, binary hashes, toolchain, OS and architecture. Recheck the snapshot before deleting it; later edits to the developer checkout cannot change the accepted run.

The desktop_e2e build tag enables mygo automation and a directory clipboard. It uses a distinct application name and requires an isolated profile. Release builds have neither hook nor automation listener. Use the real GUI composition, production service, Engine, SQLite and IPC adapter. Disable telemetry, sync, login registration intent and the global shortcut in test profiles. Never modify the user's profile or system clipboard.

## Evidence

Each case has an explicit PASS, FAIL or NOT_RUN result, an operation timeline, assertions and evidence links. Required cases cannot silently skip. Native cases must start a real native window, pass assertions, capture screenshots and produce a nonempty playable recording from continuously sampled window-content frames. The recording uses capture timestamps; it is not a slideshow assembled from selected checkpoint screenshots. Report frame count and duration. Missing required evidence fails that case.

View scenarios use the mygo software tester and retain final exercised PNG checkpoints for every created view. These images do not establish native-window behavior. They do not produce a continuous recording. The three legacy GUI scenarios remain identified as legacy-view coverage. Go JSON events must show the requested scenario passed with no skipped or failed subcases.

Capture via mygo Window.CapturePage records the running window's rendered content. It does not capture OS decorations, desktop compositing or other applications. App-scoped automation input does not establish physical key delivery. A directory clipboard assertion does not establish system clipboard delivery. Global hotkeys, OS focus policy and cross-application paste remain NOT_RUN unless a separate real-OS scenario explicitly exercises them.

One report groups results, assertions, screenshots and recording by case. Its manifest and SHA256SUMS bind retained evidence. A video is supporting evidence; assertions determine the tested behavior. Retain a failure screenshot/recording when capture remains available. Public output contains synthetic test content only. Private source, profiles, binaries and raw logs are deleted after owned processes exit, on success and failure. Cleanup failure fails acceptance. A cleanup error is appended to an existing scenario failure rather than replacing the original assertion error.

## Failure inventory specified before implementation

Missing ffmpeg/display; stale catalogue; unknown case; duplicate IDs; escaped source symlink; active real profile accidentally reused; production single-instance handover; real clipboard mutation; missing app/socket; ambiguous or absent accessibility target; failed semantic action; result timeout; no assertions; native case without a native window; empty or corrupt screenshots/video; frame capture failure; missing process receipt; successful Go command with skipped cases; source snapshot mutation; app/service residue; interrupted runner; stale artifacts reported as current; aggregate PASS despite one failing case.

## Visible-detail switching evidence

The native `history-detail-prefetch` case runs eight fully visible synthetic history rows through three forward/backward selection rounds, then repeats through an active search query. `ui-detail-cycle` waits only for the clicked row to report selection, then verifies the exact selected body within the detail region in that same accessibility snapshot and throughout each 300 ms dwell. It rejects offscreen targets, stale/missing content and loading/fetching labels. It retains timestamped `detail-observations.json` alongside the continuous video. On macOS this case records only the owned window through ScreenCaptureKit at a requested 30 fps using presentation timestamps and AVAssetWriter. Other platforms request a 50 ms window-content sampling interval. Actual frame timestamps define the video cadence. Initial population/query replacement are setup rather than switching assertions. Sampled observations do not prove the absence of an unobserved sub-frame transient.

## Production module boundary

The `production-boundary` process case checks the production command dependency graph, the shared renderer dependency graph and the i18n embedded catalogue. It builds `unic-desktop` and the separate `unic-desktop-demo`, checks demo help and rejects removed preview pages without opening a native window. `desktopui` is the package key for shared-renderer view and service-binding scenarios. Fixture builders are imported by preview startup and acceptance only.
