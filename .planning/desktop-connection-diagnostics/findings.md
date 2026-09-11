# Findings

## Requirements

- Engine capture is owned only by `uniclipd`.
- GUI and applicable CLI commands use the same authenticated daemon API.
- Repeated starts reuse one capture without extending it; stop uses `capture_id`.
- Daemon restart returns a new run in standard mode; clients discard stale capture display.
- Online export records Engine preparation and coverage separately from archive collection facts.
- Offline export includes existing files but clearly states that live Engine status and flush were unavailable.
- Window close must not stop capture; suspension/resume must not shut down observability.

## Research Findings

- Branch `fix/connection-diagnostic-export` is clean and already contains commit `2ad7f6976`, which installs the earlier Engine observability runtime in the daemon process.
- Desktop currently pins Engine commit `a3b95183`; the new APIs are in local Engine commit `9708c278`, which is not present in the checked remote refs.
- Existing authenticated routes `/diagnostics/debug` and `/diagnostics/log-export` already flow through daemon contract, webserver, daemon client, CLI, and generated frontend API.
- Existing offline startup-log export packages only GUI/daemon/CLI role logs and does not require daemon access.
- Current Engine host-source enum has `Application`, mobile extensions, and background service, but no explicit desktop-daemon variant. Desktop can only truthfully register the daemon as the process application unless Engine adds a desktop source.
- `init_tracing_subscriber()` currently drops the returned Engine process handle after checking health. The runtime remains installed globally, but the daemon API cannot control capture until the composition root retains a handle.
- `DaemonApiState` is the existing owner passed to every authenticated route, so it is the natural place to carry a clone of the process handle without exposing Engine internals to GUI or CLI.
- The current daemon online export is an Engine facade operation that writes a ZIP to a registered output handle. The offline startup exporter separately packages retained desktop role logs.
- The new Engine application exporter already includes strict `engine.YYYY-MM-DD.jsonl` files and a basic manifest, but its stable result does not carry the new preparation report into the ZIP.
- Desktop must therefore own final package assembly if the ZIP itself is to contain both actual file collection facts and the Engine preparation report. The existing `uc-observability::startup_logs` packager is the smallest existing owner to deepen for both online and offline export.
- `DaemonApiState::new` has one production construction site. Optional diagnostic runtime/archive fields with builder injection keep existing tests and non-daemon assembly explicit while production wires both.
- Existing frontend `DiagnosticsSettings` already owns debug mode and export. The detailed capture control fits there without adding a new settings category.
- The daemon has no existing cross-platform OS sleep callback. A bounded wall-clock gap observer can conservatively detect that the process did not run, record a resume boundary, and force Engine detailed capture back to standard without claiming a specific OS cause.
- A real isolated headless daemon run proved the API reports Engine source commit `b9b25fb2`, one stable run ID, a non-extending capture ID, distinct mismatch/stop results, and a new standard-mode run after restart.
- The real online ZIP contained strict Engine and daemon files plus a manifest with completed flush, actual included files, empty unreadable/truncated lists, `otherProcessesFlushed=false`, and `concurrentWritesPossible=true`.
- The local daemon protocol uses a strict revision handshake, so adding capture endpoints requires a revision bump to prevent an older daemon from being mistaken for a compatible controller.
- The first real content inspection showed why archive-only validation was insufficient: connection establishment can succeed before the peer rejects continuation authentication.
- The complete diagnosable story needs three owners' records: connection facts identify the attempt and candidates, the transport records the authentication rejection on each observable side, and Application records the recovery trigger and final deferred result.
- Detailed capture preserves per-attempt start/result, candidate source, duration, and correlation. Standard capture retains the logical start/final result and failures while filtering successful attempt detail.
- Engine still reports discovery and path sources as partial. The current evidence does not prove that a newly discovered candidate was the exact candidate selected for a later successful connection.
- Engine now exposes its complete analytics and diagnostics contracts through `uc_engine::observability`; Desktop no longer needs a direct `uc-observability-contract` dependency or a second synchronized pin.

## Technical Decisions

| Decision | Rationale |
|---|---|
| Build the new daemon API beside existing diagnostics routes | It reuses authentication and keeps GUI/CLI thin. |
| Treat Engine preparation report and ZIP collection manifest as separate fields | A completed flush does not prove every file or process was collected. |
| Develop against the local Engine worktree until a remote commit is authorized | The repository forbids a permanent local path or branch dependency. |
| Generalize the desktop archive packager instead of extending Engine business export | The product owns ZIP collection facts; Engine owns only its files, flush, and coverage report. |
| Expose status/start/stop plus one export action | Export preparation is internal to the daemon export action and its report is returned and embedded, avoiding a second caller-controlled preparation sequence. |
| Use one daemon-owned suspension observer with cancellation and join | It covers GUI-absent lightweight mode and avoids platform-specific duplicate lifecycle owners. |
| Treat diagnostic content as a separate acceptance gate | A valid ZIP and successful flush do not prove that a failure can be explained from its records. |

## Issues Encountered

| Issue | Resolution |
|---|---|
| Engine desktop source naming does not match plan wording | Verify whether `Application` is an accepted desktop mapping or add the missing Engine contract before final pin. |
| Required Engine revision is not remotely reachable | Complete reviewable desktop work first, then request only the external action needed for an immutable pin. |

## Resources

- Engine plan: `docs/exec-plans/active/041-exportable-connection-diagnostics.md` in the Engine repository
- Desktop runtime: `apps/daemon/src/daemon/`
- Daemon diagnostics transport: `crates/uc-daemon-contract`, `crates/uc-webserver`, `crates/uc-daemon-client`
- Offline archive: `crates/uc-observability/src/startup_logs.rs`
