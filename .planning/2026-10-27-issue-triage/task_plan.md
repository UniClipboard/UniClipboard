# Issue triage and documentation corrections

## Goal
Classify every open repository issue, repair independently verifiable low-complexity defects, and publish a reviewable PR.

## Scope
- Snapshot: 171 open issues; 87 created before 2026-08-01 and 84 on or after it.
- Base: 20c5ddcaf52e3504964b348908d34d3ccf09307e.
- No product architecture changes, issue comments, labels, closures, releases, or interactive acceptance.

## Phases
1. Discovery: complete.
2. Classify all issues: complete (4 Low / 105 Medium / 62 High).
3. Correct #1365 and clarify #1378: complete.
4. Verification: complete. Mirror/anchors, configuration, types and Webpack production build PASS. Default Turbopack build FAIL; no all-green claim.
5. Independent CONTEXT audit: NO_UPDATE. Documentation commit pushed. Inventory publication follows in a separate commit; PR URL and publication state belong to the GitHub record and conversation.

## Errors
- Initial branch rename: Git metadata outside writable workspace; succeeded through approved escalation.
- gh authentication: invalid local token; use connected GitHub account mkdir700, matching the local account.
- Bun install: sandbox temporary-directory write denied; approved installation succeeded without lockfile changes.
- Exploratory body summary encountered a null issue body; normalized missing bodies to empty strings.
