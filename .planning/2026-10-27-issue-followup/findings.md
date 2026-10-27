# Findings

- The live GitHub inventory has exactly the same 171 issue IDs as the merged classification report. Reuse its per-issue classifications and initial analyses; no issue was skipped.
- #2110: VISION requires a retained tray host and daemon in lightweight mode. `enterLightweightMode` stops the UI child only. The currently displayed lightweight summaries omit the retained tray in every locale. The unused legacy descriptions incorrectly promise tray removal. Correct both strings consistently.
- #1365 and #1378 were addressed by merged PR #2124; do not duplicate those documentation fixes. #2112 was addressed in source by PR #2113; container-image/runtime acceptance remains separate.
- #1616 remains Medium: coldLaunch calls enterLightweightMode and therefore sends a notification. A fix needs separate automatic/manual entry semantics and native-host acceptance. This PR does not claim to resolve it.
- #1830 remains Medium: get exports raw content or a path while search owns detailed result rendering. Define detailed get output and stream compatibility, then validate text/image/file and list modes against a real daemon.
- All High issues remain initial analyses. A High classification indicates architecture or ownership risk, not proof that every report requires an architecture change.
- docs-site already describes the retained tray. No docs-site edits, telemetry additions or Rust tracing changes are required.
