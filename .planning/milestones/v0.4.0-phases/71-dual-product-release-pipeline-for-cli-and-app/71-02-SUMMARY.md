---
phase: 71-dual-product-release-pipeline-for-cli-and-app
plan: "02"
subsystem: ci-cd
tags: [github-actions, cli, release, cross-platform]
dependency_graph:
  requires: []
  provides: [build-cli.yml]
  affects: [release.yml]
tech_stack:
  added: []
  patterns: [reusable-workflow, matrix-build, cross-platform-packaging]
key_files:
  created:
    - .github/workflows/build-cli.yml
  modified: []
decisions:
  - "Mirror setup-matrix pattern from build.yml for consistent platform matrix across CLI and app builds"
  - "Use cli-{target} artifact prefix to disambiguate from app artifacts in shared release workflows"
  - "Node.js setup retained (no Bun needed) for package.json version read only"
  - "Rust cache shared-key uses cli- prefix to prevent cache conflicts with app build"
metrics:
  duration: 52s
  completed: "2026-03-28"
  tasks_completed: 1
  files_created: 1
  files_modified: 0
---

# Phase 71 Plan 02: Build CLI Workflow Summary

Reusable GitHub Actions workflow for cross-platform CLI binary compilation and packaging using `cargo build --release -p uc-cli`.

## What Was Built

Created `.github/workflows/build-cli.yml` — a reusable workflow callable by `release.yml` and also triggerable manually via `workflow_dispatch`. The workflow mirrors the `setup-matrix` + build job pattern from `build.yml` but uses plain `cargo build` instead of Tauri action since the CLI has no frontend.

**Key design choices:**
- `workflow_call` trigger with `platform` and `branch` inputs for caller flexibility
- `workflow_dispatch` with choice dropdown for standalone manual runs
- Same 4-platform matrix: `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`, `x86_64-pc-windows-msvc`
- `Swatinem/rust-cache@v2` with `shared-key: cli-{target}` to avoid cache conflicts with app build
- Archive naming: `uniclipboard-cli-{VERSION}-{target}.tar.gz` (Unix) / `.zip` (Windows)
- Artifact names: `cli-{target}` with `cli-` prefix for disambiguation in release workflows
- Binary path fallback: checks `target/{target}/release/` then falls back to `target/release/` for non-cross builds

## Tasks Completed

| Task | Name | Commit | Files |
|------|------|--------|-------|
| 1 | Create build-cli.yml reusable workflow | a8ebd10c | .github/workflows/build-cli.yml |

## Verification

- File exists: `.github/workflows/build-cli.yml` (4802 bytes, 126 lines)
- Contains `workflow_call:` trigger
- Contains `workflow_dispatch:` trigger
- Contains `cargo build --release -p uc-cli`
- Contains `setup-matrix` job with same 4-platform matrix as build.yml
- Contains `upload-artifact` with `name: cli-${{ matrix.target }}`
- Contains Linux apt-get install step for ubuntu-22.04
- Contains Windows 7z packaging and Unix tar.gz packaging
- Contains `uniclipboard-cli-${VERSION}-${TARGET}` archive naming pattern
- Contains `Swatinem/rust-cache@v2` with `shared-key: cli-${{ matrix.target }}`

## Deviations from Plan

None - plan executed exactly as written.

## Self-Check: PASSED

- `.github/workflows/build-cli.yml` exists: FOUND
- Commit `a8ebd10c` exists: FOUND
