# Desktop diagnostic settings

## Failure inventory (before implementation)

- A saved log level differs from the service's effective level or disappears after restart.
- A temporary local override changes remote logging, persists into config, or cannot be reset.
- The telemetry master switch conceals the requested per-signal values or claims that disabled upload is active.
- An unavailable service leaves stale status labeled current, or a late result overwrites newer state.
- Export uses an unrelated executable from PATH or the wrong profile.
- Export replaces a user file, includes config/credentials/history, or claims cancellation succeeded.
- The GUI blocks on IPC, archive I/O or a native file dialog.
- Missing native callbacks appear as successful export or directory opening.
- A config revision conflict silently overwrites an external edit.

## Planned process acceptance

The canonical `settings-diagnostics` case drives the production Desktop binding
against a production `unicd` process. It changes persisted logging, verifies
configured/effective status, changes and resets a bounded local override, checks
per-signal telemetry values with the master disabled, and verifies CLI archive
membership and refusal to overwrite. It restarts the daemon and rechecks the
persisted settings. No renderer, GUI, browser, native dialog or clipboard runs.

## Boundaries

Reuse existing config fields and local diagnostic endpoints. Native dialog and
folder actions are Desktop callbacks. Export runs the private sibling `unic`
command with the exact profile. No public endpoint or Engine API is added.
Native dialogs, folder opening and visual acceptance remain NOT_RUN.

## Implemented path

Settings > General now adds diagnostic groups using the existing live config
save path. Local and remote configured log levels remain independent. Per-signal
upload switches retain their choices when the telemetry master switch is off.
The view reads actual log-level and telemetry state from the existing diagnostic
endpoints, displays configured/effective levels and dropped-record counts, and
provides explicit refresh and 15-minute local/remote debug override/reset actions.
An unavailable status is shown as unavailable rather than retained as current.

The Desktop host binds native save/folder callbacks. It resolves `unicd` with the
existing installation resolver and selects `unic` in that same directory. It
never looks up an arbitrary CLI through PATH. Archive generation uses the existing
`unic dev logs export --output` implementation and selected profile. File actions
run on a separate worker, so a pending native dialog cannot block the shared
service/config queue. Completion returns through the existing view result queue.
No native action is invoked by process acceptance.

## Validation history

The first compile probe failed as expected before diagnostic methods/state were
implemented. Its small log is `.cache/settings-diagnostics/first-red.log`.

The first canonical process run failed at fixture setup: an Engine-created test
profile lacked the CLI's explicit migration initialization. The CLI correctly
refused the external config edit with exit 6. That report remains
`.build/desktop-e2e/20261122T072117Z-1fb9a719/index.html`, with cleanup PASS.
The scenario now initializes its private profile using the production CLI and
isolates v1 discovery from the personal profile. The retry limit was two runs
after this diagnosed correction.

The corrected process run passed at
`.build/desktop-e2e/20261122T072229Z-bdce5438/index.html`.
Its receipt records binary hashes, safe archive membership/checksum and the
asserted persistence, runtime override and external-edit conflict behaviors.
This run predates the independent file-action worker correction; final replay
is recorded below when complete. No public-contract change was required.

Final replay after the file-action worker correction passed at
`.build/desktop-e2e/20261122T072325Z-2a160e6c/index.html`.
The canonical manifest reports PASS, unchanged frozen source and private cleanup
PASS. Seven process check groups passed. The [small receipt](evidence/settings-diagnostics/receipt.json)
and [checksums](evidence/settings-diagnostics/SHA256SUMS) retain source/run identity,
program hashes, checks, initial failure cause and native limits. Later shared
integration changes require their own final integrated replay.

Reproduce from the repository root:

```sh
make desktop-e2e CASE=settings-diagnostics
```
