# Private macOS bundle launch failures

Specified before this native process acceptance script.

1. The temporary native login-detection launcher consumes or loses a manual activation.
2. A manual bundle entry omits the GUI because a saved background preference is true.
3. Native role processes share a PID, installation identity or unsafe profile.
4. Closing a normal GUI terminates its tray or service through an application coalition.
5. Closing a lightweight GUI leaves a tray, panel, adapter or shortcut owner.
6. Lightweight close restarts the service instead of transferring the same generation and PID.
7. Explicit lightweight background entry creates a graphical role after setup completion.
8. The test opens personal history, registers login startup or reads/writes the host clipboard.
9. A failed assertion leaves task-owned role or service processes running.
10. Lightweight background entry bypasses pending setup and starts capture or a service.
11. Native tray auto-start creates a GUI despite explicit normal `--background` entry.

Use a private `.app`, canonical task profile and the production `desktop_e2e`
directory-board override. Keep the nonvisual role fixture disabled. Use app-scoped
window enumeration to establish the native host's reported visible GUI state.
This is not OS-native visibility or foreground-focus evidence. Role IPC commands stand
for close/quit intent; they do not prove native tray clicks or physical input.
Login/logout, login registration, global shortcut delivery and actual paste are
not executed. The receipt binds source, binary and PID identities and cleanup.
