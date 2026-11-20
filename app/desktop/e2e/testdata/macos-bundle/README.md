# Private macOS bundle process acceptance

Run on macOS from a repository checkout:

```sh
python3 app/desktop/e2e/testdata/macos-bundle/run.py \
  --root . --output .build/macos-bundle-launch-unique-run
```

The output directory must not exist. The script freezes tracked and untracked
nonignored source into a task-private directory, builds race-instrumented
`unic-desktop` and `unicd`, and creates one temporary application bundle with
a unique bundle identifier. Native roles use the production `desktop_e2e`
directory-board override. The nonvisual process fixture stays disabled.

The script launches the real application through LaunchServices, runs native
event loops and reads app-scoped content-window visibility from mygo automation. Private role IPC supplies close and quit intent.
It verifies explicit normal background entry without a GUI, normal GUI close,
lightweight GUI/panel close with same-generation
service ownership transfer, explicit lightweight background entry, and pending
setup admission. It records binary/source hashes, role PIDs, process timestamps,
service generations and cleanup in `receipt.json` and `SHA256SUMS`.

This is a supplemental native bundle-entry check. It does not replace canonical
Desktop E2E. It does not click a tray item, deliver a global shortcut, register a
login item, log out, read/write the host clipboard, or inject paste into another
application. No system permission is requested. The receipt marks OS-native visibility and foreground focus NOT_RUN. A mygo
window-visible receipt does not establish that the OS shows or foregrounds it.
No CGWindowList query, OS screenshot or pixel comparison is performed.

Cleanup closes only task-profile roles, stops only task-profile services,
unregisters the temporary bundle from LaunchServices and removes private source,
programs, profiles and logs. Receipts are retained under the chosen output path.
