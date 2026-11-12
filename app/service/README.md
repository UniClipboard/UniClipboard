# Service application

`unic` is the CLI. `unicd` is the headless Engine owner. Neither program imports a UI toolkit.
Packaged archives expose the `uniclip` alias for `unic`: a relative symlink on macOS/Linux or a `uniclip.cmd` launcher on Windows.

```sh
go build -o dist/ ./app/service/cmd/unic ./app/service/cmd/unicd
echo hello | UNIC_HOME=/absolute/private/state dist/unic send --to all --json
```

Send and receive without pairing (for example with the page at `try.uniclipboard.app`):

```sh
unic send "hello"                 # prints a 6-digit code and a link, then waits for one receiver
unic recv 482913                  # on the other side; a link works too
unic send --to laptop "hello"     # to one paired device; --to all reaches every paired device
```

`unic` starts a temporary `unicd` from the same directory when a command needs one and none runs.
It stops that service after the last command using it finishes. A `unicd` you start yourself, and the desktop's service, are never stopped by `unic`.
Run `UNIC_HOME=/absolute/private/state dist/unicd` to keep receiving content between commands.
`unic help` marks each command `[local]`, `[service]`, `[service+clipboard]` or `[offline]`.
Commands that wait for another device (`device add`, `dev logs level --for`) keep a temporary service running for a bounded window and say so on stderr.
`send` waits for its own send to finish and exits 4 when the content was committed but the send was not fully completed.

See [headless architecture](../../docs/architecture/headless.md) and the [local service contract](../../docs/contracts/local-service-contract.md).
The `unic migrate` command performs offline history import in-process.
Run `unic migrate --help` for backup, preflight, import and verification steps.
Interactive commands warn when a known v1 data directory exists. JSON and noninteractive output remain silent.


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

## Release identity

Use `unic --version --json` or `unicd --version --json` to read the embedded product version, build number and source SHA without starting the service.
Versioned archives and npm publication follow the [release architecture](../../docs/architecture/releases.md).

## Directory selections

Directory automatic sync defaults off. Copying a directory records a source reference without scanning it. Explicit paired-device send prepares a snapshot of its current contents:

```sh
unic send --to all -f /absolute/path/to/directory
unic history send @1 --to laptop --json
unic config set sync.directories.automatic true
```

A selection can mix regular files and directories. Empty directories are preserved. Links and unsupported entries fail the selection. Directory transfer does not watch later filesystem changes and requires `--to`. See the [directory contract](../../docs/contracts/directory-contract.md) for resource limits, history semantics, and platform acceptance limits.
