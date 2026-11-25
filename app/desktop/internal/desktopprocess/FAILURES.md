# Role IPC failure specification

Written before implementation. These process scenarios are the verification boundary.

- Concurrent launches must create one live owner for each profile and role.
- GUI exit must release its role lock without stopping tray or service.
- A crashed role must release its kernel lock; its stale socket must be recoverable.
- A live role must never be replaced on the strength of a stale PID file.
- An endpoint for another profile, executable path or executable SHA-256 must reject commands.
- Private state must reject symlinks and group/world access; Windows pipes must restrict access to the current user.
- An unrecognized live listener must never be unlinked.
- Malformed, excessive or stalled requests must not exhaust handlers or block shutdown.
- RPC cancellation and deadlines must release the connection.
- Launch errors must preserve existing roles and return an actionable failure.
- Closing an owner must stop admission and release its listener and lock.

The real-process fixture records OS PID, process start receipt, executable identity,
profile, role, IPC commands, shutdown and crash recovery. It opens no GUI and does
not establish native tray, focus or paste acceptance. The canonical Desktop process
scenario separately verifies the production launcher and service integration.

## IPC verification receipt

Replay with `UNIC_ROLE_E2E=1 go test -race ./app/desktop/internal/desktopprocess -run TestRoleProcesses -count=1 -v`.

- Initial RED: the real-process fixture could not build because the role runtime did not exist.
- Security RED: a different executable sent a forged valid profile/path/hash identity. The previous transport accepted it.
- GREEN: three independent OS processes answer private IPC; a duplicate owner exits; GUI crash releases its kernel lock; a recreated GUI has a new PID while tray retains its PID; explicit close removes role endpoints; the forged executable is rejected by OS peer verification.
- The role receipt includes PID, role-acquisition time, profile and executable path/hash. Role-acquisition time is not a claimed kernel process-creation timestamp.
- Unix checks the peer UID/PID through the existing `peercred` dependency. Linux resolves `/proc/<pid>/exe`; macOS resolves `proc_pidpath`. Windows resolves named-pipe client/server PID and `QueryFullProcessImageName`.
- A running process caches its first executable hash alongside its OS file identity (device/volume, inode/file index, size, timestamps), re-hashing whenever metadata changes, and refuses IPC after its installed executable changes. New processes cannot silently adopt an old executable generation.
- Windows pipes restrict the DACL to the current user. The existing pipe library rejects remote clients. Unix sockets use a hashed profile endpoint inside a private, owner-checked `/tmp/unic-desktop-<uid>` directory to stay within macOS socket path limits.
- macOS process execution with race instrumentation passed. Windows amd64 and Linux amd64 compilation passed. `go vet` and repository-pinned golangci-lint passed.
- Native Windows/macOS/Linux tray interaction, OS focus, global shortcuts and paste are outside this fixture.

## Shutdown acknowledgement failure specified before the fix

A native quit callback can cancel the role context before its IPC handler returns.
Cancellation closes active role connections. The caller then sees EOF even when
shutdown succeeds. The real-process fixture must cancel its context on close and
still deliver a complete JSON acknowledgement before immediate endpoint cleanup.
No fixed shutdown delay is an acknowledgement barrier.

The shutdown scenario first failed when close cancelled its role context inside
the handler. It passed after using ReplyAfter. ReplyAfter encodes only the wrapped
value and runs its callback in an independent goroutine after a successful full
response write. Failed encoding or a failed write never runs the callback. A
complete acknowledgement confirms admission; probing the endpoint confirms role
shutdown. The fixture needs no shutdown timer.

## Retiring-role error classification specified before the fix

A role can still authenticate and answer while retirement has already closed
business-command admission. Returning ErrUnavailable from that handler must
survive the IPC boundary as a stable machine-readable error code. A same-binary
client must classify the received error with errors.Is(err, ErrUnavailable),
including when the server wraps the sentinel. A different error with identical
human-readable text must not become retryable. Identity verification must still
precede any remote error-code mapping. Retrying business commands is a caller
policy; this transport does not replay a command.

## First native Windows window

A UI role must not inherit the service's `STARTF_USESHOWWINDOW/SW_HIDE` policy.
Windows Full01 and Full02 reproduced a registered global shortcut and a live
panel process whose first native window never became visible. A plain explicit
Show still consumes the hidden startup policy; it is not evidence of visibility.
Use a console-free process creation flag without overriding native ShowWindow.

## Stale previous-build view roles after update

If an earlier installation left a `gui` or `quick-panel` process holding the
profile's role endpoint after an in-place or move-aside executable replacement
(for example `.unic-desktop.exe.old-<pid>` from `mygo.Updater`), IPC still
rejects commands and classifies the mismatch as `ErrStaleBuild`. `RetireStale`
operates only on view roles (`gui`, `quick-panel`), verifies via the live OS
endpoint peer handle that the listener is a replaced build of this exact
executable in the same installation directory, terminates only that verified
process, and waits until the role endpoint becomes `ErrUnavailable`. Live
current-build roles, the `tray` role, and unrecognized binaries in the same
directory are never retired.
