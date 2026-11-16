# Issue 2252: v1 tag and color migration

## Current implementation contract

CLI migration and Desktop setup share `internal/v1migration.Source.Run`.
Every real migration automatically fills safe missing tags and colors.
There is no repair flag or opt-in option. Dry runs never change the target.
The source remains read-only.

Custom definitions and assignments use the authenticated v1 payloads. Built-in
membership comes from the separate profile-scoped search table. Map link,
image, directory and code to canonical v2 IDs. Never identify built-ins from
localized custom display names. Favorites remain unsupported and are reported.

For mapped records, union missing source tags only at revision 1 through the
public compare-and-swap API. Preserve content, existing tags, expiry, edits and
deletions. A completed update is safe to repeat after interruption. Edited
records needing tags remain partial and require manual review.

Read `history-tags/layout.v1.json` with a 1 MiB bound. Accept version 1, palette
colors and six-digit hex values. Resolve custom IDs through authenticated
names. Ignore stale IDs. Reject ambiguous duplicate-name colors. Missing layouts
use v1 defaults: Link blue, Code purple, Image green and Folder gray.

Write missing colors through the existing host configuration CAS while holding
the offline target lock. Existing target colors, including explicit gray,
always win. Preserve unrelated settings. Reopen configuration to verify writes.
Desktop projects saved overrides for built-in and custom tags. Setup results
show color failures and remaining missing-color counts.

## Failure inventory

- Missing definitions, corrupt assignments, authentication failures or malformed tag tables cannot silently become empty tags.
- Optional older-format search tables may be absent; no search index is rebuilt.
- Another profile's membership and a custom Links display name cannot become canonical Link membership.
- Unsupported favorites, invalid layouts, unknown versions, invalid colors and oversized files cannot report complete success.
- Invalid target config and CAS conflicts cannot overwrite existing settings.
- Missing target config must be created by ordinary migration when colors need completion.
- Existing colors, target edits, deletions and unrelated history must survive every rerun.
- Partial imports and cancellation must retain committed records and avoid duplicates.
- CLI and Desktop must share default behavior without platform-specific opt-in switches.
- Restart must preserve associations and colors through public history/config APIs.
- Artifacts must contain synthetic data only; retain receipts, not private profiles or binaries.

## Repeatable acceptance

```sh
TMPDIR=/private/tmp UNIC_MIGRATION_TAGS_E2E=1 \
UNIC_MIGRATION_TAGS_ARTIFACTS=/private/tmp/unic-migration-tags \
go test ./e2e/headless -run '^TestProductionMigrationTags$' -count=1 -v

make desktop-e2e CASE=first-install-setup
make desktop-e2e CASE=data-binding
make desktop-e2e CASE=tag-manager
```

The production CLI scenario builds actual CLI/service binaries with race
instrumentation. It imports frozen synthetic Rust payloads, reopens through
public Engine APIs, and queries real history, search and configuration APIs
after restart. It verifies custom assignments, all supported built-ins,
unassigned definitions, name collisions, original colors, target overrides,
automatic completion of old imports and idempotency. Failure variants exercise
unsupported favorites, damaged assignments, orphaned definitions, malformed
layouts, invalid target config and missing-layout defaults.

The canonical Desktop setup scenario seeds an untouched legacy import without
tags or colors. It calls the normal `setupflow.Run` path. No extra option is
passed. It verifies automatic tag completion during cancellation/resume,
original colors through the actual service, repaired tag filters, restart
persistence, source checksums and exclusive service ownership.

The data-binding scenario verifies saved built-in colors through the production
Desktop model, reconciliation and model reopen. Later settings-menu checks use
the software tester, wait for an idle frame and use Reduce Motion to keep the
sheet stationary. Dedicated scenarios cover animation. The tag-manager software
scenario includes the sixth built-in Code tag in expected counts.

The Production headless E2E CI job explicitly runs CLI migration acceptance and
uploads its receipt. The full Desktop job includes the setup process scenario.

## Results and evidence

The baseline `bd89ff5a12885079e4259c3a72c90ab5e8b420ca` retained custom tags in the
fixture but omitted built-in membership. Later acceptance reproduced missing
color configuration. This does not establish the cause of missing custom tags
in a personal profile; no personal data was read.

The final default-behavior CLI and Desktop setup scenarios passed. All 216
related migration, CLI, configuration and Engine race tests passed. Pinned
golangci-lint reported zero issues. Formatting, vet, documentation and checksum
checks also passed before publication. The prior color projection and
tag-manager scenarios passed; their implementation is unchanged by default
completion. Native-window, OS menu and system clipboard behavior remain outside
these process/software scenarios.

The [migration receipt](evidence/2252-v1-migration-tags/receipt.json) binds source
and binary identities, fixture hashes and expected/actual tag/color mappings.
The [color binding receipt](evidence/2252-v1-migration-tags/desktop-color-binding.json)
records the earlier successful production-model color projection with software
rendering. Its source hashes identify that earlier run. The [Desktop setup receipt](evidence/2252-v1-migration-tags/desktop-default-migration.json)
records the current shared default migration path. Retained receipts are
covered by the [checksum manifest](evidence/2252-v1-migration-tags/SHA256SUMS):

```sh
cd docs/exec/evidence/2252-v1-migration-tags
shasum -a 256 -c SHA256SUMS
```

Owned temporary profiles, source snapshots, binaries, logs and software images
are removed after acceptance. Personal-profile and native visual acceptance
remain NOT_RUN.
