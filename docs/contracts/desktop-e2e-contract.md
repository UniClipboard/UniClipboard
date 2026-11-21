# Desktop E2E contract

Status: implemented in the working tree; execution evidence is recorded in the [receipt](../exec/desktop-e2e.md).

## Entry and catalogue

`make desktop-e2e` is the canonical developer entry. It runs all registered cases and prints one HTML report path. `CASE=<id>` selects one case. `make desktop-e2e-list` lists cases. The runner owns environment switches, binary assembly, private profiles and evidence paths; callers do not compose individual test flags.

`SHARD=<index>/<count>` partitions the sorted complete catalogue by round-robin position, using one-based indices. It is mutually exclusive with `CASE`. Invalid indices, counts above the catalogue size and empty shards fail before building. `python3 app/desktop/e2e/run.py list --shard 1/3` previews the exact selection. PR CI runs all three shards on separate hosted desktops with fail-fast disabled and distinct evidence artifacts. Each shard keeps the same assertions, source identity, race instrumentation, cleanup and thirty-minute job limit. Cases assigned to other shards remain NOT_RUN in that shard's report; full acceptance requires all three shards to pass.

The Ubuntu 24.04 PR jobs bound dependency installation to five minutes. Regional
EC2 Ubuntu archive entries and the runner's existing file-backed mirror list use
the official HTTPS archive. APT retains package
signature verification, three retries and thirty-second connection/data timeouts.
This bootstrap policy preserves the full Desktop catalogue and job budget.

Cases live in `app/desktop/e2e/cases/*.txtar` and use the existing testscript dependency. Each case declares Case, Mode, Purpose, Steps, Expected and Forbidden. Mode is native, process or view. The generated CASES.md must match these declarations. Native scripts use semantic accessibility selection and app-scoped input through the pinned mygo automation plugin. All previous Desktop acceptance functions are scenarios in package-local `TestDesktopAcceptance` registries. The `acceptance` command dispatches them through one controlled interface. Preflight requires exact registry/catalogue correspondence. Original test entries, switches and standalone scripts are removed. See the [migration map](../../app/desktop/e2e/MIGRATION.md).

## Execution identity and isolation

Freeze tracked and nonignored source files before any build. Build and execute from that snapshot. Record source HEAD, dirty state, per-file SHA256, build tags, binary hashes, toolchain, OS and architecture. Recheck the snapshot before deleting it; later edits to the developer checkout cannot change the accepted run.

The desktop_e2e build tag enables mygo automation and a directory clipboard. It uses a distinct application name and requires an isolated profile. Release builds have neither hook nor automation listener. Native setup source discovery and credential reads use a build-tagged adapter restricted to the owned fixture; release builds retain the normal platform adapters. Use the real GUI composition, production service, Engine, SQLite and IPC adapter. Disable telemetry, sync, login registration intent and the global shortcut in test profiles. Never modify the user's profile or system clipboard.

An explicit `Environment: disposable-guest` case is the exception for a task-owned guest system clipboard. It requires an explicit VM UUID and an owned physical-input control directory. The ordinary catalogue and CI shards exclude that environment and report it as NOT_RUN. Guest preflight verifies virtualization, a real Wayland session and an absent SPICE clipboard user agent. It refuses locked sessions. Native cases may request standard temporary idle inhibition while unlocked. Cleanup releases the D-Bus connection. Tests never change lock settings or unlock a session.

An `Environment: owned-window-guest` case requires an explicit VM UUID and a Windows guest. It remains excluded from ordinary runs and CI shards. The caller must confirm exclusive handover before execution. This case keeps the directory clipboard. Physical pointer operations verify the exact foreground PID and executable before acting. This authorization does not enable system clipboard acceptance.

Guest cases retain the production board, initialize a task profile through the production CLI and isolate legacy-source discovery under a private XDG data directory. Physical UTM input requests carry unique identities. The controller never retries an unknown outcome. Verify the current owned window or exact receiving process before input. Exact target insertion requires independent accessible-text and saved-document assertions. Retain synthetic target bytes and application identity in the unified evidence. Clipboard, input delivery and target insertion remain separate verdicts. Host clipboard access stays forbidden.

