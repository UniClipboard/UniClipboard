# Private macOS bundle launch acceptance

## Scope and identity

The supplemental script and its pre-execution failure inventory live in
[`app/desktop/e2e/testdata/macos-bundle`](../../app/desktop/e2e/testdata/macos-bundle/README.md).
It freezes source and builds a task-private application bundle with race
instrumentation. It uses native event loops and the production directory-board
E2E override. The nonvisual role fixture is disabled.

The complete run used macOS 27 ARM64. Its base HEAD was
`a99ccc0bcf2309eb8cf0bb3468802c4f740fd786` with modified source. The frozen source
tree digest was
`281cf585fc2b5ebd54d1daea25c70798c3186b6f8af182f0bd046cf356be5b8d`.
The local receipt retains source-file hashes, binary hashes, role PIDs, kernel
start-time observations, service generations and cleanup:
`.build/macos-bundle-background-green-20261120/receipt.json`.
Its sibling `SHA256SUMS` covers the receipt.

## Results

- PASS: Explicit normal background entry retains native tray and service
  processes without creating a GUI role endpoint during the observation window.
- PASS: Manual normal bundle entry reports an app-scoped visible GUI window
  even when the saved login background preference is true. GUI close retires
  that process and preserves the same tray and service identities.
- PASS: Manual lightweight bundle entry reports app-scoped visible GUI and
  panel windows in separate OS processes. GUI close retires all three Desktop
  roles and transfers the same service generation and PID to standalone mode.
- PASS: Explicit lightweight background entry leaves only standalone unicd.
- PASS: Pending setup overrides lightweight background entry. The native setup
  host reports an app-scoped visible window without starting a service/adapter.
- PASS: All observed task processes exited without forced cleanup. The private
  bundle was unregistered from LaunchServices. Programs, source snapshot,
  profiles and raw logs were removed.

The normal background case first failed against frozen tree
`404d110de86b7bf25f4223470f89660f704910405a042da3899b50359449ff1c`:
the native tray's unconditional startup created GUI PID 3522 despite the explicit
background entry. The failure receipt is
`.build/macos-bundle-normal-background-red-20261120/receipt.json`.
After the launcher passed its background intent into the native tray role,
the same case and the complete bundle scenario passed. This was actual role
process creation, independent of the window-visibility evidence below.

Window visibility comes only from mygo automation `windows[].visible`.
It is app-scoped state. OS-native visibility and foreground focus are NOT_RUN.
This receipt does not use CGWindowList, OS screenshots or physical input.
Native tray clicks, global shortcut delivery, actual paste and real login
registration/logout remain NOT_RUN. No system permission was requested.
The script never accesses the host clipboard or personal installation.

## Replay

```sh
python3 app/desktop/e2e/testdata/macos-bundle/run.py \
  --root . --output .build/macos-bundle-launch-new-run
```

Choose a new output directory. Each replay records its own exact source identity.
This supplemental check does not replace canonical Desktop E2E or Windows
native visibility/input acceptance.
