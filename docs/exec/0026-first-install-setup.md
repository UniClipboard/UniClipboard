# 0026: Desktop first-install setup implementation

- Date: 2026-11-13
- Decision: [0026](../decisions/proposed/0026-first-install-setup.md).
- Prototype: [setup](../../prototype/setup.html), using the established [GUI](../../prototype/gui.html) style.
- Status: Implemented. Process acceptance, platform builds, software-view acceptance and macOS native complete/partial recordings passed. Real system credential prompts and other native platforms remain NOT_RUN.

## Implementation contract

Use a Desktop-owned setup flow, a UI Kit renderer, and the existing offline v1 importer. Keep setup ownership separate from normal service ownership. Persist a pending marker before migration can create a target database. Treat a pre-existing database without this marker as an established v2 installation. An empty history does not imply a fresh installation.

Resolve setup before starting the service, clipboard adapter, tray actions, notifications, and global shortcuts. Preserve the existing post-setup runtime. Later migration must stop and wait for only the Desktop-owned runtime before taking exclusive target ownership.

Use the native directory picker. Read credentials only after explicit consent. Show safe errors, masked passphrase input, phase progress, cancellation, and separate complete/partial/cancelled/failed results. Resume through the existing import mappings. Preserve the user's source and prototype files.

## Failure inventory before implementation

- Setup completes merely because a welcome window closed.
- An established empty v2 installation is mistaken for a new installation.
- Migration initializes the database, then restart incorrectly bypasses unfinished setup.
- Malformed setup state silently loses an unfinished migration.
- Detection triggers a credential prompt or confuses access denial with an absent source.
- Invalid directories enable import; named/custom sources cannot be selected.
- Target ownership conflicts prompt for secrets or terminate an unrelated service.
- Clipboard capture, sync, quick-panel shortcuts, or notification workers start during migration.
- Wrong credentials prevent retry or skip; secrets appear in process arguments or reports.
- Import completion is presented before verification; a partial report is labeled complete.
- Missing content parts are counted as failed whole records.
- Cancellation discards committed records, releases ownership too late, or duplicates records on retry.
- Window close and application quit exit before migration releases ownership.
- Settings migration leaves an adapter running or exits the app when its old service stops.
- Runtime startup failure leaves an unresponsive page or incorrectly completes setup.

## Acceptance plan

Add a canonical process case using the production setup flow, deterministic Rust-generated v1 fixtures, real Engine/SQLite, and a real service process. Verify fresh/established/restarted setup, credential fallback, cancellation, partial import, repair/retry, source preservation, target conflict, and service-visible imported history. Retain a receipt through the canonical runner.

Add a canonical software-view case for the production setup renderer and semantic controls across skins and themes. Do not run native/software GUI acceptance until explicitly authorized. Build checks and process checks do not establish prototype fidelity.

## Delivered implementation

- `app/desktop/internal/setupflow`: profile marker, source selection and structural detection, source/target overlap rejection, target ownership preflight, and existing offline importer integration.
- `app/desktop/internal/setupui`: welcome, confirmation, invalid source, system credential preparation, masked passphrase fallback, import, verification, safe stopping, and four terminal results. Reuses the existing UI Kit without new dependencies.
- `app/desktop/cmd/unic-desktop/session_gui.go`: service startup after user entry, completion persistence after readiness, safe quit during migration, and later migration through Settings > Storage.
- The normal runtime closes its quick panel and shortcut, cancels and waits for its adapter, then stops its owned service before opening later migration. Failed startup candidates release ownership before retry.
- Source strings are registered for the existing English, Japanese, Brazilian Portuguese and Russian locales. The agreed Chinese slogan remains exact.
- The existing native history-case fixture now explicitly completes setup before launching the desktop. This preserves those cases' declared history precondition.

## Results

Source HEAD: `1a6e8d617a87` with task changes in the working tree. No commit or push was performed. Each canonical report contains its own frozen source manifest and binary identities.

| Check | Result | Evidence |
|---|---|---|
| Canonical `first-install-setup` process case | PASS | `.build/desktop-e2e/20261113T152804Z-7e725936/index.html` |
| Canonical `startup-lifecycle` process case | PASS | `.build/desktop-e2e/20261113T152804Z-aaeb3937/index.html` |
| Canonical `production-boundary` process case | PASS | `.build/desktop-e2e/20261113T152801Z-97f34b24/index.html` |
| Retained report checksums | PASS | All entries in the three reports' `SHA256SUMS` were independently verified. |
| Final macOS arm64, Linux amd64 and Windows amd64 Desktop builds | PASS | `go build -buildvcs=false`; cross-builds use `CGO_ENABLED=0`. No executable was launched. |
| Acceptance test compilation | PASS | Desktop command and E2E package, `go test -run '^$'`. This does not execute a scenario. |
| Scoped `go vet`, catalogue consistency, translation entries/format arguments and `git diff --check` | PASS | Nonvisual checks only. |
| Documentation check | PASS with 8 stale-page warnings | `go run ./tools/docscheck`. |
| Architecture verification marker update | Blocked by uncommitted source | `docscheck -bump` correctly requires a commit first. Existing markers remain unchanged. |
| `setup-pages` software-view case | NOT_RUN | Registered before renderer implementation; requires the separate visual acceptance run. |
| Native setup, directory picker, system credential prompts, actual clipboard and OS focus | NOT_RUN | No native application or browser was launched. |

The process case imported four recoverable records from a five-record source, retained two commits across cancellation, repaired the synthetic failed record, then verified all five records. A subsequent import added zero records and skipped five. The real service exposed five history records. A concurrent migration rejected target ownership before invoking its credential reader and left the existing service generation unchanged.

