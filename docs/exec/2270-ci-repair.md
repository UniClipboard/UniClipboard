# PR 2270 package acceptance repair

## Failure inventory (before implementation)

The six native package jobs in run `44452818995`, attempt 1, checked PR head
`c3e85ec93613662bd57dc7ef4aff9f1ac387369b`. Every platform failed in the
Desktop stage smoke: a fresh profile ran `unic config --json` without explicit
migration initialization and returned exit 6, `migration_required`. macOS local
`TMPDIR=/private/tmp python3 e2e/packaging/run.py --output <PRIVATE>/red-package`
reproduced that exact failure before any fix.

Other paths in the same process suite must obey the same rule:

- Version inspection must remain state-free.
- A bundled CLI business command must refuse an uninitialized private profile,
  keep its machine-readable reason and create no database or service socket.
- The shared-configuration smoke must explicitly initialize its private profile
  through the production CLI before starting a service.
- Discovery HOME, XDG and Windows profile variables must all be private. Never
  inspect a developer's personal legacy data or system credentials.
- Portable `data/` and a separate explicit `UNIC_HOME` must each be initialized;
  initializing one cannot grant permission to another.
- The disposable DEB install/reinstall/upgrade/remove scenario must explicitly
  initialize once, preserve that receipt through upgrade and retain user data
  on removal.
- Do not weaken the CLI gate, write a receipt directly, skip failing checks,
  default to consent or add a bypass environment variable.

## Acceptance entry

Use the existing `e2e/packaging/run.py` real Go/npm/packager suite. Add refusal
assertions before repairing fixtures. Re-run the exact entry locally, then
verify all six hosted package jobs on the repaired PR head. Linux installation
lifecycle runs only in a disposable container. No GUI is started.


## Local result

The original canonical entry failed twice at the same production smoke before
repair. The failure-first extension first proved that the packaged CLI refused
with exit 6 and did not create a database, receipt or socket.

After repair, `TMPDIR=/private/tmp python3 e2e/packaging/run.py --output
<PRIVATE>/green-package` passed on macOS arm64. It checked real locked package
bytes and versions, explicit initialization, independent portable/explicit
profiles, shared-configuration IPC and temporary service shutdown. Python and
shell syntax checks and `git diff --check` passed. No GUI, personal profile,
credential store, public registry publication or native installer was used.

The production CLI gate is unchanged. `tools/package/desktop.py smoke` invokes
the real initialization command in a separate private profile with private
HOME/XDG/Windows discovery paths and no inherited socket override. Portable
fixtures initialize each actual selected root. The disposable DEB script
initializes before `status` and checks the receipt remains usable after upgrade
and remains on disk after removal. Hosted six-target and Linux installer results
must be verified on the pushed repair head; local macOS results do not replace
those checks.

## Desktop CI provisioning follow-up

After the six repaired package jobs passed on `d3d1e54`, the remaining Desktop
job spent fourteen minutes downloading Ubuntu 26.04 graphics dependencies before
its first attempt was cancelled for an isolated retry. The captured log showed
ongoing downloads, including the 25.7 MB WebKitGTK library; it did not show a
project test failure. The retry used the same commit and unchanged test scope.

The primary PR Desktop job still used `ubuntu-latest` and a WebKitGTK dependency
list. The actual Desktop creates `Content: ui.View(...)`, uses the pinned mygo
native GTK path, and does not show web pages. The SDK's own doctor/packager only
requires WebKitGTK for webview applications. The existing six-platform native
acceptance already selects Ubuntu 24.04 and GTK 3 plus the tray indicator runtime.

Align the primary job with that existing supported baseline and dependency list.
Keep the same complete `make desktop-e2e` command and the same thirty-minute job
budget. Do not skip cases, raise timeouts or weaken migration admission. Hosted
Desktop execution must validate the changed provisioning on the new PR head.

## Startup lifecycle fixture follow-up

While the full hosted suite was running, a call-site audit found one more
pre-gate fixture: `scenarioStartupProcesses` created two empty profiles, started
services, then invoked the CLI without initializing either profile. The isolated
canonical `make desktop-e2e CASE=startup-lifecycle` reproduced `unic send` exit 6
on line 190 before this fixture repair. Its cleanup passed.

The regression keeps the initial business-command refusal and asserts no
database/socket creation. It then initializes both profiles through the real CLI
before any service launch. HOME, XDG and Windows discovery variables are private;
no completion receipt is written directly. The existing lifecycle/IPC/login
fixture assertions and nonvisual scope remain intact.

## Hosted full-suite failure inventory (before further changes)

Run `44458629169` on `c6edaf3` installed native prerequisites in 21 seconds.
It then reached the unchanged thirty-minute job deadline after 33 case results.
Two emitted failures have the same exit-6 cause: `startup-lifecycle` and
`tray-icon-settings`. The remaining 31 emitted results passed. Missing results
are NOT_RUN, never passes. The tray scenario calls the production CLI after
starting its service, so it must explicitly initialize its private profile
before that service acquires the target lock.

The repaired startup case passed locally through the canonical process entry
with race instrumentation and cleanup. Tray acceptance will use its existing
software-view entry; it does not open a native window.

To execute the entire growing catalogue within the existing per-job budget,
partition the sorted catalogue across three independent hosted runners. Before
implementation, require: disjoint selections; union equals the complete current
catalogue; invalid, empty and mixed single-case/shard requests fail before a
build; new cases automatically join exactly one shard; all shards run despite
another failing; each shard preserves normal source freezing, race flags,
assertions, evidence and cleanup. Keep the thirty-minute limit. Do not execute
native cases concurrently against the same desktop. Do not hardcode a partial
case list or treat another shard's unselected cases as passes.

## Local Desktop repair result

Both canonical entries passed after repair: `make desktop-e2e
CASE=startup-lifecycle` and `make desktop-e2e CASE=tray-icon-settings`. Both used
real CLI/service processes and private discovery paths. The process case kept
race instrumentation and all lifecycle assertions. The tray case kept its full
software-view/configuration-watcher assertions. Both reported cleanup PASS.
No native window, real clipboard, login registration or credential store was
used by these local runs.

CLI selection validation found 47 catalogue cases partitioned as 16/16/15,
with a disjoint union equal to the complete catalogue. Six malformed/out-of-range
shard requests and a mixed CASE/shard request refused before execution.
Pinned golangci-lint 2.15.0 reported zero issues and no formatting differences.
Hosted full-suite acceptance remains required on the pushed repair head.

## Translation catalogue failure inventory (before repair)

On `996b98c`, shard 1 executed all sixteen selected cases. Fifteen passed,
including the repaired tray fixture. The existing `i18n` view E2E failed before
rendering: `live_devices.go` used an uncatalogued literal. Auditing the same
literal pattern found eleven missing catalogue keys across device approval and
plugin settings, introduced by previously integrated features.

Use the unchanged canonical `CASE=i18n` to reproduce before changing the
catalogue. Supply all four supported translations for every missing key. Preserve
format arguments, source keys, device-name trust wording and user content.
Do not relax catalogue completeness assertions or substitute Chinese strings for
missing translations. Re-run the full existing five-language/three-skin/two-theme
view scenario, then verify all hosted shards on the new head.

The unchanged canonical i18n entry failed locally before the catalogue repair
and passed afterward across all thirty language/skin/theme combinations.
Private cleanup and source revalidation passed. The repair adds eleven keys
with English, Japanese, Brazilian Portuguese and Russian translations. It
changes no source labels, controls, device trust behavior or catalogue checks.
The small redacted receipt and checksums are retained beside the package proof.
