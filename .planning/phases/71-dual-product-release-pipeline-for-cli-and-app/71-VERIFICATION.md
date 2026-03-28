---
phase: 71-dual-product-release-pipeline-for-cli-and-app
verified: 2026-03-28T15:00:00Z
status: passed
score: 6/6 must-haves verified
re_verification: false
---

# Phase 71: Dual-Product Release Pipeline Verification Report

**Phase Goal:** Align CLI version to workspace inheritance, create a parallel CLI build workflow, and integrate CLI artifacts into the existing release pipeline so every release ships both App bundles and CLI binaries.
**Verified:** 2026-03-28T15:00:00Z
**Status:** passed
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths (Success Criteria from ROADMAP.md)

| #   | Truth                                                                                              | Status     | Evidence                                                                                   |
| --- | -------------------------------------------------------------------------------------------------- | ---------- | ------------------------------------------------------------------------------------------ |
| 1   | `uc-cli/Cargo.toml` uses `version.workspace = true` (no hardcoded version)                         | ✓ VERIFIED | Line 3: `version.workspace = true`; Cargo.lock shows `uc-cli` at `0.4.0-alpha.1`           |
| 2   | CI workflows refresh Cargo.lock workspace members after version bumps via `cargo update -p uc-cli` | ✓ VERIFIED | prepare-release.yml line 96; release.yml line 88 (workflow_dispatch-gated)                 |
| 3   | `build-cli.yml` reusable workflow exists and builds CLI binaries for all 4 platforms               | ✓ VERIFIED | 126-line file; `workflow_call` + `workflow_dispatch` triggers; 4-platform matrix           |
| 4   | `release.yml` calls both `build.yml` and `build-cli.yml`, downloads CLI artifacts, includes them   | ✓ VERIFIED | Lines 156-162: `build-cli` job; line 166: `needs: [validate, build, build-cli]`            |
| 5   | Release notes include a "CLI Downloads" section with platform-specific download links              | ✓ VERIFIED | Template has `## CLI Downloads`; generator has `buildCliInstallerLines()`; 3 tests pass    |
| 6   | Update manifest (`assemble-update-manifest.js`) remains App-only (unchanged)                       | ✓ VERIFIED | No CLI references in assemble-update-manifest.js; release.yml unchanged for manifest steps |

**Score:** 6/6 truths verified

### Required Artifacts

| Artifact                                           | Expected                                 | Status     | Details                                                   |
| -------------------------------------------------- | ---------------------------------------- | ---------- | --------------------------------------------------------- |
| `src-tauri/crates/uc-cli/Cargo.toml`               | `version.workspace = true`               | ✓ VERIFIED | Contains `version.workspace = true` on line 3             |
| `.github/workflows/build-cli.yml`                  | Reusable workflow, `workflow_call`, 60+L | ✓ VERIFIED | 126 lines; `workflow_call` at line 17; valid structure    |
| `.github/workflows/prepare-release.yml`            | `cargo update -p uc-cli` step            | ✓ VERIFIED | Line 96; placed after bump-version.js, before git commit  |
| `.github/workflows/release.yml`                    | `build-cli.yml` call + CLI collection    | ✓ VERIFIED | Lines 156-162 (build-cli job); lines 240-251 (collection) |
| `.github/release-notes/release.md.tmpl`            | CLI section with `CLI_MACOS_INSTALLERS`  | ✓ VERIFIED | Lines 17-26: `## CLI Downloads` with all 3 placeholders   |
| `scripts/generate-release-notes.js`                | `buildCliInstallerLines` function        | ✓ VERIFIED | Lines 134-174: full implementation; wired at line 249     |
| `scripts/__tests__/generate-release-notes.test.ts` | CLI Downloads test case                  | ✓ VERIFIED | Test at line 85 asserting CLI artifact links; 3/3 pass    |

### Key Link Verification

| From                                    | To                                      | Via                                         | Status  | Details                                                                          |
| --------------------------------------- | --------------------------------------- | ------------------------------------------- | ------- | -------------------------------------------------------------------------------- |
| `.github/workflows/prepare-release.yml` | `src-tauri/Cargo.lock`                  | `cargo update -p uc-cli` after bump-version | ✓ WIRED | Step at line 95-96; `git add ... src-tauri/Cargo.lock` at line 147               |
| `.github/workflows/release.yml`         | `src-tauri/Cargo.lock`                  | `cargo update -p uc-cli` after bump-version | ✓ WIRED | Step at lines 86-88; `git add ... src-tauri/Cargo.lock` at line 116              |
| `.github/workflows/release.yml`         | `.github/workflows/build-cli.yml`       | `uses: ./.github/workflows/build-cli.yml`   | ✓ WIRED | Lines 158: exact `uses:` reference; job depends on validate                      |
| `.github/workflows/build-cli.yml`       | `src-tauri/crates/uc-cli`               | `cargo build --release -p uc-cli`           | ✓ WIRED | Line 93: `cargo build --release -p uc-cli ${{ matrix.args }}`                    |
| `scripts/generate-release-notes.js`     | `.github/release-notes/release.md.tmpl` | `CLI_MACOS_INSTALLERS` placeholder          | ✓ WIRED | Lines 267-269: all three CLI placeholder replacements present                    |
| CLI archives in `release-assets/`       | GitHub Release + R2 upload              | `files: release-assets/*` and R2 loop       | ✓ WIRED | `softprops/action-gh-release` uses `release-assets/*`; R2 loop iterates same dir |

