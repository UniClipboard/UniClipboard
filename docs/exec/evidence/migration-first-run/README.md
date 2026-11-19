# Migration and first-run receipt

Production source: `9d6c3fb56e31b0b29ec33301c073f66722ebb753`.
Task branch: `feat/migration-first-run`. The parent session worktree was untouched.

Verify the retained files:

```sh
cd docs/exec/evidence/migration-first-run
shasum -a 256 -c SHA256SUMS
```

`source.json` records the baseline, exact source commit, changed-file hashes and
diff hash. `programs.json` records actual production CLI/service hashes for
macOS, Linux and Windows, arm64 and amd64. Compilation does not prove native
execution. Only macOS arm64 and Linux arm64 process runs were performed.

The two first-run receipts verify actionable refusal before service creation,
explicit no-source initialization, empty leftovers, preserved help/version,
dry-run/partial refusal, repair, idempotence, exact target scope, concurrent
initialization, SIGINT during lock/stdin waits, stable source identity, and two
real service restarts. Separate private PTY runs verified restored terminal echo.
Both ran the production binaries recorded in `programs.json`.

The Linux resilience receipt covers 10,000 small encrypted-text records, peak
child RSS 81,948,672 bytes, a dedicated 32 MiB tmpfs with actual ENOSPC, verified
recovery, retained unrelated history, source immutability, and stable IDs across
two service starts. The predeclared RSS bound is 512 MiB. It does not establish
multi-GiB blob performance. Historical fixtures explicitly disable normal age
cleanup. Production user cleanup policy is unchanged.

The credential receipt uses real GNOME Secret Service in a private HOME and
D-Bus session. Container teardown removes its task-only credential items.
Desktop results summarize canonical process and native-window cases. The native
snapshot's relevant production bodies match committed source; the shared receipt
file differs only in two comments. The canonical runner's actual file hashes
and binary identities remain recorded. Cancellation and persisted-history
screens were inspected. Native UI exact-count assertions use production IPC;
the displayed history remains paged.

Repeat with private synthetic profiles:

```sh
TMPDIR=/private/tmp UNIC_E2E=1 go test ./e2e/cli -run '^(TestMigrationFirstRun|TestMigrationTerminalCancellation)$' -count=1 -v
make desktop-e2e CASE=first-install-setup
make desktop-e2e CASE=setup-later-migration
UNIC_MIGRATION_RESILIENCE_E2E=1 go test ./e2e/headless -run '^TestProductionMigrationResilience$' -count=1 -v
```

For ENOSPC, run `e2e/release/migration_resilience.py` in an isolated Linux
container with `--network none --read-only --memory 768m --cpus 2`, a private
256 MiB `/tmp`, and a **separate** `--tmpfs /limited:rw,size=32m`. Bind only the
authorized repository read-only, task binaries read-only, and a private result
directory. Pass `--cli /bin/task/unic --daemon /bin/task/unicd --records 10000
--limited-dir /limited --artifacts /results`. Never fill a host filesystem.
Build Linux programs with `CGO_ENABLED=0 GOOS=linux GOARCH=arm64 go build`.
Use `sh e2e/credentials/run.sh /bin/task/credentials` in an isolated Linux image
with real GNOME Secret Service for the credential probe.

Windows native execution, Linux locked-store prompts, macOS actual credential
prompts and OS directory-picker interaction remain NOT_RUN. Accessibility was
not granted, so system-dialog automation paused without changing permissions.
The separate existing lifecycle C03 assertion remained intermittent (one failure,
two passes in an isolated three-run probe). The full lifecycle suite is not
claimed passed. The ordinary race suite passed 782 tests across 80 packages;
fixed-version lint reported zero issues. Documentation passed with four unrelated
stale-page warnings.

Only these small receipts and hashes are committed. Task binaries, private
profiles, source snapshots, raw logs and worker worktrees were removed. Local
canonical native reports remain ignored review artifacts. No public push, PR,
merge, release or deployment was performed.
