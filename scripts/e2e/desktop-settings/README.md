# Desktop Engine settings adoption E2E

The runner starts the real daemon and production Go CLI in synthetic, isolated profiles.
It uses the no-op system clipboard and portable file secure storage. It does not open a GUI,
browser or VM, and does not read production profiles or system credentials.

Build from the repository root:

```bash
cargo build --locked -p uc-daemon
(cd apps/cli-go && go build -o ../../target/compat/go/uniclip ./cmd/uniclip)
uv run --with pynacl==1.6.2 --with argon2-cffi==25.1.0 python scripts/e2e/desktop-settings/adoption.py \
  --daemon target/debug/uniclipd --cli target/compat/go/uniclip --out target/settings-adoption-evidence
```

Use a new output directory for each run. Bun is required for the WebSocket observer.
The runner keeps input/output settings copies, SHA-256 inventories, daemon logs,
API responses with authentication responses redacted, CLI stdout/stderr and exit codes,
process exit codes, the encrypted configuration bundle and a final `result.json`.
The final receipt includes the Engine SHA, source hashes and binary hashes.
All profiles and keys under the output directory are synthetic test artifacts.

Coverage:

- Failed-import preservation before Engine I/O, successful retry after correction, and
  lenient fallback for unusable current product files without legacy fields.
- Direct pre-P1 migration and P1 field-set-2 migration before Engine rewrites settings.
- Removed product keys, repeated restart idempotence and invalid legacy enum startup.
- CLI debug writes, persistent preferences and environment-overridden effective logging.
- Real upgrade backup and configuration export inventories.
- Configuration import into separate fresh and existing installations, followed by an
  additional write to detect stale in-memory settings.
- Retained local telemetry consent and rejection of a synthetic old-schema incomplete bundle
  through both preview and staging, including the re-export explanation.
- Engine reset failure, host deletion failure, explicit failure response, retained memory,
  successful retry, Engine-owned updater deletion, Desktop settings event and restart defaults.

The reset failure cases use invalid file types inside the isolated test profile.
The test creates no production fault-injection path. The incomplete bundle is re-sealed with
an old manifest schema; it is a deterministic fixture, not an archived historical user bundle.

Not covered: power loss, forced termination during atomic persistence, concurrent reset/write
stress, native GUI/shortcut registration, real historical user profiles, Windows/Linux execution,
physical-device synchronization or release installation. Engine PRs #225, #226 and #227 remain
cross-repository release dependencies; a successful local run does not mark them released.


Run review regressions against the same built binaries with:

```bash
uv run --with pynacl==1.6.2 --with argon2-cffi==25.1.0 python scripts/e2e/desktop-settings/review_regressions.py   --daemon target/debug/uniclipd --cli target/compat/go/uniclip --out target/settings-review-evidence
```

This runner verifies initial migration warning replay and then kills/stops only its own spawned
real daemons. It requires nonzero-exit receipts, timeout escalation, and cleanup of a surviving
daemon. It does not establish Windows or power-loss durability.
