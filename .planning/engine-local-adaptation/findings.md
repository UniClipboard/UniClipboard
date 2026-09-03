# Findings

## Dependency patch

- Desktop pins both `uc-engine` and `uc-observability-contract` to Engine revision `31c149c5bfb8a8edfe80c94944c8255157a3a3af` in the root workspace dependencies.
- Direct desktop consumers use the workspace entries; the override belongs at the root so every member resolves one source.
- The existing crates.io patch for `iroh-blobs` is unrelated and must be preserved.
- The repository explicitly forbids committing local Engine paths. The machine-level Cargo config is the correct scope for this requested development override.
- Patching the two direct public packages is sufficient: local `uc-engine` resolves its internal path dependencies from the same local Engine workspace.
- Resolution confirms eight Engine-owned packages are sourced from `/Users/mark/MyProjects/uni/Engine`; the newer Engine also introduces four compatible third-party dependencies into the desktop lock.

## Engine changes

- Local `../engine` is clean at `229edc7f` on `main`, matching `origin/main`.
- Recent history after the desktop pin includes substantial facade/runtime/assembly ownership changes and updated documentation.
- The pinned public packages are both version `1.1.0-rc.5`.
- Full desktop compilation reaches local `uc-engine` successfully. The first desktop boundary failures are six removed public items centered on device-group decisions and workspace convergence.
- Commit `a6b3281f` is the decisive public-contract cutover: it replaces the separate device-trust and membership-conflict operations with one device-group choices query and one device-group choice command.
- The query now returns one revision, the complete device-trust snapshot, and every pending issue with opaque choices. The command must echo the selected issue ID, choice ID, and query revision.
- The command result no longer carries a replacement snapshot. It returns one of completed, pending, re-pairing required, already completed, state changed, or local-device confirmation required, plus an optional current revision.
- `RemoveMember` now returns a device-trust snapshot. Workspace convergence is an internal dev-tools diagnostic, not a formal product response.

## Desktop impact

- Main host boundaries likely affected are `uc-bootstrap`, daemon/webserver startup, and observability; exact call-site mapping remains pending.
- `uc-webserver` currently imports removed `DecideDeviceTrustChangeInput` and `DeviceTrustDecisionSummary` types.
- It calls removed `QueryDeviceTrust` and `DecideDeviceTrustChange` operations and matches removed `DeviceTrustDecision` result.
- Pairing API matches `WorkspaceConvergence`, which the current Engine interface documents as dev-tools-only rather than a formal product operation.
- `uc-bootstrap` passes its full all-target check unchanged, so host capability preparation and Engine startup wiring need no adaptation.
- Default workspace compilation has six `uc-webserver` errors. The CLI with all features has two additional errors in its dev-tools pairing wait helper.
- The old daemon client exposes `/member/workspace-convergence`, but the server has no matching route. The new Engine contract confirms this stale product path should be deleted rather than recreated.
- The frontend currently models one binary device-trust change. The new contract can return multiple pending issues and arbitrary candidate groups, so a compile-only rename would silently ignore valid choices.
- `RefreshRequired` is forwarded on the `system` WebSocket topic. The device-trust provider currently subscribes only to `device-trust`, so it must also refresh on the process-wide invalidation event.

## Adaptation plan

1. **Daemon contract and web API**
   - Replace the old query/decision DTOs with `DeviceGroupChoices`, issue, option, request, outcome, and result DTOs matching Engine fields exactly.
   - Query with `Operation::QueryDeviceGroupChoices`; return both the nested trust snapshot and the issue list.
   - Submit `Operation::ChooseDeviceGroup` with the opaque issue ID, opaque choice ID, query revision, and explicit local-removal confirmation.
   - Return the compact outcome, then require callers to re-query. Do not reconstruct or cache a snapshot in the daemon.
   - Change unpairing to consume and return `OperationResult::DeviceTrust`; remove formal workspace-convergence DTOs, projections, routes, constants, and OpenAPI entries.
   - Keep internal convergence diagnostics only behind the existing e2e/dev-tools feature.

2. **Daemon client and CLI**
   - Replace `device_trust`/`decide_device_trust` service methods with query/choose device-group methods and matching paths.
   - Make member removal render or return the resulting trust snapshot. Remove the stale `member removal-status` path or fold it into the device-group status command; do not expose internal convergence as product state.
   - Update the trust command to select an issue and one of that issue's returned choices, pass the exact revision, handle all six outcomes, and re-query before rendering current state.
   - For the dev-tools invitation wait helper only, replace the removed convergence query with `QueryMembershipDiagnostics` and read its revision.

3. **Frontend product flow**
   - Make the context own the complete device-group choices response as the single source of truth; derive the nested trust snapshot for existing device lists and setup/join observers.
   - Submit opaque returned IDs and revision. Re-query after every choice outcome and on both `device-trust.changed` and `system.refresh_required`.
   - Preserve the current binary removal dialog for the pending-change issue, but drive it from the matching returned issue/options rather than deriving IDs.
   - Add a general candidate-group view for branch-conflict issues and queue multiple issues deterministically. Show explicit states for pending, re-pairing required, stale revision, and local-device confirmation.

4. **Generated API and proof**
   - Regenerate OpenAPI and the TypeScript client after the Rust wire contract changes; do not hand-edit generated files.
   - Update projection, daemon-client, CLI, context, dialog, multi-issue, stale-revision, refresh-required, unpair, and generated-schema tests.
   - Run focused Rust tests, all-feature CLI checks, full workspace all-target checks, frontend type/build/tests, React diagnostics, and an actual device-group dialog flow in the app.

## Deletion checklist

- `QueryDeviceTrust`, `DecideDeviceTrustChangeInput`, and `DeviceTrustDecisionSummary` call paths.
- `WorkspaceConvergenceDto` as a formal daemon/client response and the unused `/member/workspace-convergence` client path.
- `member removal-status` if it has no distinct product behavior after the cutover.
- Production-facing workspace-convergence WebSocket constants and projections; retain only explicitly feature-gated internal diagnostics.
- Handwritten frontend decision-result snapshots and assumptions that there is only one binary issue.

## Errors

- `../engine/VISION.md` does not exist. Engine's root `AGENTS.md` points to `docs/design-docs/core-beliefs.md` and related focused documents instead; follow those sources.
- The desktop `target` symlink is valid in intent but its external destination had been removed during prior cache cleanup; the external volume is mounted with ample free space.

## Pairing feedback diagnosis

- The reported pairing did not spend more than one minute in the protocol. The original profile logs show about 6.6 seconds from the join request to the installed target session; the stale modal created the perceived delay.
- The sponsor committed the new member but Engine did not publish `DeviceTrustChanged`. The joiner published only `RefreshRequired`, while the join hook subscribed only to the device-trust topic.
- The sponsor modal also required `currentInvitation === null`. Current Engine keeps the issued invitation visible until cancellation or expiry, so that condition prevented success even after the new member was active.
- Engine now publishes a device-trust invalidation after every successful membership ledger commit and after the joiner installs its new session. Failed commits do not publish success.
- The frontend listens to both device-trust and system invalidations. Sponsor completion is based on a newly active device and clears the stale invitation. Joiner completion compares the active peers with a pre-join baseline when the completed join projection is no longer present.
- A real two-window test with fresh profiles completed both success screens in 12.9 seconds end to end. Logs show the sponsor recognized the member about 4.9 seconds after the request and the joiner installed the new session about 8.8 seconds after the request.
