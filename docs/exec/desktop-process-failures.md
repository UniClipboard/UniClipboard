# Desktop process separation failure inventory

Specified before the process-role implementation. The acceptance entry is
`make desktop-e2e CASE=desktop-processes`.

## Failures to prevent

1. Saved lightweight mode starts only unicd and leaves no tray entry.
2. A running unicd prevents a tray command from creating a visible GUI process.
3. Closing the GUI hides a resident renderer instead of terminating its process.
4. Reopening the GUI creates duplicate processes or replaces durable history.
5. The quick panel shares the GUI process or requires an open GUI.
6. Concurrent launch creates duplicate tray, GUI, shortcut or adapter owners.
7. A foreign profile or executable can activate or terminate another profile.
8. A stale PID or endpoint blocks recovery after a role crash.
9. A GUI or panel crash terminates the tray or the service.
10. A service restart leaves a stale adapter or applies disconnected backlog.
11. Explicit Quit leaves owned roles, adapter registrations or an owned service.
12. Explicit Quit terminates an independently started service.
13. Setup or migration starts capture before its completion barrier.
14. Shortcut or native permission failure is presented as success.
15. Two adapters write or observe the same content and create synchronization echo.
16. A process fixture is reported as native tray, focus or paste acceptance.
17. Tray startup replaces an independently started standalone service.
18. Tray startup adopts a live desktop-owned or CLI-temporary service.
19. Invalid daemon paths or an unknown listener are modified before identity validation.
20. An acquired desktop-owned service survives owner exit, or its shutdown stops an adopted standalone service.

## Evidence boundary

The process case must execute actual OS processes and the production local service,
profile storage, role transport and lifecycle code. A build-tagged nonvisual host
may replace native rendering and clipboard access. Such a host does not prove
native tray visibility, OS shortcut delivery, focus restoration or paste insertion.

The receipt records role PIDs, launcher and role-acquisition timestamps, binary hashes, source identity,
command results, durable history and settings checks, and cleanup results.
Role acquisition time is not a kernel process creation timestamp.
The canonical runner retains the receipt and SHA256SUMS. Replay uses the entry above. It removes
private binaries, profiles and logs after cleanup.
