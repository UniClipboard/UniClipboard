---
phase: 71-dual-product-release-pipeline-for-cli-and-app
plan: 03
subsystem: infra
tags: [github-actions, release-pipeline, cli, release-notes, artifact-collection]

# Dependency graph
requires:
  - phase: 71-01
    provides: release.yml with validate/build/create-release job structure
  - phase: 71-02
    provides: build-cli.yml workflow that produces uniclipboard-cli-*.tar.gz and *.zip artifacts
provides:
  - release.yml calls build-cli.yml in parallel with build.yml
  - CLI archives collected from artifacts/ into release-assets/ before GitHub Release creation
  - Release notes template has separate App Installation and CLI Downloads sections
  - buildCliInstallerLines() function detects CLI artifacts by filename pattern
affects: [future-release-automation, github-release-notes]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - Parallel build jobs (build + build-cli) both feeding into single create-release job
    - CLI artifact collection uses separate find loop with unique-name assumption (no collision handling)
    - CLI installer lines follow same makeLink pattern as app installer lines

key-files:
  created: []
  modified:
    - .github/workflows/release.yml
    - .github/release-notes/release.md.tmpl
    - scripts/generate-release-notes.js
    - scripts/__tests__/generate-release-notes.test.ts

key-decisions:
  - 'CLI artifact collection uses separate find loop after app artifacts — CLI archives have unique names so no collision handling needed'
  - 'App Installation section renamed from Installation in template to distinguish from CLI Downloads'
  - 'buildCliInstallerLines() detects CLI artifacts by uniclipboard-cli- prefix + target triple in filename'

patterns-established:
  - 'Parallel product builds: both build and build-cli run in parallel, gated by validate, before create-release'

requirements-completed: [PH71-04, PH71-05, PH71-06]

# Metrics
duration: 2min
completed: 2026-03-28
---

# Phase 71 Plan 03: Wire CLI into Release Pipeline Summary

**release.yml now orchestrates parallel App + CLI builds and produces a single GitHub Release with both artifact sets and CLI-specific download links in release notes**

## Performance

- **Duration:** 2 min
- **Started:** 2026-03-28T14:32:42Z
- **Completed:** 2026-03-28T14:34:39Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments

- release.yml calls build-cli.yml as a parallel job alongside build.yml, both gated by validate
- CLI archives (uniclipboard-cli-_.tar.gz and _.zip) are collected from artifacts/ into release-assets/ before GitHub Release creation, with verification step
- Release notes template split into "App Installation" and "CLI Downloads" sections with per-platform placeholders
- buildCliInstallerLines() added to generate-release-notes.js to detect and link CLI artifacts by filename pattern
- Tests updated to cover CLI download link generation (3 tests passing)

## Task Commits

1. **Task 1: Add build-cli job to release.yml and collect CLI artifacts** - `a70b036c` (feat)
2. **Task 2: Extend release notes template and generator with CLI Downloads section** - `a3636d60` (feat)

**Plan metadata:** (docs commit follows)

## Files Created/Modified

- `.github/workflows/release.yml` - Added build-cli job, updated needs array, added CLI artifact collection loop and verification
- `.github/release-notes/release.md.tmpl` - Renamed Installation to App Installation, added CLI Downloads section with CLI\_\*\_INSTALLERS placeholders
- `scripts/generate-release-notes.js` - Added buildCliInstallerLines() function, wired CLI installers into renderTemplate call
- `scripts/__tests__/generate-release-notes.test.ts` - Updated setupRepo() template to include CLI placeholders, added CLI Downloads test case

## Decisions Made

- CLI artifact collection uses a separate find loop with no collision handling — CLI archives have unique names (version + target triple in filename)
- App Installation section renamed from Installation in template to clearly distinguish from CLI Downloads
- buildCliInstallerLines() detects artifacts by uniclipboard-cli- prefix + target triple patterns (aarch64-apple-darwin, x86_64-apple-darwin, linux-gnu, windows-msvc)

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Phase 71 complete: dual-product release pipeline fully wired
- CLI and App artifacts both included in every release from this point forward
- R2 upload handles CLI artifacts automatically (existing loop uploads all release-assets/\*)
- Update manifest remains App-only (assemble-update-manifest.js not modified)

---

_Phase: 71-dual-product-release-pipeline-for-cli-and-app_
_Completed: 2026-03-28_
