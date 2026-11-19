# Migration and first-run failure inventory

Written before implementation and process acceptance additions.

## CLI
- A fresh profile starts a service or accesses sync/clipboard before explicit migration completion.
- A noninteractive or JSON invocation silently proceeds or emits an unactionable error.
- Help/version/migration instructions are blocked by initialization.
- Existing v2 database presence silently grants permission.
- Missing v1 data leaves a new user permanently blocked.
- No-source initialization ignores an existing v1 source, a pending import, invalid options or a running target owner.
- Dry-run, authentication failure, cancellation, partial import, corrupt state or failed state publication marks completion.
- Target resolution differs between normal commands and migration (development/portable/UNIC_HOME).
- Concurrent imports or initialization overwrite pending state; service acquisition races a failed import.
- Explicit custom target completes a different profile.
- Repeated import duplicates records; restarting loses completion or imported data.

## Desktop and importer
Desktop and importer workers record their detailed failure inventories before their respective E2E changes. Acceptance must include pending Settings migration, directory selection/cancel, service ownership, credential prompts, bounded large imports, real ENOSPC, interruption/resume, partial repair and reopen integrity. Cross-compilation is not native acceptance.

## Acceptance
Use real CLI/service processes with synthetic encrypted fixtures. Retain a redacted receipt, source/binary hashes, replay command, cleanup assertion and SHA256SUMS. Do not fill the host disk. Linux process acceptance uses a bounded isolated container when available. Native system credentials use only task-created items.
