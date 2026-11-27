# Service application

`unic` is the CLI. `unicd` owns the Engine and starts headless by default. Neither program imports the desktop UI toolkit. Desktop may explicitly enable the native clipboard adapter in the service on supported macOS, Windows and Linux sessions.
Packaged archives expose the `uniclip` alias for `unic`: a relative symlink on macOS/Linux or a `uniclip.cmd` launcher on Windows.

```sh
go build -o dist/ ./app/service/cmd/unic ./app/service/cmd/unicd
dist/unic init                         # initialize default profile (or use -p/--profile NAME)
dist/unic daemon start                 # keep unicd running in the background
echo hello | dist/unic send --to all --json
```

Send and receive without pairing (for example with the page at `try.uniclipboard.app`):

```sh
unic send "hello"                 # prints a 6-digit code and a link, then waits for one receiver
unic recv 482913                  # on the other side; a link works too
unic send --to laptop "hello"     # to one paired device; --to all reaches every paired device
```

`unic` starts a temporary `unicd` from the same directory when a service command needs one and none runs.
It stops that service after the last command using it finishes. A `unicd` managed via `unic daemon start` (or run directly) and the desktop's service are never stopped by ordinary `unic` commands.
Use `unic daemon start|stop|restart|status|logs` to control the background service, `unic status` to inspect state without launching a temporary daemon, `unic doctor` to diagnose profile/daemon health, and `unic completion bash|zsh|fish` for shell completions.
Commands that wait for another device (`device add`, `dev logs level --for`) keep a temporary service running for a bounded window and say so on stderr.
`send` waits for its own send to finish and exits 4 when the content was committed but the send was not fully completed.

See [headless architecture](../../docs/architecture/headless.md) and the [local service contract](../../docs/contracts/local-service-contract.md).
The `unic migrate` command performs offline history import in-process.
Run `unic migrate --help` for backup, preflight, import and verification steps.
Business commands refuse until explicit migration initialization completes for the selected data directory. JSON and noninteractive invocations receive the same actionable refusal (exit 6, `migration_required`). Help and version remain accessible. A partial import remains pending. See the [initialization contract](../../docs/contracts/local-service-contract.md#explicit-cli-migration-initialization).


Remote logs, metrics and product events default enabled. Release assembly supplies the selected PostHog project endpoint and ingestion-only token.
A developer build without deployment metadata keeps the service usable and reports degraded upload status.
Disable every upload in `$UNIC_HOME/config.toml`:

```toml
[telemetry]
enabled = false
```

Use `[telemetry.logs]`, `[telemetry.metrics]`, `[telemetry.traces]`, or `[telemetry.analytics]` with `enabled = false` to disable one signal.
Traces default enabled and sample 1% of unique explicit submissions and their commit calls. Set `[telemetry.traces]` to `enabled = false` to disable traces. Trace context stays inside the service.
Switch changes hot reload and remain effective across restart. Check `unic dev diagnostics telemetry` for actual signal state.
See the [observability contract](../../docs/contracts/observability-contract.md).

Export retained local logs (works with or without a running service):

```sh
unic dev logs export
unic dev logs export --output ./unic-diagnostics.zip
```

The private `.zip` bundle contains local JSONL segments (`logs/engine/`), a SHA-256
manifest with build/platform metadata, and—when an existing `unicd` is already
running—a best-effort safe runtime/telemetry/log-level summary without ever starting
a temporary service. Existing outputs are not replaced. Live export copies
already-written bytes and does not flush service queues. See the [observability contract](../../docs/contracts/observability-contract.md#12-offline-local-log-export).

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

### Native desktop clipboard acceptance

On an isolated Windows, X11, or Wayland desktop, build `unicd` from the
checkout and run the production-daemon test below. The test writes synthetic
values to the session clipboard and outputs `manifest.json` and
`SHA256SUMS`.

```sh
go build -o /tmp/unicd-clipboard ./app/service/cmd/unicd
UNIC_NATIVE_CLIPBOARD_E2E=1 \
UNIC_NATIVE_CLIPBOARD_DAEMON=/tmp/unicd-clipboard \
UNIC_NATIVE_CLIPBOARD_ARTIFACTS=/tmp/unicd-clipboard-receipt \
go test ./e2e/headless -run '^TestDaemonNativeDesktopClipboard$' -v -count=1
```

On Windows set the same environment variables and use a path to the built
`unicd.exe`. In an X11 session, set `UNIC_NATIVE_CLIPBOARD_X11_INCR=1` and
make `xclip` available to verify a large image in both directions. In a
Wayland session without data control, set
`UNIC_NATIVE_CLIPBOARD_EXPECT_UNSUPPORTED=1` to verify that the service
stays headless and reports the unavailable capability.
