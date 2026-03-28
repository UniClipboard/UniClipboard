---
phase: 71-dual-product-release-pipeline-for-cli-and-app
plan: 01
subsystem: infra
tags: [cargo, workspace, release-pipeline, ci, github-actions]

# Dependency graph
requires: []
provides:
  - uc-cli/Cargo.toml uses version.workspace = true (no more hardcoded 0.1.0 drift)
  - prepare-release.yml runs cargo update -p uc-cli after version bump
  - release.yml runs cargo update -p uc-cli after version bump
  - Cargo.lock always reflects correct workspace version for uc-cli after CI bump
affects:
  - 71-02-PLAN.md
  - 71-03-PLAN.md

# Tech tracking
tech-stack:
  added: []
  patterns:
    - CI delegates Cargo.lock refresh for workspace members to cargo update (not fragile JS regex patching)
    - dtolnay/rust-toolchain@stable installed inline before cargo update in release workflows

key-files:
  created: []
  modified:
    - src-tauri/crates/uc-cli/Cargo.toml
    - scripts/__tests__/bump-version.test.ts
    - .github/workflows/prepare-release.yml
    - .github/workflows/release.yml

key-decisions:
  - "Cargo.lock refresh for workspace members delegated to cargo update -p uc-cli in CI (not JS regex patching) per RESEARCH.md Pattern 2 and Pitfall 5"
  - "dtolnay/rust-toolchain@stable added inline (not via job-level setup) to keep Rust toolchain install scoped only to the bump step"
  - "cargo update step in release.yml gated with if: github.event_name == 'workflow_dispatch' to avoid running on tag-push events"

patterns-established:
  - "Workspace crates that need version tracking: use version.workspace = true; CI handles Cargo.lock"

requirements-completed: [PH71-01, PH71-02]

# Metrics
duration: 2min
completed: 2026-03-28
---

# Phase 71 Plan 01: CLI Workspace Version Alignment Summary

**uc-cli switched to workspace version inheritance with CI cargo update -p uc-cli ensuring Cargo.lock stays synchronized after every release bump**

## Performance

- **Duration:** ~2 min
- **Started:** 2026-03-28T07:28:43Z
- **Completed:** 2026-03-28T07:30:14Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments

- Replaced hardcoded `version = "0.1.0"` in uc-cli/Cargo.toml with `version.workspace = true` to eliminate version drift
- Added static test in bump-version.test.ts asserting uc-cli uses workspace version (not a hardcoded semver)
- Added `cargo update -p uc-cli` step to prepare-release.yml after version bump, before the commit that includes Cargo.lock
- Added same `cargo update -p uc-cli` step to release.yml validate job, guarded by workflow_dispatch condition

## Task Commits

Each task was committed atomically:

1. **Task 1: Align uc-cli version to workspace inheritance** - `ba902de4` (feat)
2. **Task 2: Add cargo update -p uc-cli step to CI workflows** - `95a22148` (chore)

## Files Created/Modified

- `src-tauri/crates/uc-cli/Cargo.toml` - version field changed to version.workspace = true
- `scripts/__tests__/bump-version.test.ts` - added workspace version inheritance test block
- `.github/workflows/prepare-release.yml` - added dtolnay/rust-toolchain@stable + cargo update step after bump-version.js
- `.github/workflows/release.yml` - added same steps (with workflow_dispatch guard) after bump-version.js

## Decisions Made

- Cargo.lock refresh for workspace members delegated to `cargo update -p uc-cli` in CI (not fragile JS regex) per RESEARCH.md Pattern 2 and Pitfall 5
- Rust toolchain setup added inline just before cargo update (not at job level) to keep it scoped
- cargo update steps in release.yml gated with `if: github.event_name == 'workflow_dispatch'` — not needed on tag-push since version is already committed

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- uc-cli now inherits workspace version automatically
- CI release workflows will correctly refresh Cargo.lock after each version bump
- Ready for Phase 71-02 (CLI binary publishing pipeline)

---
*Phase: 71-dual-product-release-pipeline-for-cli-and-app*
*Completed: 2026-03-28*
