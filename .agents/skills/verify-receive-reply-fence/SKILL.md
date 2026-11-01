---
name: verify-receive-reply-fence
description: Verify the receive-detail request lifecycle when changing useEntryReceive refresh, retire, retry, reconnect, cleanup, or either awaited response fence. Check that retired responses cannot publish snapshots or metadata.
---

# Verify receive reply fencing

Property: after a request is retired, its response must not publish a receive snapshot or metadata.
Harm: a late response can overwrite the current file progress after a detail remount or daemon reconnect.

This skill checks one lifecycle safety property. It does not prove the entire receive flow.

## Run

From the repository root, install locked dependencies with `bun install --frozen-lockfile` if needed. Install the browser using the existing GUI E2E setup in `docs/testing/tester-army-migration.md`. Java 11 or newer, Python 3, npm, and network access for the first pinned-tool download are required.

```bash
npx --yes --package=node@24.22.2 --call 'python3 .agents/skills/verify-receive-reply-fence/verify.py'
```

The command verifies the checksum-pinned TLC 1.7.4 distribution (TLC reports version 2.19), checks the model, requires the missing-fence model to violate `NoRetiredCommit`, and runs two real React detail flows through the unified TesterArmy GUI entrypoint. No product source is changed by this command. Model checking is a finite-state proof; GUI replay is an implementation regression check, not a proof of every implementation execution.

Read **every TLA+ result with `/statechart`**, including the deliberately broken model. Locate the installed `statechart` skill and follow its current instructions. Copy model inputs to a scratch directory outside the repo before using that skill. Do not substitute raw checker success for its counterexample/statechart report. The model's `view.mjs` labels states and actions in the product's terms.

Artifacts: `target/formal-receive-reply/` contains the model inputs, checker logs and `receipt.json`; `target/tester-army-layout-report/macos/receive-generation/` contains the GUI report; `target/tester-army-layout/macos/receive-generation/` contains screenshots and assertion JSON. A missing or failed receipt is not acceptance. Reruns replace these outputs; preserve a failed run before rerunning.

## Coverage and synchronization

Last code/model synchronization commit: `95a398f6085cc595efd57858ca23759282dd94ce`.
Covered hook SHA-256: `82a77326ca7b9e2b091f327d1f272972b7f378de03f7d99da025f2f42e557ba7`.

| Model action | Real code in `apps/gui-go/frontend/src/hooks/useEntryReceive.ts` |
| --- | --- |
| `StartRead` | `refresh`: capture `const ticket = generation` before `await getEntryReceive` |
| `ReceiveReply` | First `cancelled || ticket !== generation` check; then direct snapshot publication or the metadata branch |
| `DetailReply` | `await getEntryDetail`; the code then starts the confirmation read without another fence |
| `ConfirmReply` | Second `cancelled || ticket !== generation` check after `await getEntryReceive`; metadata and snapshot publication then run synchronously |
| `Retire` | `retire` increments generation; used by reconnect and retry |
| `Unmount` | Effect cleanup sets `cancelled` and increments generation |
| `Done` | An idle effect has no more bounded reads; stuttering represents no external event |

The synchronous continuation from each fence to the next await or final dispatch is one JavaScript task. Do not add an await inside that span without splitting the model action and checking the new boundary. Publication marks a violation at that instant; retiring an already published snapshot is not itself a violation.

The GUI replay lives in `apps/gui-go/frontend/e2e/tester-army/receive-generation.e2e.mjs`. It drives the production detail, hook and Redux store through `file-preview` HTTP fixtures. For each await boundary it holds an old 75% response, switches to another entry, remounts the original detail, observes a fresh 25% response, then releases the old response and requires 25% to remain. It omits optional snapshot versions to exercise lifecycle protection without relying on revision rejection. This is a supported compatibility case, not a mutation of the reducer.

Inspect changes since the synchronization commit in the hook and in `receive-file-progress.ts`/`fileTransferSlice.ts` when they affect publication. A source hash mismatch requires a new code/model review; it is not automatically a defect. Move the synchronization commit and hash forward only after confirming each action and atomic boundary still describes the new code, rerunning this command, and validating the mutation checks below. Keep production ownership in the hook; do not add production flags solely for verification.

## Bounds and assumptions

- One effect, at most two serialized read cycles, generations 0 through 2, at most two retirement events, and an optional unmount.
- Both reply boundaries and retirement orders are nondeterministic. Metadata success and the direct publication branch are included.
- JavaScript executes each synchronous continuation without event-handler interleaving. API promises resolve at the modeled await boundaries.
- The model deliberately permits publication whenever the lifecycle fence permits it. Other rejection checks can only remove those modeled publications; they are not proved here.
- Excluded: liveness/eventual refresh, errors, retry-command completion, multiple hook instances, Engine correctness, revision ordering, file/name association, encryption, network transport and native platform behavior. GUI replay covers remounting two actual effect instances, but does not exhaust all their interleavings.

## Prove the check can fail

Do this only in an isolated scratch change. Preserve unrelated work. Never commit a mutation.

1. Run the command unchanged and preserve its passing receipt.
2. In the real hook, remove only the first `if (cancelled || ticket !== generation) continue` statement. Run the same command. The `receive` GUI replay must fail because the old 75% snapshot overwrites 25%.
3. Restore the original file. Remove only the second identical statement. Run the same command. The `confirm` GUI replay must fail for the same reason after metadata confirmation.
4. Restore the original file and verify its original hash. Rerun the command. Both GUI replays and the current model must pass. The broken model must still fail.

The model is independent of the hook source. Its pass alone cannot detect removal of a real-code guard. The GUI replay is mandatory in the one-command check specifically to catch that gap.

If the current model fails, use `/statechart` to read the shortest path. Replay that path against the real hook using the unified E2E fixture. A model/code mismatch is a modeling error until evidence shows the real defect. Keep a genuine finding visible; do not weaken the property and do not modify product code without user authorization.

Use `/maintain-formal-verification-skill` after changes that did not recheck this coverage. If that skill is unavailable, perform the synchronization review and rerun procedure above directly.
