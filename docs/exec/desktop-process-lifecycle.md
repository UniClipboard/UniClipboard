# Desktop process lifecycle acceptance

## Source and scope

Worktree: `b973f49e-2d6a-498f-bde6-2774391245a4`.
Branch: `fix/desktop-lightweight-processes`.
Baseline: `173d362036ae4a746878306915c85bcdf7468751`, fetched from `origin/v2` on 2026-11-20.
Implementation commit: `51ee83471`. The change is committed locally and has not been pushed. Acceptance manifests bind the exact source-file and executable SHA-256 values; the baseline HEAD alone does not identify the modified source.

The user's Windows x64 comparison artifact is `desktop-test-windows-amd64`, artifact `14596984031`, from [Build CLI run 44665561906](https://github.com/UniClipboard/UniClipboard/actions/runs/44665561906). The GitHub API reports the same baseline SHA above. An ARM64 Windows host must distinguish this x64 emulated artifact from a native ARM64 build.

## Resulting process model

```text
unic-desktop launcher -- private role IPC --> tray process
                                               |-- clipboard adapter
                                               |-- global shortcut / target capture
                                               |-- native notifications / login / updates
                                               |-- owns pipe to unicd, or borrows standalone unicd
                                               |-- starts / activates GUI process
                                               `-- starts / activates quick-panel process

GUI process ----------- shared local service API ---- unicd / Engine / history
quick-panel process --- shared local service API ---- unicd
quick-panel process --- private apply/mark RPC ------ tray's only adapter
```

Quit/close handlers publish their complete IPC acknowledgement before scheduling process shutdown. Tray retires UI roles, its adapter, its owned service and its role endpoint before mygo relaunch starts a replacement. GUI activation also transfers Windows foreground permission to the verified GUI PID.

All three desktop roles use the same installed executable. They are separate OS processes, with separate renderer lifetimes. Closing the GUI terminates its process. Dismissing the quick panel hides its independent process, which persists until explicit application Quit. Tray and service do not require a resident GUI. A role endpoint and live kernel lock enforce per-profile, per-role ownership. Both IPC peers validate the OS peer PID and executable path/hash. The role-acquisition timestamp is not a kernel process-creation timestamp.

See the [architecture](../architecture/headless.md#desktop-startup-settings), [design](desktop-process-design.md), [failure inventory](desktop-process-failures.md), and [quick-panel contract](../contracts/desktop-quick-panel-contract.md).

## Evidence recorded so far

| Check | Result | Evidence and limit |
|---|---|---|
| Baseline independent tray entry | FAIL before implementation | Canonical `desktop-processes` run `20261120T033223Z-904afd33`; old binary rejects `--role` |
| Role IPC spoof rejection | PASS | `.build/desktopprocess-ipc-20261120/`; real-process RED/GREEN, OS peer authentication and race |
| Service acquisition | PASS | `/tmp/unic-acquire-b973/`; real processes preserve an independent standalone generation, refuse foreign/unknown owners and release owned services |
| Canonical separated processes | PASS | `.build/desktop-e2e/20261120T040810Z-cd66f6c6/`; nonvisual role host with production IPC, service, storage and directory adapter |
| Legacy service primitives | PASS | `.build/desktop-e2e/20261120T034257Z-03fce0ca/`; this is not the current lightweight product behavior |
| Quick-panel focus, permission, cancellation and ordering | PASS | `.build/desktop-e2e/20261120T035124Z-fe372394/`; OS fixtures, not actual native input |
| macOS native history/search/copy/quit | PASS | `.build/desktop-e2e/20261120T035221Z-656bf1c5/`; real windows, synthetic directory clipboard, recording and cleanup |
| macOS native independent panel/GUI processes | PASS | `.build/desktop-e2e/20261120T040750Z-1394ed91/`; distinct tray, GUI, panel and service PIDs, real panel Copy, GUI termination/recreation, role endpoint and OS process cleanup |
| macOS native setup/migration | PASS | `.build/desktop-e2e/20261120T035346Z-ac653298/`; 1505 synthetic encrypted records, wrong/correct passphrase, import, verification and history entry |
| Desktop and Engine existing checks | PASS | 197 existing tests with race across five packages, using canonical `TMPDIR=/private/tmp` |
| Windows amd64/ARM64 and Linux amd64 compilation | PASS | Production Desktop cross-builds; no native execution claim |
| Fixed lint | PASS | golangci-lint v2.15.0 with Go 1.27.3; changed packages |
| Documentation check | PASS with warnings | The four updated architecture pages now verify implementation commit `51ee83471`. Three unrelated stale verification warnings remain. The initial pre-commit `-bump` refusal was resolved by the local implementation commit |

The first macOS native history run `20261120T035013Z-7a95cf83` passed business assertions but failed cleanup. Its close callback prevented application shutdown. The same native case passed after adding a quit-in-progress guard. The failure report remains local evidence; it is not counted as a pass.

## Reproduction

```sh
make desktop-e2e CASE=desktop-processes
make desktop-e2e CASE=quick-panel-contract
make desktop-e2e CASE=history-search-copy
make desktop-e2e CASE=setup-onboarding
make desktop-e2e CASE=desktop-native-processes
```

Each canonical run freezes source, isolates profiles and synthetic content, and emits `index.html`, `manifest.json`, case receipts and `SHA256SUMS`. Native cases retain real window-content screenshots and continuous recordings. Private source snapshots, binaries, profiles and logs are removed only after task-owned process cleanup. No fixture proves a native tray click, OS hotkey delivery or cross-application paste.

## Windows coordination

The authorized Parallels VM is `32affa85-5174-4d5b-ab6b-903a06d7fa9b`, Windows 11 ARM64. Initial state was running, with shared clipboard enabled. A read-only identity check confirmed the OS and VM identity. The independent memory research task has first use of the VM. This task has not yet launched guest applications or changed its settings. Native acceptance remains pending until serial use is available. Do not mix x64 emulation measurements with ARM64 native behavior.

## Remaining evidence

Native Windows tray clicks, native global-hotkey delivery and actual target insertion are separate acceptance gates. Actual login credential/permission prompts and other-platform native input remain separate from compile/process receipts. No production publication, push, PR or merge is part of this task.
