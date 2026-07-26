---
name: review-strict
description: Perform a strict, evidence-based review of the current branch or working-tree changes without modifying files. Use when the user asks for a strict review, deep review, senior review, merge assessment, or explicitly invokes `$review-strict`.
---

# Strict Code Review

Review the current diff first. Inspect unchanged code only when it is necessary to establish the behavior of a changed path.

Prioritize:

1. Correctness, regressions, state transitions, edge cases, concurrency, cleanup, compatibility, and performance.
2. Security and privacy, especially plaintext persistence, sensitive logs, excessive permissions, clipboard data exposure, authentication reuse, and raw paths.
3. Repository architecture, ownership, existing reusable modules, hard-coded values, dead code, and parallel old/new logic.
4. Cross-platform behavior on macOS, Windows, Linux, iOS, Android, and HarmonyOS where the changed path applies.
5. Missing or ineffective tests, including duplicated hand-written mocks where an established test helper exists.

For every finding:

- Cite the file and exact line.
- State the concrete trigger as input or state -> incorrect behavior.
- Explain why it matters and the smallest sound correction.
- Try to disprove the finding before retaining it.

Order findings by severity: blocking, important, optional. Do not report style preferences or speculative issues. If no real issue remains, say so clearly and note any residual verification gap.

End with one merge assessment: ready to merge, merge after fixes, redesign required, or insufficient evidence. Do not edit, commit, or push.
