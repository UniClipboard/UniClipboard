# Desktop E2E consolidation and visible evidence

Baseline: `8453b421e0`, branch `v2`, clean at task start.
Contract: [Desktop E2E](../contracts/desktop-e2e-contract.md).
Guide and catalogue: [README](../../app/desktop/e2e/README.md), [CASES](../../app/desktop/e2e/CASES.md).

## Implemented result

One command, `make desktop-e2e`, selects the registered cases, freezes source, builds the native test host and production service, executes testscript cases and prints one HTML report. `CASE=<id>` selects a case; list and doctor use the same runner. The runner reports per-case progress and publishes an aggregate result only after the report and evidence checksums exist.

The initial five-case catalogue contained one real native-window scenario and four production-process suites: data binding, history search, quick-panel ordering and startup lifecycle. The native case submits two real history entries, opens the actual search control, checks filtered rows, selects/copies a result, verifies full text at the directory clipboard and clears the search. It uses the production mygo view/Engine/SQLite/IPC path, with only the automation and Board test-host seam enabled by `desktop_e2e`.

Evidence includes per-case assertions and timed actions, checkpoint PNGs, continuously sampled window-content MP4, frame capture times, original process receipts, source/binary identity and SHA256SUMS. The HTML report links the video and screenshots alongside each case. Private source, programs, profiles, logs and raw frames are removed after cleanup.

## Acceptance

Initial five-case canonical run: `.build/desktop-e2e/20261111T104441Z-c5af3863/index.html`.

- `make desktop-e2e` exited successfully with five PASS cases: data-binding, history-search-copy, quick-panel-contract, search-contract and startup-lifecycle.
- The native case retained five checkpoint PNGs and a 9.16-second H.264 recording from eight continuously sampled frames. Full video decoding passed; this is sampled running-window content evidence, not a high-frame-rate OS screen recording.
- Every report link resolves. SHA256SUMS covers every retained evidence file and every checksum matches.
- The immutable source snapshot passed post-run verification. At the initial acceptance, executable-source files (Go, Python, txtar, module files, Makefile and workflow) matched that snapshot. The full migration below supersedes that initial implementation.
- Every case reports successful cleanup, and the controller removed its private source, binaries, profiles, logs and intermediate frames.
- Unknown case selection and a missing ffmpeg recorder both failed preflight without opening a window.
- The generated catalogue check and scoped lint passed, including the desktop_e2e build tag. Windows/Linux amd64 test-host cross-builds passed. Untagged dependency inspection excludes the automation plugin; the tagged test host includes it.
- CI workflow YAML parsed successfully. Linux/Xvfb execution and artifact upload are configured but were not dispatched during this task.

Earlier execution history:  Initial native runs exercised the failure path and retained failure screenshots/recordings. They revealed the collapsed search control, its comboBox accessibility role and variable search-preview titles. The scenario follows the observed UI rather than changing the product to satisfy the test. A later native run passed with eight captured frames and a playable recording.

The first full run passed its five case receipts but failed report publication on nullable metadata lists. That run is recorded as FAIL. The runner now accepts nullable optional arrays and keeps the aggregate at FINALIZING until report/checksum publication completes.

## Boundaries

Captures use the running native window's rendered content, not OS desktop compositing or decorations. App-scoped automation does not prove physical input or global-shortcut delivery. The directory clipboard is isolated from the system clipboard. OS focus restoration and cross-application paste remain NOT_RUN. macOS local execution and checked-in Linux/Xvfb CI wiring are separate evidence layers; a workflow edit is not a CI execution. No CI dispatch, external upload or commit was performed.

The repository documentation checker passes with stale verified markers while source is uncommitted. Update those markers after committing the accepted implementation; do not fabricate a verified SHA for dirty source.

## Full migration

All 25 previous Desktop acceptance entries are now package-local scenario functions. The catalogue contains 26 cases: 21 software-view cases, four process cases and one native-window case. The [migration map](../../app/desktop/e2e/MIGRATION.md) identifies every original entry. The original test files, independent environment gates, standalone quick-panel script and startup Make target are removed.

Every original behavioral assertion and subcase matrix is retained. The unified runner selects `TestDesktopAcceptance/<case-id>` and verifies JSON events rather than wrapping old test names. Missing, failing or skipped subcases fail acceptance. View checkpoints include final exercised states and failure states. Legacy `gui.view` coverage is labeled explicitly and does not claim active-composition acceptance. UI Kit component tests remain separate component checks.

Full migration acceptance: `.build/desktop-e2e/20261111T120131Z-bd31a8c7/index.html`.

- `make desktop-e2e` passed all 26 cases: 21 software-view cases, four production-process cases and one native-window case. No registered scenario or subcase was skipped.
- The run retained 311 passing scenario/subcase event records and 350 decodable PNGs. These event records are not a count of individual assertion expressions.
- The native-window case retained five PNG checkpoints and a 10.8-second H.264 recording from nine timestamped window-content samples. Full video decoding passed. Software-view cases retain checkpoints and JSON measurements; they do not claim continuous native-window recordings.
- Every report link resolves, every retained checksum matches, and all four process receipts contain successful verdicts and nonempty check lists.
- Frozen-source verification passed. Current executable-source files match the accepted snapshot; this receipt and planning notes were updated afterwards. All case cleanup results and controller private cleanup passed. The private run directory no longer exists.
- The migration audit confirmed removal of all 22 original test files. Every original behavioral failure message remains in the migrated scenario code, with the artifact-variable name updated where required. Original test functions and independent opt-in gates are absent from test source. The old quick-panel shell entry and startup Make target are removed.
- Ordinary `go test -race` passed for the four affected Desktop test packages. Catalogue consistency and documentation links passed. The change-scoped lint check reports zero issues. Full scoped lint reports one pre-existing import-group formatting issue in unchanged `internal/desktopdemo/tag_manager.go`; the issue is present in HEAD. No unrelated formatting change was made.
- Untagged release dependency inspection excludes both the automation plugin and the evidence helper. The tagged native host includes automation and excludes the evidence helper.
- Documentation verification markers remain stale while source is uncommitted. The documentation checker reports five freshness warnings and no errors. No commit, CI dispatch or external upload was performed.

The earlier full-migration discovery run `20261111T115606Z-61774c21` was deliberately interrupted before final verification and remains FAIL. It is not acceptance evidence. The final run above uses the completed registry, catalogue, report labels and extended execution timeouts.
