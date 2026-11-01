# Linux binary Secret Service regression

Run from the repository root on native Linux with Rust, Python 3,
`dbus-run-session`, `dbus-send`, and `gnome-keyring-daemon` installed:

```bash
python3 scripts/e2e/linux-binary-secret-storage.py
```

The runner builds the production `uc-platform` provider and its diagnostic
example with `--locked --message-format=json`. It obtains the executable path
from the matching Cargo `compiler-artifact` message. Execution and hashing use
that same absolute path, including when `CARGO_TARGET_DIR` or Cargo configuration
changes the build directory. The private session receives the resolved path
through `UC_BINARY_SECRET_E2E_BINARY`. There is no default-path fallback.

It creates private session buses with no activation
directories. It replaces HOME and every relevant XDG directory. Wallets exist
only in a disposable `/dev/shm` directory. The runner sends a generated wallet
password through a pipe and discards backend message bodies. It never connects
to the user's session bus or wallet. Do not run the example directly against a
user session bus.

Each write, read, inspection and deletion runs in a separate process. Cases cover:

- Creation and replacement of empty values and all byte values, including NUL
  and invalid UTF-8. Both paths must use `application/octet-stream`.
- Exact reads after process exit, deletion, absence and repeated deletion.
- Reading old `text/plain` entries, including entries without `target`, and
  updating them in place. Object path, label and extra attributes must survive.
- Ambiguous entries: reads, writes and deletes must fail and preserve both items.
- Service absence and D-Bus permission denial with the private service running.
- The production integrity probe, unchanged System source record, and absence
  of a file key store on success and failure.

Artifacts default to `target/binary-secret-storage`. Set
`UC_BINARY_SECRET_ARTIFACTS` to use a separate directory for each run. The result
records source SHA, the complete scoped patch hash (including untracked inputs),
input and binary hashes, locked dependency versions, commands, exit codes and
case statuses. `source.patch` makes the tested changes reviewable. Compiler logs
are retained. Secret values, passwords, wallet files and D-Bus bodies are not
retained. Cargo JSON build messages are retained alongside compiler diagnostics.
Each private bus session has a dedicated POSIX process group. On completion,
timeout or exception, the runner sends `SIGTERM` to remaining group members,
waits up to five seconds, and escalates to `SIGKILL` if needed. It reaps the
direct child and waits for the process group to disappear before deleting the
wallet directory. If cleanup cannot be proven, the run fails and retains the
directory in `/dev/shm`; it records the retained path for operator cleanup.
Structured session results include timeout status, signals sent, leader reap,
group disappearance and directory deletion. No wallet data is copied to artifacts.
Exit codes are 0 for passed executable cases, 1 for failure and 77 when the Linux
environment is unavailable. Failed runs retain their reports.

Run the runner's local process regression on macOS or Linux without a wallet:

```bash
UC_RUNNER_PROCESS_ARTIFACTS=target/binary-secret-runner-process \
  python3 scripts/e2e/binary-secret-runner-process.py
```

This regression compiles a tiny synthetic Rust example with real Cargo in a
custom target directory containing spaces. It leaves a stale executable in the
default directory and verifies that the private session executes the Cargo
artifact whose hash was recorded. Synthetic processes verify timeout cleanup of
a child process, `SIGKILL` escalation, command failure, spawn failure and directory
deletion order. The regression records commands, hashes, exits and cleanup facts.
It does not exercise Secret Service or claim Linux/KDE acceptance.

This GNOME regression does not prove strict KDE behavior. Wallet unlock and
prompt cancellation are explicitly `NOT_RUN`. For KDE acceptance, use a separate
Linux test account and wallet. Record the distribution, Plasma, KWallet/ksecretd,
Qt and application versions. Verify that ksecretd owns `org.freedesktop.secrets`.
Verify creation, replacement, cross-process reads, deletion, old-entry updates,
locked-wallet unlock and cancelled prompts. Check the application's startup,
restart and unlock with System storage selected. Keep the wallet outside the
artifact directory. Do not include wallet contents or secrets in evidence.

Additional existing regression entry points:

```bash
python3 packages/desktop-host-go/e2e/startup/readiness_model.py
python3 packages/desktop-host-go/e2e/startup/secret_service.py
UC_E2E_REQUIRE_SECRET_SERVICE=1 bash scripts/e2e/linux-legacy-file-key-store.sh
```

The readiness model runs production probe policy with controlled faults. It does
not exercise Linux D-Bus. The other two entry points require Linux and do not
replace KDE acceptance. The legacy-file suite additionally needs its daemon/CLI
build dependencies and fixtures.
