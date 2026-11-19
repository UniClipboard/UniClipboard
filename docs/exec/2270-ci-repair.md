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