### Data-Flow Trace (Level 4)

Not applicable — this phase produces CI workflow files and scripts, not components rendering dynamic UI data. The "data flow" is CI job-to-job artifact passing, which is verified through the key links above.

### Behavioral Spot-Checks

| Behavior                                            | Command                                                                               | Result                                      | Status |
| --------------------------------------------------- | ------------------------------------------------------------------------------------- | ------------------------------------------- | ------ |
| `uc-cli/Cargo.toml` uses workspace version          | `grep 'version.workspace = true' src-tauri/crates/uc-cli/Cargo.toml`                  | 1 match                                     | ✓ PASS |
| Cargo.lock reflects workspace version for uc-cli    | `grep -A3 'name = "uc-cli"' src-tauri/Cargo.lock`                                     | version = 0.4.0-alpha.1 (matches workspace) | ✓ PASS |
| generate-release-notes tests pass                   | `bun test -- scripts/__tests__/generate-release-notes.test.ts`                        | 3 pass, 0 fail                              | ✓ PASS |
| bump-version tests pass (incl. workspace assertion) | `bun test -- scripts/__tests__/bump-version.test.ts`                                  | 6 pass, 0 fail                              | ✓ PASS |
| build-cli.yml has all required sections             | content check for `workflow_call`, `workflow_dispatch`, `setup-matrix`, `cargo build` | all 5 found                                 | ✓ PASS |
| CLI artifact collection wired into release.yml      | `grep -c 'uniclipboard-cli-' .github/workflows/release.yml`                           | 3 matches                                   | ✓ PASS |

### Requirements Coverage

The requirement IDs PH71-01 through PH71-06 are declared in PLAN frontmatter and ROADMAP.md but are not present in `.planning/REQUIREMENTS.md`. This is expected — these are phase-internal IDs used for tracking within the phase, not cross-cutting product requirements. REQUIREMENTS.md covers user-facing functional requirements (EVNT-xx, RNTM-xx, CLI-xx, etc.) rather than CI/release infrastructure. No orphaned requirements were found.

| Requirement | Source Plan | Description (from ROADMAP)                                               | Status      | Evidence                                                 |
| ----------- | ----------- | ------------------------------------------------------------------------ | ----------- | -------------------------------------------------------- |
| PH71-01     | 71-01-PLAN  | uc-cli uses workspace version inheritance                                | ✓ SATISFIED | `version.workspace = true` confirmed in Cargo.toml       |
| PH71-02     | 71-01-PLAN  | CI refreshes Cargo.lock via `cargo update -p uc-cli` after version bumps | ✓ SATISFIED | Step present in both prepare-release.yml and release.yml |
| PH71-03     | 71-02-PLAN  | `build-cli.yml` reusable workflow for cross-platform CLI builds          | ✓ SATISFIED | 126-line workflow with 4-platform matrix confirmed       |
| PH71-04     | 71-03-PLAN  | `release.yml` calls `build-cli.yml` alongside `build.yml`                | ✓ SATISFIED | `build-cli` job at lines 156-162 confirmed               |
| PH71-05     | 71-03-PLAN  | CLI artifacts included in GitHub Release and R2                          | ✓ SATISFIED | Collection loop + `files: release-assets/*` + R2 loop    |
| PH71-06     | 71-03-PLAN  | Release notes include CLI Downloads section                              | ✓ SATISFIED | Template, generator function, and test all confirmed     |

### Anti-Patterns Found

No anti-patterns found in phase-modified files.

Checked files: `src-tauri/crates/uc-cli/Cargo.toml`, `.github/workflows/build-cli.yml`, `.github/workflows/prepare-release.yml`, `.github/workflows/release.yml`, `.github/release-notes/release.md.tmpl`, `scripts/generate-release-notes.js`, `scripts/__tests__/generate-release-notes.test.ts`.

Notable items (info-level only):

- `scripts/generate-release-notes.js` passes `VERIFICATION_SECTION: ''` to `renderTemplate` but the template no longer contains the `{{VERIFICATION_SECTION}}` placeholder — the text is now hardcoded in the template. This is benign (unused replacement key does nothing), but could be cleaned up in a future pass.

### Human Verification Required

No items require human verification for the core pipeline logic. However, the following can only be confirmed during an actual release run:

1. **End-to-end CLI artifact flow**
   - **Test:** Trigger a manual `workflow_dispatch` run of `release.yml` against a test tag
   - **Expected:** Four `cli-{target}` artifacts appear in the run, are collected into `release-assets/`, and show up in the GitHub Release draft
   - **Why human:** GitHub Actions environment required; cannot test artifact download/upload locally

2. **CLI binary execution on target platforms**
   - **Test:** Download a produced `uniclipboard-cli-{version}-{target}.tar.gz` and run `./uniclipboard-cli --help`
   - **Expected:** CLI starts without errors, shows help text
   - **Why human:** Requires an actual compiled binary from a CI run

### Gaps Summary

No gaps. All six success criteria are satisfied by the actual codebase, not just documented claims.

---

_Verified: 2026-03-28T15:00:00Z_
_Verifier: Claude (gsd-verifier)_
