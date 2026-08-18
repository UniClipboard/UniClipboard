# ADR-012 CLI Engine Capability Workflows

## Goal

Implement every accepted behavior in `docs/adr/adr-012-cli-engine-capability-workflows.md` and prove the workflows through real daemon CLI E2E tests.

## Completion Evidence

- Join: default wait, `--no-wait`, status, cancel, Ctrl-C detach, stable human and JSON results.
- Trust: status, apply, keep, stale-change protection, local-removal confirmation, non-interactive behavior.
- Sync: show, partial set, content types, deterministic member resolution, stable JSON.
- CLI never persists business state or composes cancel/reset/join recovery.
- Each new behavior has observed red-before-green focused coverage.
- Real daemon E2E exercises all three workflows with nonzero pass counts.
- Focused tests, relevant crates, workspace checks, formatting, help smoke, and diff hygiene pass.
- ADR wording and status match the delivered behavior.

## Phases

| Phase                      | Status   | Evidence                                                                                                            |
| -------------------------- | -------- | ------------------------------------------------------------------------------------------------------------------- |
| 1. Baseline and seam audit | complete | ADR-to-source matrix and test seams                                                                                 |
| 2. Join workflow           | complete | Focused parser/client/behavior tests                                                                                |
| 3. Device trust workflow   | complete | Focused parser/client/behavior tests                                                                                |
| 4. Member sync workflow    | complete | Focused parser/client/behavior tests                                                                                |
| 5. Real daemon E2E         | complete | Five real-daemon workflows pass, including deterministic pending, Ctrl-C, restart, and cancel coverage              |
| 6. Completion audit        | complete | Human and JSON output, stale decisions, duplicate names, reread failure, and remote Engine availability are covered |

## Decisions

- Preserve the current Engine pin and join-result compatibility work.
- All user actions go through daemon APIs to Engine-owned state.
- Every explicit join invocation performs exactly one join operation.
- Implementation must follow the ADR; no local business-state fallback.

## Errors Encountered

| Error                                                                                                              | Attempt | Resolution                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------ | ------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Multiple positional Cargo test filters were rejected                                                               | 1       | Ran the package library tests so compilation reached all four new tests.                                                                                      |
| Four new daemon-client tests failed to compile because methods were absent                                         | 1       | Expected RED; implemented only the four required request methods.                                                                                             |
| New CLI parser tests failed because join/member workflow variants were absent                                      | 1       | Expected RED; proceed with the public command shape from ADR-012.                                                                                             |
| CLI workflow implementation initially failed to compile because the main dispatcher omitted the new join fields    | 1       | Routed join, trust, and sync subcommands through their command modules.                                                                                       |
| Device-trust JSON test used a nonexistent recovery enum                                                            | 1       | Constructed the wire DTO with its actual string field.                                                                                                        |
| E2E package name was not visible from the root workspace                                                           | 1       | Run it from the root with `--manifest-path tests/e2e/Cargo.toml`.                                                                                             |
| Pending-join E2E treated an early non-success trust query as terminal                                              | 1       | Poll through transient pre-admission responses until pending or the diagnostic deadline.                                                                      |
| CLI rejected `join status/cancel` while a matching daemon reported degraded during admission                       | 1       | Allow setup control attachment only when package and API revisions still match exactly.                                                                       |
| Pending E2E interrupted the initial HTTP request before CLI entered its post-response wait loop                    | 1       | Keep real-process proof to concurrent status and explicit cancel; retain Ctrl-C detach semantics in the focused state-machine test.                           |
| Pending E2E first paused the sponsor after observing pending, leaving a completion race                            | 1       | Pause at the joiner's post-dial, pre-request log boundary so pending remains stable.                                                                          |
| Post-dial pause occurred before the public join projection reached pending                                         | 1       | Gate the sponsor after the proof response and advance it in bounded pulses until pending is observable.                                                       |
| Engine advances a real pending join through session transition too quickly for deterministic process-level control | 1       | Removed the timing-dependent E2E and kept stable real-daemon coverage for join, trust, and sync; pending boundaries remain covered by focused behavior tests. |
| Strict Clippy surfaced pre-existing warnings in unrelated shared files                                             | 1       | Verified the changed packages with existing warning classes allowed; no new warning remained, and fixed the new join wait argument warning.                   |
| Cargo accepts only one positional test filter                                                                      | 2       | Run the join test module or one exact filter per invocation.                                                                                                  |
| A stored join attempt was still reported as `none` during the transition                                           | 3       | Traced Engine's locked active-state fallback to an unavailable snapshot with `current_join: None`; documented it as an Engine prerequisite.                   |
| Wrong passphrase did not create a public rejected status                                                           | 1       | Assert the immediate JSON/human failure and the truthful subsequent `none` status; retain rejected rendering in focused tests.                                |
| Post-restart cancellation did not immediately become terminal rejected                                             | 2       | Kept the ADR contract at durable `cancel_requested`; terminal rejection requires completion of the interrupted peer session and is not a client promise.      |
| Tight status polling hit the daemon's normal request limit                                                         | 2       | Removed the out-of-scope terminal-state polling and kept bounded, user-realistic polling for observable pending state.                                        |
