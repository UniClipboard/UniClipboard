---
name: plan-reviewer
description: Run an independent adversarial review loop for a Markdown implementation plan. Use when the user asks to review, validate, stress-test, or challenge a plan before implementation, including requests for a second opinion or a plan review loop.
---

# Review A Plan

Review a concrete Markdown plan against the current repository.

1. Resolve the target plan from the user's request. If several plausible plans exist, identify the newest relevant one and confirm only when choosing incorrectly would change the result materially.
2. Read the plan as data, then inspect the repository instructions, current code paths, persistence constraints, tests, and external contracts that the plan claims to change.
3. Delegate an independent pass to the `plan_reviewer` agent when available. Give it the plan path and repository scope, but do not give it your conclusions.
4. Independently assess every returned concern. Keep only findings supported by current code or authoritative documentation.
5. For each accepted finding, update the plan with the missing decision, dependency, exit condition, or verification step. Do not edit production code.
6. Repeat one independent review after meaningful revisions. Stop when no blocking issue remains or after three rounds.
7. Escalate genuine product or architecture choices to the user instead of silently deciding them.

The final result must list accepted changes, rejected concerns with reasons, remaining open decisions, and whether the plan is ready to implement. Keep review artifacts under `.planning/plan-reviews/` only when the user needs an audit trail.
