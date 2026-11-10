# Service application

`unic` is the CLI. `unicd` is the headless Engine owner. Neither program imports a UI toolkit.

```sh
go build -o dist/ ./app/service/cmd/unic ./app/service/cmd/unicd
UNIC_HOME=/absolute/private/state dist/unicd
echo hello | UNIC_HOME=/absolute/private/state dist/unic submit --json
```

See [headless architecture](../../docs/architecture/headless.md) and the [local service contract](../../docs/contracts/local-service-contract.md).
The optional `unic-migrate` program performs offline history import and is not included in the standard release archives.


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

Export retained local logs without a running service:

```sh
unic logs export
unic logs export --output ./unic-logs.tar.gz
```

The private tar.gz contains local JSONL segments and a SHA-256 manifest. Existing
outputs are not replaced. Live export copies already-written bytes and does not
flush service queues. See the [observability contract](../../docs/contracts/observability-contract.md#12-offline-local-log-export).
