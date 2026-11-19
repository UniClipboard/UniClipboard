# Real Linux credential-store acceptance

This process probe uses the existing `godbus` dependency and a real GNOME
Secret Service. It writes only task-owned binary items in a new HOME and a
new D-Bus session. It verifies modern and legacy attribute lookup, raw NUL and
non-UTF-8 bytes, ambiguous and missing items, unavailable service guidance, and
cancellation before credential access. It never prints a credential value.

Build and run inside an isolated Linux container with `dbus-run-session` and
`gnome-keyring-daemon` installed:

```sh
go build -o /tmp/credentials ./e2e/credentials
sh e2e/credentials/run.sh /tmp/credentials
```

The container must have no personal credential mounts. Destroy the container
after the command to terminate its private credential daemon. A locked-store
native prompt still requires separate user-authorized interaction. This probe
does not establish that native prompt acceptance.