The Rust-generated fixture uses timestamps near the Unix epoch. Its isolated service configuration disables automatic history cleanup so the normal age policy does not remove test records. Production retains its configured cleanup policy. The fixture's source database hash remained unchanged during import; later fixture repair is an explicit test mutation.

The last changes after the process snapshots only refine the zero-new-record result copy and its software-view scenario. The final platform builds and test compilation include those changes; visual results remain NOT_RUN.

## Reproduce

```sh
make desktop-e2e CASE=first-install-setup
make desktop-e2e CASE=startup-lifecycle
make desktop-e2e CASE=production-boundary
```

A restricted sandbox must permit local UNIX sockets for the process cases. The canonical runner owns temporary source snapshots, profiles, logs and process cleanup. Public receipts contain synthetic data only.

After visual execution is explicitly authorized, run `make desktop-e2e CASE=setup-pages`. This produces software-view PNG evidence across skins and themes. It does not establish native directory dialogs, system credential access, or operating-system window behavior. Compare the native flow against the supplied prototype separately before declaring visual acceptance.


## Native recording follow-up (2026-11-14)

The user explicitly authorized recording evidence for review. Use the canonical native runner with an isolated synthetic v1 source, unavailable synthetic system credentials, real passphrase fallback, actual offline import, and the production history interface.

Failure inventory before recording-harness changes: the native launcher incorrectly waits for a daemon before setup completes; recording starts after welcome disappears; test credentials touch the personal keychain; a source inside the target violates import ownership; tiny fixtures skip all visible progress between sampled frames; startup history cleanup removes epoch-dated synthetic records; source fixtures leak into release dependencies; the resulting movie changes size mid-stream and loses later frames; accessible labels collide or change; cleanup leaves the owned service alive.


### Recording results

| Case | Result | Captured frames | Duration | Report |
|---|---|---:|---:|---|
| `setup-onboarding` | PASS | 127 | 63.24 s | `.build/desktop-e2e/20261113T233004Z-4426e7e7/index.html` |
| `setup-partial` | PASS | 114 | 57.36 s | `.build/desktop-e2e/20261113T233149Z-e1001cd6/index.html` |
| `setup-pages` | PASS | Software-view PNG checkpoints | N/A | `.build/desktop-e2e/20261113T233012Z-57544393/index.html` |
| `production-boundary` after source-adapter changes | PASS | N/A | N/A | `.build/desktop-e2e/20261113T233250Z-367e2089/index.html` |

The complete recording covers welcome, confirmation, synthetic credential unavailability, wrong-passphrase retry, real import, real verification, complete result and history entry. Seventeen assertions passed. The service returned an exact count of 1505 imported records. The partial recording retains one unrecoverable fixture record, shows the partial result, and enters history with exactly 1504 records; fourteen assertions passed.

Both movies use H.264 at 2200 × 1440. `ffprobe` decoded 128 and 115 movie frames respectively: the extra final frame is the recorder's terminal hold frame, not an additional capture. Full `ffmpeg` decode completed without error. All retained SHA256SUMS entries were independently verified. Native verification, result and history screenshots were inspected, including a frame extracted from the end of the encoded complete-flow movie. The final enlarged history view remains present in the movie.

The recordings capture the actual macOS window content at real capture times. They exclude OS decorations and desktop compositing. Synthetic source discovery and unavailable credentials are restricted to the `desktop_e2e` build. The passphrase fallback, encrypted-source reader, importer, Engine, service and history UI are real. No personal profile, system keychain or system clipboard was accessed. All test-owned processes and private profiles were cleaned up successfully.

Two initial recording attempts remain marked FAIL: the first exposed an automation race between input updates and a disabled continuation button; the second hit the old 20-second assertion timeout while 1218 of 1505 records were still importing. The harness now asserts enabled controls before clicking and gives the actual bulk-import phase an explicit 90-second assertion timeout. No production migration delays or fabricated progress were added. The step-number badges were centered after inspection of the native screenshots.

Reproduce these recordings with:

```sh
make desktop-e2e CASE=setup-onboarding
make desktop-e2e CASE=setup-partial
make desktop-e2e CASE=setup-pages
```

These results supersede the earlier NOT_RUN entries only for the named macOS native and software-view scenarios. Native cancellation, native later Settings migration, native directory picker interaction, actual system credential prompts, and Windows/Linux native execution remain NOT_RUN. The earlier nonvisual cancellation and ownership evidence retains its original scope.


## PR integration (2026-11-14)

Integrated `origin/v2` at `6dd839548` into the feature branch. Preserve upstream history-detail prefetch and serialized native capture/RPC budgets. Upstream had already allocated decision 0025 to package identity, so this setup decision and receipt now use 0026; historical evidence retains its original frozen source identity.

After integration, canonical `first-install-setup` passed in `.build/desktop-e2e/20261114T033935Z-c2ddf77f/`, `setup-pages` passed in `.build/desktop-e2e/20261114T033931Z-8ecb1a1c/`, and `setup-onboarding` passed in `.build/desktop-e2e/20261114T033934Z-6dd546cd/` with a 71.84-second native recording. Scoped vet, test compilation, catalogue consistency and whitespace checks passed. Documentation checking passed with five stale warnings on other architecture topics. The three directly maintained architecture pages now verify integrated source commit `a531ab8b4`.

Setup and history still reuse one native window. Setup is 640 × 560; explicit entry or skip switches the same window to the 1100 × 720 history composition. Completion alone does not create a second window or enter history.
