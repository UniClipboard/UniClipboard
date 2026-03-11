---
phase: quick-6
plan: 01
subsystem: ci-cd
tags: [github-actions, release-automation, workflow]
dependency_graph:
  requires: []
  provides: [prepare-release-workflow, tag-on-merge-workflow]
  affects: [release.yml]
tech_stack:
  added: [openai/codex-action]
  patterns: [workflow_dispatch, pull_request-closed-trigger, REPO_BOT_TOKEN-for-cross-workflow]
key_files:
  created:
    - .github/workflows/prepare-release.yml
    - .github/workflows/tag-on-merge.yml
  modified: []
decisions:
  - Used REPO_BOT_TOKEN (PAT) instead of GITHUB_TOKEN to ensure cross-workflow triggering
  - Codex polish step is conditional on CODEX_API_KEY secret availability
  - Release branch cleanup in tag-on-merge uses continue-on-error for resilience
metrics:
  duration_seconds: 132
  completed: 2026-03-11T05:01:33Z
---

# Quick Task 6 Plan 01: Auto PR Release Bot Summary

Two GitHub Actions workflows automating the release preparation and tagging pipeline using REPO_BOT_TOKEN for cross-workflow triggering.

## Tasks Completed

| # | Task | Commit | Key Files |
|---|------|--------|-----------|
| 1 | Create prepare-release.yml workflow | 7634e779 | .github/workflows/prepare-release.yml |
| 2 | Create tag-on-merge.yml workflow | 43581bc7 | .github/workflows/tag-on-merge.yml |

## What Was Built

### prepare-release.yml
- **Trigger:** `workflow_dispatch` with inputs: version (exact semver), bump (patch/minor/major), channel (stable/alpha/beta/rc), base_branch
- **Flow:** Checkout -> determine version -> create release/vX.Y.Z branch -> bump version via `scripts/bump-version.js --to` -> generate changelog skeleton -> commit -> optional Codex polish -> push branch -> create PR via `gh pr create`
- **Auth:** Uses `REPO_BOT_TOKEN` for checkout and PR creation to enable cross-workflow triggering

### tag-on-merge.yml
- **Trigger:** `pull_request: types: [closed]` on branches `[main]`
- **Guard:** Job-level `if` ensures only merged `release/v*` PRs trigger the workflow
- **Flow:** Extract version from branch name -> checkout main -> validate version matches package.json -> check no existing tag -> create annotated tag -> push tag (triggers release.yml) -> delete release branch (best-effort)
- **Auth:** Uses `REPO_BOT_TOKEN` for checkout and tag push to trigger release.yml

### End-to-End Flow
1. Developer runs `prepare-release` workflow with a version
2. Workflow creates branch, bumps version, optionally polishes changelog, opens PR
3. Developer reviews and merges PR
4. `tag-on-merge` automatically creates annotated tag `vX.Y.Z`
5. Tag push triggers existing `release.yml` which builds artifacts and creates GitHub Release

## Deviations from Plan

None - plan executed exactly as written.

## Decisions Made

1. **Conditional Codex step:** Both the codex action and its commit step check `env.CODEX_API_KEY != ''` so the workflow gracefully skips when the secret is not configured
2. **Changelog fallback:** If `generate-release-notes.js` fails (e.g., missing template), creates a minimal changelog stub rather than failing the workflow
3. **Branch existence check:** Added explicit remote branch check before creating release branch to fail fast with clear error

## Verification Results

- Both workflow files are valid YAML (verified with Python yaml.safe_load)
- prepare-release.yml has workflow_dispatch with version/bump/channel/base_branch inputs
- tag-on-merge.yml has pull_request closed trigger with correct job-level if condition
- Both use REPO_BOT_TOKEN (not GITHUB_TOKEN) for cross-workflow triggering
- prepare-release.yml calls bump-version.js with --to flag
- tag-on-merge.yml creates annotated tag matching release.yml's v* pattern
- Existing release.yml is not modified (0 lines diff)

## Self-Check: PASSED

- [x] `.github/workflows/prepare-release.yml` exists
- [x] `.github/workflows/tag-on-merge.yml` exists
- [x] Commit 7634e779 exists
- [x] Commit 43581bc7 exists
