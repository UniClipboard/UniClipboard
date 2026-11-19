# Migration resilience acceptance

Write and review this failure list before changing the importer.

| Failure | Production-process assertion |
| --- | --- |
| System credential backend unavailable | Explain the platform backend and old-passphrase fallback without revealing account, path or secret. |
| Linux collection locked | Complete the Secret Service Prompt/Completed handshake; cancellation must return an unlock failure. |
| Missing or ambiguous credential | Never choose an unrelated item; preserve raw binary bytes on success. |
| Interrupted import | Send SIGINT after committed progress; restart the same command and verify all records without duplicates. |
| Damaged source row | Return partial, repair the row, and import only that row on retry. |
| Large source | Import 10,000 records, report peak child RSS, reopen all records, and retain records beyond normal history limits. |
| Full target filesystem | Use a separate, bounded filesystem only; establish ENOSPC, require failed status, remove the test filler, and resume. |
| Target already owned | Fail without modifying a running service or accessing another data directory. |
| Source or target corrupted | Compare source hashes, SQLite integrity, committed mapping count, and reopened CLI history count. |
| Process restart | Restart the real service twice; assert persistent history and capacity floor. |

Run `migration_resilience.py --help` for the repeatable process entry. It reuses the frozen Rust profile from `migration_acceptance.py`, the production `unic migrate` importer, and real `unicd` processes. It does not inject a replacement importer or storage implementation. A Linux container may supply `--limited-dir /limited` with `--tmpfs /limited:rw,size=32m`; the runner rejects a target filesystem larger than 128 MiB or shared with its ordinary workspace.

Native credentials and native prompts require a platform credential store with task-owned test items. Cross-compilation and frozen credentials do not satisfy that acceptance.

Canonical headless-suite entry (builds real production binaries):

```sh
UNIC_MIGRATION_RESILIENCE_E2E=1 \
UNIC_MIGRATION_RESILIENCE_ARTIFACTS="$PWD/.cache/0011-resilience" \
go test ./e2e/headless -run '^TestProductionMigrationResilience$' -count=1 -v
```

Set `UNIC_MIGRATION_LIMITED_DIR` only to a dedicated bounded filesystem. Without
it, the receipt explicitly records ENOSPC as NOT_RUN. The large-library fixture
contains 10,000 independent records backed by the frozen small encrypted text;
it measures record-count pressure and does not establish multi-gigabyte blob
performance. The process limit is 512 MiB peak RSS for a normal production
binary. The Linux container run uses a 768 MiB memory cap and two CPUs.
