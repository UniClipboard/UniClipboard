---
phase: 71
slug: dual-product-release-pipeline-for-cli-and-app
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-28
---

# Phase 71 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                                                              |
| ---------------------- | -------------------------------------------------------------------------------------------------- |
| **Framework**          | GitHub Actions (CI/CD pipeline validation) + Node.js scripts (unit tests for bump/release scripts) |
| **Config file**        | `.github/workflows/*.yml`                                                                          |
| **Quick run command**  | `node scripts/bump-version.js --dry-run --to 0.0.1-test.1`                                         |
| **Full suite command** | `cd src-tauri && cargo check -p uc-cli --all-targets`                                              |
| **Estimated runtime**  | ~30 seconds                                                                                        |

---

## Sampling Rate

- **After every task commit:** Run `node scripts/bump-version.js --dry-run --to 0.0.1-test.1`
- **After every plan wave:** Run `cd src-tauri && cargo check -p uc-cli --all-targets`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement       | Test Type   | Automated Command                                             | File Exists | Status     |
| -------- | ---- | ---- | ----------------- | ----------- | ------------------------------------------------------------- | ----------- | ---------- |
| 71-01-01 | 01   | 1    | Version sync      | unit        | `grep 'version.workspace' src-tauri/crates/uc-cli/Cargo.toml` | ✅          | ⬜ pending |
| 71-01-02 | 01   | 1    | CI build          | integration | `act -n -W .github/workflows/build-cli.yml` (dry-run)         | ❌ W0       | ⬜ pending |
| 71-02-01 | 02   | 2    | Release artifacts | integration | `grep 'cli' .github/workflows/release.yml`                    | ❌ W0       | ⬜ pending |
| 71-02-02 | 02   | 2    | Release notes     | unit        | `node scripts/generate-release-notes.js --help`               | ✅          | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] Existing infrastructure covers all phase requirements — no new test framework needed
- [ ] Validation is primarily structural (file existence, content patterns in YAML/JS)

_Existing infrastructure covers all phase requirements._

---

## Manual-Only Verifications

| Behavior                           | Requirement       | Why Manual                         | Test Instructions                                            |
| ---------------------------------- | ----------------- | ---------------------------------- | ------------------------------------------------------------ |
| CLI binary runs on target platform | Cross-platform    | Requires actual platform execution | Build CLI, run `uniclipboard-cli --version` on each platform |
| GitHub Release shows both products | Release integrity | Requires actual GitHub Release     | Trigger alpha build, verify artifacts in Release page        |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
