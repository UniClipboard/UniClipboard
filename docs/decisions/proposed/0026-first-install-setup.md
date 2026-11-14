# 0026: Desktop first-install setup and v1 migration

- Status: Implemented; nonvisual acceptance is recorded in the [execution receipt](../../exec/0026-first-install-setup.md). Software-view and macOS native complete/partial recording acceptance passed. Actual system credential prompts and remaining native scenarios/platforms are pending.
- Date: 2026-11-13
- Scope: Desktop first-install experience for v2.
- Related decision: [v1 history migration](0011-v1-history-migration.md).
- Current behavior: [Live desktop composition](../../architecture/overview.md#live-desktop-composition).
- Landing targets: [Desktop runtime](../../architecture/desktop-runtime.md) and [live desktop composition](../../architecture/overview.md#live-desktop-composition).

## Context

The current desktop opens history directly. The existing `unic migrate` command can discover a default v1 profile, read supported history, import it offline, and verify the imported content. Desktop has no graphical migration flow.

First-time users need a welcome page and an explicit choice about importing v1 data. Sync is one product capability, so the welcome message must describe broader clipboard value.

This decision introduces a first-install exception to direct entry. Subsequent launches continue to open history directly after setup completion. It does not restore the former setup, space creation, or recurring unlock flows.

## Decision

Use three logical pages: welcome, migration, and migration result. Migration confirmation and progress share the migration page. Use the existing mygo UI Kit. Use the user-approved [setup prototype](../../../prototype/setup.html), with the established [GUI style](../../../prototype/gui.html).

### 1. Welcome

Use the following exact Simplified Chinese product copy:

| Element | Copy |
|---|---|
| Heading | 欢迎使用 UniClipboard |
| Slogan | 让每一次复制更有用 |
| Primary action | 开始使用 |

Detect the default local v1 data directory in the background. Detection must not read clipboard content, request system credentials, or begin migration.

After the primary action, show migration confirmation if a v1 source is detected. Otherwise, enter history without an extra completion page. A detection error must remain distinguishable from an absent source. Allow retry or entry without migration.

### 2. Migration

Explain what will be imported before requesting credentials or writing target data. Import supported text, HTML, images, files, and tag definitions and associations. Do not promise import of settings, favorites, device identities, or trust. Explain that devices must upgrade to v2 and pair again.

State that migration preserves the v1 source. Ask the user to quit v1 before starting. Do not quit v1 automatically. Allow selection of another source directory for a custom or portable profile.

Offer `开始迁移` as the primary action and `暂不迁移` as the secondary action. Skipping enters history and completes setup. Keep a later migration entry under Settings > Storage > Migrate from v1.

After explicit consent:

1. Read system credentials through the existing platform integration.
2. If credentials cannot unlock the source, request the old passphrase in the current flow.
3. Import using the existing migration implementation.
4. Reopen the target and verify imported content.
5. Show the actual terminal result.

Keep passphrases out of command arguments, environment variables, persistence, logs, and reports. System credential prompts occur only after the user starts migration. Reject unsupported or invalid sources without creating misleading success results.

Display the current phase and processed/total record counts. Distinguish importing from verification. Do not invent a remaining-time estimate or show import completion as verification completion.

Offer `停止迁移` while work is running. Wait for safe cancellation before presenting a terminal result. Preserve committed records. Closing or quitting during migration must use the same safe cancellation path and release migration resources before process exit.

### 3. Migration result

| Outcome | Presentation | Actions |
|---|---|---|
| Complete | Migration completed; show imported and verified record counts. | Start using the app. |
| Partial | Some data could not be imported; show imported records, failed records, missing content parts, and verification failures separately when nonzero. | Enter the app or retry migration. |
| Cancelled | Migration stopped; committed records remain available. | Resume migration or enter the app. |
| Failed | Migration did not complete; show a safe, actionable reason. Do not imply committed records were rolled back. | Retry or enter without further migration. |

Use `迁移完成` only for a complete, verified result. A missing content-part count is not a failed-record count. A record may retain usable content while some attachments are missing. On retries, distinguish newly imported records from records already imported; do not present a zero-new-record retry as an empty history.

Entering the app opens history directly. Do not add another completion dialog or automatically apply imported content to the system clipboard.

## Setup state and startup ordering

Scope setup state to the current v2 data directory. Persist completion only when the user proceeds into history, including the explicit skip path. Closing an unfinished flow must not silently mark setup complete.

After an interruption, restore the appropriate unfinished flow. Do not automatically resume data import without a user action. Reuse the migration implementation's persistent import mappings to skip successful records and retry remaining work. Do not build a second record-level migration ledger.

Existing initialized v2 installations must continue to open history directly. Absence of a newly introduced setup marker alone must not classify an existing installation as a fresh install. Use a pre-existing regular, nonempty `unic.db` when no setup marker exists. A pending setup marker takes precedence. Do not use history count, because an established installation can have empty history.

Migration requires exclusive target ownership. Resolve setup before the normal desktop path starts its service and clipboard adapter. Do not capture clipboard content or start sync during migration. If a service already owns the target, report the conflict and require it to stop before retrying. Do not terminate an independently running service automatically.

The later Settings migration entry must respect the same exclusive ownership rule. Stop only desktop-owned activity through its normal lifecycle before migration, then restart it after releasing migration ownership. A failure to release or acquire ownership must leave migration blocked with an actionable message.

## Boundaries and alternatives

- Do not add account registration, device pairing, generic permission setup, or a recurring v2 passphrase gate to first-install setup.
- Do not migrate automatically after detecting a source. Detection does not authorize credential access or import.
- Do not force migration. Users can start with an empty v2 history and import later.
- Reuse the existing migration implementation and report semantics. Do not create another importer or a standalone migration program.
- Maintain delivered behavior in the linked architecture pages and acceptance limits in the execution receipt.

## Implementation acceptance

Implementation must use the canonical [Desktop E2E entry and evidence contract](../../contracts/desktop-e2e-contract.md). Cover these user-visible paths with deterministic, non-personal fixtures:

1. Fresh installation without v1 data enters history after welcome.
2. Detected v1 data offers import and skip; skip persists setup completion.
3. Complete import verifies content and exposes it in history.
4. Partial import reports failed records and missing parts separately.
5. Credential denial or a wrong passphrase permits retry and skip.
6. Cancellation and process restart retain committed data without duplicate records.
7. Invalid sources and target ownership conflicts give actionable failures.
8. Completed setup and existing initialized v2 installations bypass setup on later launches.
9. The Settings entry permits a later import under exclusive target ownership.

Retain a repeatable E2E receipt with source identity, fixture identity, assertions, and the evidence required by the Desktop E2E contract. Native visual acceptance must follow the supplied prototype and requires a separately authorized run. Documentation-only work does not establish these acceptance results.
