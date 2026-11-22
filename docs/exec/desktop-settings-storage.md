# Desktop storage settings

- Date: 2026-11-22.
- Baseline: `fcf5611662039c1db79fc6537c4f79aff2a5b436`.
- Scope: Group B of the [settings work agreement](desktop-settings-parity.md).
- Status: Existing cache-inventory and native directory-action paths implemented; canonical process acceptance passed. Native file-manager execution remains NOT_RUN.

## Existing capabilities and implementation

The production Storage page reads `GET /v1/file-cache` through the existing
`Client.FileCache` method. It displays stored file bytes, pending transfer
reservations, managed file count and held file count. It exposes an explicit
refresh action and keeps the last successful sample with a visible warning if
the next read fails. Reads coalesce while one refresh is in progress and have a
five-second timeout through the existing view worker. The view stores aggregate
numbers only, not cache IDs, paths or filenames.

These are ledger measurements, not whole-profile disk usage. `StoredBytes`
counts files charged to the default cache ledger, including retained payloads
until their physical removal succeeds. `ReservedBytes` describes pending cache
admission. `Held` includes active holders and startup clipboard protection.
Database, history blobs outside that cache, logs, unknown files and user-managed
custom received files are outside this measurement. The service reports custom
destination selection separately; the view explains that exclusion.

The data-directory button invokes a Desktop host callback bound to the current
launcher's absolute profile directory. The host checks that the directory
exists and calls the existing mygo `Shell.OpenPath`. The view cannot supply a
path or URL. Native errors become fixed messages and never expose private
paths. The implementation does not invoke a shell command.

The existing migration action, unified history-cleanup rules, tag exclusions,
cache quota and received-directory description remain intact. This change adds
no configuration field, Engine method, HTTP route or new dependency.

## Failure inventory written before implementation

- Confusing ledger counts with database, logs or total disk usage.
- Counting or deleting unknown/custom/user-owned files.
- Showing a failed refresh as a fresh successful measurement.
- Queueing repeated refreshes or blocking UI frames on service I/O.
- Exposing raw errors or paths through directory-opening failures.
- Changing unrelated configuration merely by visiting or refreshing Storage.
- Losing existing cleanup controls while adding inventory.

## Acceptance

The canonical `settings-storage` process case starts a real race-instrumented
`unicd` in a private profile. It invokes the production settings model against
the service. No renderer, GUI, system clipboard or file manager is started.

Scenarios cover an empty default ledger, refresh coalescing, an unknown file
that must remain untouched and uncounted, unchanged config bytes, an injected
host directory callback, sanitized callback failure, a real service restart
with a custom destination and retained custom file, and a service-unavailable
refresh that keeps the previous sample with an error.

Replay:

```sh
make desktop-e2e CASE=settings-storage
```

The canonical report binds source and daemon hashes, scenario results and
private-profile cleanup. Its receipt contains aggregate checks only. Native
directory opening, rendered layout, platform file-manager permissions and
cross-platform native acceptance remain NOT_RUN.

The first run, `.build/desktop-e2e/20261122T071909Z-e198ad85/index.html`, failed
before the scenario executed. The frozen source included Diagnostics files
before their shared `viewState.diagnostics` integration. The compilation errors
name that missing field; private cleanup passed. This was a known unfinished
parallel integration, not a storage runtime failure. Keep that artifact.
After shared integration completes, allow at most two further runs with a
three-minute scenario budget. A repeated failure requires comparison and
diagnosis rather than additional retries.

The first integrated-source rerun passed at
`.build/desktop-e2e/20261122T072236Z-15c98377/index.html`.
All five report `SHA256SUMS` entries verified independently, including the
scenario receipt. The production-service/model assertions and private cleanup
passed. That source snapshot includes the shared Storage state, route and host
binding; it does not convert the earlier compile failure into a pass. The
receipt identifies the actual race-instrumented daemon bytes. No native UI,
renderer, clipboard or file-manager process ran.

## Contract-dependent follow-up

The following v1 parity items remain unavailable through existing public
operations. They require a concrete API proposal and the user's contract
decision before implementation:

1. Aggregate database/history/log disk inventory: add bounded, content-free
   service-owned size categories. Specify symlink, concurrent-write, quota and
   measurement-timing behavior; do not implement an unbounded Desktop scan.
2. Manual cache maintenance: define a service operation that reclaims only
   unreferenced, unheld ledger-owned payloads. It must not evict referenced
   payloads independently from history. The [history contract](../contracts/history-contract.md#unified-cleanup-and-storage-budgets)
   explicitly preserves active holders, shared references and user-managed
   files. A v1-style payload-only clear operation would change that contract.
3. Clear-all history: specify bounded pagination, revision/conflict behavior,
   cancellation, concurrent submissions and partial completion. The existing
   per-item revision-checked deletion API does not define an atomic clear-all
   operation. No naive page loop or filesystem removal was added.
4. Search status and manual rebuild: the [search contract](../contracts/history-search-contract.md#ownership-and-reads)
   exposes three reads and startup derived repair, but no public maintenance
   operation or job state. Define repair ownership, cancellation, query behavior,
   bounded admission and truthful progress before implementing a rebuild button.

`desktop.confirm_before_delete` remains only a planning reservation. No bulk
delete control or hidden confirmation bypass was introduced. Configuration
backup belongs to group G and is outside this implementation.
