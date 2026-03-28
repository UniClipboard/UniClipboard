---
phase: 68-adopt-tauri-sidecar-for-daemon
plan: 01
subsystem: infra
tags: [tauri, sidecar, build-rs, shell-plugin, capabilities, daemon]

# Dependency graph
requires:
  - phase: 66-fix-daemon-ws-reconnection
    provides: working daemon binary (uniclipboard-daemon) ready for sidecar adoption
provides:
  - tauri-plugin-shell dependency in workspace and uc-tauri crates
  - externalBin declaration in tauri.conf.json for daemon sidecar
  - build.rs daemon binary copy logic staging binaries/ for Tauri bundling
  - shell:allow-spawn capability permission for daemon sidecar with --gui-managed arg
  - src-tauri/binaries/ gitignored to exclude build artifacts
affects:
  - 68-02 (sidecar spawn migration — depends on this infrastructure)
  - CI/CD build workflows (externalBin changes bundling behavior)

# Tech tracking
tech-stack:
  added:
    - tauri-plugin-shell = "2" (Tauri v2 official sidecar/shell plugin)
  patterns:
    - build.rs daemon staging: copy_daemon_binary_to_binaries() runs BEFORE tauri_build::build() so externalBin validation succeeds
    - TAURI_ENV_TARGET_TRIPLE primary triple source with CARGO_CFG_* fallback for bare cargo builds
    - Non-fatal build.rs: emits cargo:warning on missing daemon binary (clean checkout safe)

key-files:
  created: []
  modified:
    - src-tauri/build.rs
    - src-tauri/Cargo.toml
    - src-tauri/crates/uc-tauri/Cargo.toml
    - src-tauri/tauri.conf.json
    - src-tauri/capabilities/default.json
    - .gitignore

key-decisions:
  - "Copy daemon binary BEFORE tauri_build::build() so Tauri externalBin path validation finds the staged binary"
  - "tauri-plugin-shell added to workspace.dependencies for future reuse by other workspace crates"
  - "shell:allow-spawn capability uses sidecar=true with explicit args=[--gui-managed] for Tauri v2 capability enforcement"
  - "build.rs placed in src-tauri/ (main crate) not uc-tauri/ so TAURI_ENV_TARGET_TRIPLE is available from Tauri CLI"

patterns-established:
  - "Sidecar staging: build.rs copies target/{profile}/uniclipboard-daemon to binaries/uniclipboard-daemon-{triple} before tauri_build::build()"
  - "Capability format: shell:allow-spawn as object with identifier + allow array containing sidecar=true scoped rule"

requirements-completed: [PH68-01, PH68-02, PH68-05]

# Metrics
duration: 5min
completed: 2026-03-28
---

# Phase 68 Plan 01: Tauri Sidecar Infrastructure Summary

**Tauri sidecar infrastructure configured: tauri-plugin-shell added, externalBin declared, build.rs stages daemon binary with target-triple suffix, shell:allow-spawn capability granted**

## Performance

- **Duration:** 5 min
- **Started:** 2026-03-28T04:20:36Z
- **Completed:** 2026-03-28T04:25:19Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments

- Added `tauri-plugin-shell = "2"` to workspace and uniclipboard/uc-tauri Cargo.toml, enabling Tauri sidecar spawn API
- Configured `externalBin: ["binaries/uniclipboard-daemon"]` in tauri.conf.json for Tauri bundler to include daemon binary
- Extended `src-tauri/build.rs` with `copy_daemon_binary_to_binaries()` that stages daemon binary before `tauri_build::build()` validates paths
- Added `shell:allow-spawn` capability permission with `sidecar=true` and `--gui-managed` arg to capabilities/default.json
- Added `src-tauri/binaries/` to .gitignore to prevent build artifacts from being committed

## Task Commits

Each task was committed atomically:

1. **Task 1: Add tauri-plugin-shell dependency and externalBin config** - `c3edc573` (feat)
2. **Task 2: Add daemon binary copy logic to build.rs** - `44497f12` (feat)

## Files Created/Modified

- `src-tauri/build.rs` - Added copy_daemon_binary_to_binaries() and construct_triple_from_cfg() before tauri_build::build()
- `src-tauri/Cargo.toml` - Added tauri-plugin-shell = "2" to [dependencies] and [workspace.dependencies]
- `src-tauri/crates/uc-tauri/Cargo.toml` - Added tauri-plugin-shell = { workspace = true } in # Tauri section
- `src-tauri/tauri.conf.json` - Added externalBin array with "binaries/uniclipboard-daemon"
- `src-tauri/capabilities/default.json` - Added shell:allow-spawn permission object with sidecar allow rule
- `.gitignore` - Added src-tauri/binaries/ line

## Decisions Made

- **Copy order in build.rs**: `copy_daemon_binary_to_binaries()` runs BEFORE `tauri_build::build()` — this is critical because Tauri validates externalBin paths during its build step. If the copy runs after, the staged binary doesn't exist when Tauri checks for it.
- **Workspace dependency**: Added `tauri-plugin-shell` to `[workspace.dependencies]` for consistency with `tauri-plugin-autostart` pattern and future reuse.
- **build.rs location**: Placed in `src-tauri/build.rs` (main crate), not a new `uc-tauri/build.rs`, because `TAURI_ENV_TARGET_TRIPLE` is set by the Tauri CLI at the workspace level.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Reordered copy_daemon_binary_to_binaries() to run before tauri_build::build()**

- **Found during:** Task 2 (verification)
- **Issue:** Plan specified `tauri_build::build()` must remain first, but `cargo check -p uniclipboard` failed with `resource path 'binaries/uniclipboard-daemon-aarch64-apple-darwin' doesn't exist` because Tauri validates externalBin paths inside `tauri_build::build()`. Running copy after means Tauri sees missing binary.
- **Fix:** Moved `copy_daemon_binary_to_binaries()` call to execute BEFORE `tauri_build::build()` so the binary is staged when Tauri validates it.
- **Files modified:** src-tauri/build.rs
- **Verification:** `cargo build -p uc-daemon && cargo check -p uniclipboard` succeeds; binaries/ shows `uniclipboard-daemon-aarch64-apple-darwin` (105MB)
- **Committed in:** 44497f12

---

**Total deviations:** 1 auto-fixed (Rule 1 - bug fix: build order correction)
**Impact on plan:** Essential for correctness — without this fix cargo check would fail on any system with externalBin declared.

## Issues Encountered

- Initial build.rs had `tauri_build::build()` first per plan instructions, but Tauri v2 validates externalBin paths inside that call. The daemon copy must precede it. Fixed during Task 2 verification.

## User Setup Required

None — no external service configuration required. First-run note: run `cd src-tauri && cargo build -p uc-daemon` before `bun tauri dev` on clean checkout so the daemon binary exists for staging.

## Next Phase Readiness

- Tauri sidecar infrastructure fully configured (externalBin, capabilities, dependencies, build staging)
- Plan 02 can now replace `std::process::Command` spawn in `run.rs` with `app.shell().sidecar("uniclipboard-daemon")`
- `tauri-plugin-shell` dependency is available in uc-tauri for Plan 02 implementation

---
*Phase: 68-adopt-tauri-sidecar-for-daemon*
*Completed: 2026-03-28*
