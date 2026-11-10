# Service application

`unic` is the CLI. `unicd` is the headless Engine owner. Neither program imports a UI toolkit.

```sh
go build -o dist/ ./app/service/cmd/unic ./app/service/cmd/unicd
echo hello | UNIC_HOME=/absolute/private/state dist/unic submit --json
```

`unic` starts a temporary `unicd` from the same directory when a command needs one and none runs.
It stops that service after the last command using it finishes. A `unicd` you start yourself, and the desktop's service, are never stopped by `unic`.
Run `UNIC_HOME=/absolute/private/state dist/unicd` to keep receiving content between commands.
`unic help` marks each command `[local]`, `[service]`, `[service+clipboard]` or `[offline]`.
Commands that wait for another device (`device add`, `dev logs level --for`) keep a temporary service running for a bounded window and say so on stderr.
`send` waits for its own send to finish and exits 4 when the content was committed but the send was not fully completed.

See [headless architecture](../../docs/architecture/headless.md) and the [local service contract](../../docs/contracts/local-service-contract.md).
The `unic migrate` command performs offline history import in-process; there is no separate migration program.


Remote logs, metrics and product events default enabled. Release assembly supplies the selected PostHog project endpoint and ingestion-only token.
A developer build without deployment metadata keeps the service usable and reports degraded upload status.
Disable every upload in `$UNIC_HOME/config.toml`:

```toml
[telemetry]
enabled = false
```

Use `[telemetry.logs]`, `[telemetry.metrics]`, or `[telemetry.analytics]` with `enabled = false` to disable one signal.
Switch changes hot reload and remain effective across restart. Check `unic dev diagnostics telemetry` for actual signal state.
See the [observability contract](../../docs/contracts/observability-contract.md).

Export retained local logs without a running service:

```sh
unic dev logs export
unic dev logs export --output ./unic-logs.tar.gz
```

The private tar.gz contains local JSONL segments and a SHA-256 manifest. Existing
outputs are not replaced. Live export copies already-written bytes and does not
flush service queues. See the [observability contract](../../docs/contracts/observability-contract.md#12-offline-local-log-export).
