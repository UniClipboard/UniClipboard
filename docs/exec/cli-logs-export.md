# CLI local log export

## Contract and failure inventory

The CLI exports retained local logs without service IPC. Interface, archive layout,
private creation, source bounds and live-snapshot limits are specified in section 12
of the [observability contract](../contracts/observability-contract.md#12-offline-local-log-export).

The scenarios cover offline export, default output, membership/privacy/checksums,
missing/empty logs, matching directories/symlinks, symlinked logs root, individual and
combined size limits, managed-directory output, absent output parent, existing files
and symlinks, invalid arguments, concurrent append and truncation after output creation.
The initial process scenarios were written before the exporter. The truncation fault
scenario was added during review. No unit tests are added.

Rotation can skip an unopened missing segment. Appends after selected lengths are not
included. Export does not flush service queues or promise an atomic cross-file view.
Filesystem cancellation/permissions/write failures return errors and clean up the
new archive where its identity still matches. Abrupt process termination cannot run
cleanup; no crash-atomic publication guarantee is made.

## Replay

```sh
make logs-export-e2e
make headless-e2e
go test -run '^$' ./...
go run ./tools/docscheck
```

`logs-export-e2e` builds the production CLI with race detection, then drives real
subprocesses with an absent service socket. It fails on a missing binary, test failure,
skipped parent or zero selected cases. The retained artifact is
`.cache/logs-export/acceptance/logs-export/{manifest.json,replay.sh,SHA256SUMS}`.
It records source/binary hashes, fixture archive hashes, scenario outcomes and replay.
No profile, credentials, source log content, binary or UI screenshot is retained.

## Receipt

- Initial preimplementation run: expected failure on positive export cases because
  the command did not exist. Its negative cases are not acceptance evidence.
- Targeted production CLI E2E: all 15 scenarios passed (6.748 seconds with runner).
- Full CLI script suite, log-export suite and existing log-level/observability
  process regression: passed (265.466 seconds). The log-export rerun passed all
  15 scenarios against unchanged source.
- Production headless process/dependency acceptance: passed (48.448 seconds).
  The standalone adapter subprocess entry skips unless invoked by its parent fixture.
- All-package compilation: `go test -run '^$' ./...` passed. No unit tests ran.
- Scoped diff review and `git diff --check`: passed. No dependency was added.
- No native GUI/clipboard operation, network upload, release or installation was performed.

Artifacts retain only manifests, checksums and replay instructions. The production
headless manifest is retained separately at `.cache/logs-export/headless/manifest.json`.
The export is limited to already-written bytes. Rotation/skipping is best effort;
no atomic multi-file snapshot, queue flush or abrupt-kill cleanup is claimed.
