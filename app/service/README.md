# Service application

`unic` is the CLI. `unicd` is the headless Engine owner. Neither program imports a UI toolkit.
Packaged archives also expose the `uniclip` alias for `unic`: a relative symlink on macOS/Linux, or a `uniclip.cmd` launcher on Windows.
Both command names use the same implementation. Direct `go build` commands below produce the canonical binaries only.

```sh
go build -o dist/ ./app/service/cmd/unic ./app/service/cmd/unicd ./app/service/cmd/unic-migrate
UNIC_HOME=/absolute/private/state dist/unicd
echo hello | UNIC_HOME=/absolute/private/state dist/unic submit --json
```

See [headless architecture](../../docs/architecture/headless.md) and the [local service contract](../../docs/contracts/local-service-contract.md).
Release archives and npm platform packages include `unic-migrate` beside `unic` for offline v1 history import.
When a known v1 data directory exists, interactive CLI commands show a compatibility warning on stderr.
Fresh installations, noninteractive commands and `--json` output remain silent.
The check covers the default v1 directory and the active `UC_PROFILE` directory. It does not read credentials or change data.
Run `unic migrate --help` for backup, preflight, import and verification steps.
Custom and portable v1 profiles require `--source DIR`; named profiles also require the matching `--service`.
Stop v1 and the target `unicd` before importing. Migration preserves source data and resumes successful records without duplication.
Settings, device identities and trust do not migrate. Upgrade all peers to v2 and pair them again.


Remote logs, metrics and product events default enabled. Release assembly supplies the selected PostHog project endpoint and ingestion-only token.
A developer build without deployment metadata keeps the service usable and reports degraded upload status.
Disable every upload in `$UNIC_HOME/config.toml`:

```toml
[telemetry]
enabled = false
```

Use `[telemetry.logs]`, `[telemetry.metrics]`, or `[telemetry.analytics]` with `enabled = false` to disable one signal.
Switch changes hot reload and remain effective across restart. Check `unic diagnostics telemetry` for actual signal state.
See the [observability contract](../../docs/contracts/observability-contract.md).

## Release identity

Use `unic --version --json` or `unicd --version --json` to read the embedded product version, build number and source SHA without starting the service.
Versioned archives and npm publication follow the [release architecture](../../docs/architecture/releases.md).