An exact saved-document fixture may include a receiver's documented default trailing newline. This never relaxes the inserted accessible-text equality. An OS screenshot of the receiving application must identify its capture method separately from mygo window-content frames. MIME alternatives supplied by the desktop must match independent OS reads. A headless sender plus one graphical clipboard is not two graphical endpoints. Closing the GUI must terminate its process while tray capture remains available. Reopening creates a new GUI process and retains the service generation. These are independent assertions; reopening cannot override failed capture while the GUI is absent.

## Evidence

Each case has an explicit PASS, FAIL or NOT_RUN result, an operation timeline, assertions and evidence links. Required cases cannot silently skip. Native cases must start a real native window, pass assertions, capture screenshots and produce a nonempty playable recording from continuously sampled window-content frames. The recording uses capture timestamps; it is not a slideshow assembled from selected checkpoint screenshots. Pause capture while the GUI process is absent. Resume sampling the new process after discovering its window; retain actual elapsed time rather than inventing frames for the absence. Main and panel automation endpoints are separate, and window IDs are valid only within their owning process. Report frame count and duration. A resized window must retain all later frames on a fixed canvas with original-scale, centered padding. Recording and checkpoint captures use one capture lock. Owned Windows minimize verification holds that lock across the caption click, OS IsIconic assertion and restore, because minimized content has no drawable size. Other semantic commands do not wait for that lock. Allow up to 30 seconds for each native raster capture; keep the eight-second RPC deadline for semantic commands. Recording shutdown waits at most 40 seconds for an in-flight capture to finish. Native reports record capture durations and the actual FFmpeg/FFprobe versions and executable hashes. Missing required evidence fails that case.

View scenarios use the mygo software tester and retain final exercised PNG checkpoints for every created view. These images do not establish native-window behavior. They do not produce a continuous recording. The three legacy GUI scenarios remain identified as legacy-view coverage. Go JSON events must show the requested scenario passed with no skipped or failed subcases.

Capture via mygo Window.CapturePage records the running window's rendered content. It does not capture OS decorations, desktop compositing or other applications. App-scoped automation input does not establish physical key delivery. A directory clipboard assertion does not establish system clipboard delivery. Global hotkeys, OS focus policy and cross-application paste remain NOT_RUN unless a separate real-OS scenario explicitly exercises them.

One report groups results, assertions, screenshots and recording by case. Its manifest and SHA256SUMS bind retained evidence. A video is supporting evidence; assertions determine the tested behavior. Retain a failure screenshot/recording when capture remains available. Public output contains synthetic test content only. Private source, profiles, binaries and raw logs are deleted after owned processes exit, on success and failure. Cleanup failure fails acceptance. A cleanup error is appended to an existing scenario failure rather than replacing the original assertion error.

## Failure inventory specified before implementation

Missing ffmpeg/display; stale catalogue; unknown case; duplicate IDs; escaped source symlink; active real profile accidentally reused; production single-instance handover; real clipboard mutation; missing app/socket; ambiguous or absent accessibility target; failed semantic action; result timeout; no assertions; native case without a native window; empty or corrupt screenshots/video; frame capture failure; missing process receipt; successful Go command with skipped cases; source snapshot mutation; app/service residue; interrupted runner; stale artifacts reported as current; aggregate PASS despite one failing case.

## Production module boundary

The `production-boundary` process case checks the production command dependency graph, the shared renderer dependency graph and the i18n embedded catalogue. It builds `unic-desktop` and the separate `unic-desktop-demo`, checks demo help and rejects removed preview pages without opening a native window. `desktopui` is the package key for shared-renderer view and service-binding scenarios. Fixture builders are imported by preview startup and acceptance only.

## Hosted native-window replay

The reusable native Desktop workflow runs `history-search-copy` on all six declared OS/architecture targets. Linux uses GTK under Xvfb and a private D-Bus session. Windows/macOS use the hosted native desktop. The same canonical runner freezes source, builds isolated programs, drives semantic native actions, records continuous content frames and verifies cleanup. The report's scope remains application-native directory-clipboard interaction; it does not prove OS focus or system clipboard delivery.

The pinned Go 1.27 toolchain has no Windows arm64 race detector. For that target, the native runner omits `-race` from the selected binary and driver builds and records `race_detector=false`. It still requires every selected scenario and evidence assertion to pass. Other declared targets retain race instrumentation. This target-specific toolchain limitation is not a skipped scenario or a claim of full cross-platform catalogue acceptance.
