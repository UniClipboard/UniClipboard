# Desktop process lifecycle acceptance

## Source and scope

Worktree: `b973f49e-2d6a-498f-bde6-2774391245a4`.
Branch: `fix/desktop-lightweight-processes`.
Baseline: `173d362036ae4a746878306915c85bcdf7468751`, fetched from `origin/v2` on 2026-11-20.
Original process implementation: `51ee83471`. Updated activation/close semantics: `213912f70`; last-valid-configuration close correction: `f25ea8c1c3af1edb72911a51d61f746c15baeb3d`. Windows first-display correction: `ce4a0a8cf07fc15b404f20352d08406286261c04`. These changes are committed locally and have not been pushed. Acceptance manifests bind the exact source-file and executable SHA-256 values; the baseline HEAD alone does not identify the modified source.

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

All three desktop roles use the same installed executable. They are separate OS processes, with separate renderer lifetimes. Manual no-argument activation always opens the GUI. Normal GUI close terminates only that process and retains tray/service. Lightweight GUI close retires GUI, tray and panel, leaving only unicd without a hotkey or adapter. The tray transfers an owned service using the strict owner-pipe release command followed by EOF and confirms the same generation/PID report standalone through the existing service API. It does not restart the service. Explicit background entry retains tray/service in normal mode and only unicd in lightweight mode. Normal login uses `desktop.start_in_background` to choose GUI visibility; lightweight login always keeps only unicd. Fresh or pending setup cannot be bypassed. Dismissing the quick panel hides its independent process while tray integration remains. Explicit application Quit still stops only the service owned by that tray; a borrowed standalone service survives. A role endpoint and live kernel lock enforce per-profile, per-role ownership. Both IPC peers validate the OS peer PID and executable path/hash. The role-acquisition timestamp is not a kernel process-creation timestamp.

See the [architecture](../architecture/headless.md#desktop-startup-settings), [design](desktop-process-design.md), [failure inventory](desktop-process-failures.md), and [quick-panel contract](../contracts/desktop-quick-panel-contract.md).

## Updated close semantics acceptance

The follow-up replaces the original lightweight-tray-residency requirement. Its
canonical regression first failed at manual activation (`20261120T062043Z-d8d7b5af`).
The following modified-source receipts record hashes of the frozen source and programs;
the original HEAD alone does not identify their changes.

| Check | Result | Evidence and boundary |
|---|---|---|
| Lightweight close, owned handoff, concurrent reopen, background entry and immediate saved-mode close | PASS | `20261120T065405Z-682d0b58`, exact clean implementation `213912f70`, nine real-process checks, same service PID/generation and cleanup |
| Invalid external TOML close policy | PASS | `20261120T071633Z-84d36e1f` RED, then `20261120T072300Z-e0eebbab` GREEN; last valid lightweight policy closes every UI process and preserves the same service |
| Cold background/manual overlap hypothesis | NOT_REPRODUCED | `20261120T070957Z-3929c022`, four real-process rounds completed; two observed transient desktop ownership. No unproven product change was made |
| Normal background/GUI/panel/crash/adapter regressions | PASS | `20261120T065405Z-b513c45f`, exact clean implementation `213912f70`, thirteen real-process checks; nonvisual host |
| Strict service ownership release | PASS | `20261120T062722Z-bb037bfc`, marker/EOF RED then GREEN, malformed frames, same PID/history and idempotent release |
| Authenticated retiring-error classification | PASS | `.build/desktopprocess-retiring-20261120/`, real-role RED/GREEN, typed retry signal without text matching |
| Native event-loop panel Copy and GUI recreation | PASS | `20261120T064152Z-86917464`; app-scoped windows and directory clipboard, not OS foreground or physical input |
| Private macOS application-bundle entry matrix | PASS | `.build/macos-bundle-background-green-20261120/`; normal background native-path RED/GREEN plus manual/light/setup checks; OS visibility/focus NOT_RUN |
| Existing Desktop/Engine checks | PASS | 782 existing tests with race across 82 packages; module verification and fixed lint passed |
| Updated Windows native close semantics, tray and first-panel activation | PASS | Native ARM64 Full06 on exact clean product `ce4a0a8cf07fc15b404f20352d08406286261c04`; five scenarios, six native tray business clicks, GUI PID exit/recreation, same service PID/generation, Ctrl+Alt+V without GUI, independent rendered panel and Escape target restoration; cleanup PASS |

See the [private-bundle receipt](macos-private-bundle-launch.md). These results
separate process identities from app-scoped visibility and OS-native input.

## Original implementation evidence

The historical receipts below bind their original tested source. They predate the follow-up manual/background and lightweight-close semantics described above. They remain historical evidence and do not establish that the new entry matrix or same-PID ownership transfer passed. Re-run the canonical cases against the follow-up source before extending acceptance.

| Check | Result | Evidence and limit |
|---|---|---|
| Exact baseline saved-lightweight comparison | EXPECTED FAILURE | Supplemental post-implementation comparison built immutable baseline `173d362036ae4a746878306915c85bcdf7468751`; save lightweight through CLI, keep standalone service, launch twice: both desktop PIDs exit and no graphical owner remains. Programs/source/profile cleaned. This host comparison is not Windows native evidence. |
| Baseline independent tray entry | FAIL before implementation | Canonical `desktop-processes` run `20261120T033223Z-904afd33`; old binary rejects `--role` |
| Role IPC spoof rejection | PASS | `.build/desktopprocess-ipc-20261120/`; real-process RED/GREEN, OS peer authentication and race |
| Service acquisition | PASS | `/tmp/unic-acquire-b973/`; real processes preserve an independent standalone generation, refuse foreign/unknown owners and release owned services |
| Canonical separated processes | PASS | `.build/desktop-e2e/20261120T042241Z-e3d8c1dc/`; nonvisual role host with production IPC, service, storage and directory adapter |
| Legacy service primitives | PASS | `.build/desktop-e2e/20261120T034257Z-03fce0ca/`; this is not the current lightweight product behavior |
| Quick-panel focus, permission, cancellation and ordering | PASS | `.build/desktop-e2e/20261120T035124Z-fe372394/`; OS fixtures, not actual native input |
| macOS native history/search/copy/quit | PASS | `.build/desktop-e2e/20261120T035221Z-656bf1c5/`; real windows, synthetic directory clipboard, recording and cleanup |
| macOS native independent panel/GUI processes | PASS | `.build/desktop-e2e/20261120T042240Z-024eba14/`; distinct tray, GUI, panel and service PIDs, real panel Copy, GUI termination/recreation, role endpoint and OS process cleanup |
| macOS native setup/migration | PASS | `.build/desktop-e2e/20261120T035346Z-ac653298/`; 1505 synthetic encrypted records, wrong/correct passphrase, import, verification and history entry |
| Desktop and Engine existing checks | PASS | 197 existing tests with race across five packages, using canonical `TMPDIR=/private/tmp` |
| Windows amd64/ARM64 and Linux amd64 compilation | PASS | Production Desktop cross-builds; no native execution claim |
| Fixed lint | PASS | golangci-lint v2.15.0 with Go 1.27.3; changed packages |
| Documentation check | PASS with warnings | The four updated architecture pages now verify implementation commit `51ee83471`. Three unrelated stale verification warnings remain. The initial pre-commit `-bump` refusal was resolved by the local implementation commit |

The first macOS native history run `20261120T035013Z-7a95cf83` passed business assertions but failed cleanup. Its close callback prevented application shutdown. The same native case passed after adding a quit-in-progress guard. The failure report remains local evidence; it is not counted as a pass.

## Reproduction

```sh
make desktop-e2e CASE=desktop-processes
make desktop-e2e CASE=desktop-lightweight-close
make desktop-e2e CASE=quick-panel-contract
make desktop-e2e CASE=history-search-copy
make desktop-e2e CASE=setup-onboarding
make desktop-e2e CASE=desktop-native-processes
```

Each canonical run freezes source, isolates profiles and synthetic content, and emits `index.html`, `manifest.json`, case receipts and `SHA256SUMS`. Native cases retain real window-content screenshots and continuous recordings. Private source snapshots, binaries, profiles and logs are removed only after task-owned process cleanup. No fixture proves a native tray click, OS hotkey delivery or cross-application paste.

## Windows coordination

The authorized Parallels VM is `32affa85-5174-4d5b-ab6b-903a06d7fa9b`, Windows 11 ARM64. The memory research task completed and explicitly handed over the suspended guest. Full06 used ARM64 binaries built from clean product commit `ce4a0a8cf07fc15b404f20352d08406286261c04`. It did not use x64 emulation or the user's installed application. Private task profiles, synthetic content and the directory clipboard board isolated all runs. No VM clipboard/sharing, installation or permission changes were made.

Full01/Full02 reproduced an invisible cold panel. Windows applies `STARTF_USESHOWWINDOW/SW_HIDE` to the first ShowWindow call. Role spawning had inherited the daemon launch policy. The correction uses console-free UI process creation without the service display override. The daemon retains its separate launch policy.

Full04 passed lifecycle, native visibility and foreground checks, but its panel crops were taken before first paint. Full05 added the existing search-field tree, automation idle barrier, app frame and DwmFlush before native capture. Its first panel rendered correctly, but a later menu capture lost foreground; that whole run remains FAIL. Full06 added an observational menu-foreground wait, retained every ownership guard, and passed all five scenarios. Four native panel crops now show rendered controls. No unknown business input was replayed.

Full06 records standalone S0 PID `5524`, owned-then-standalone S1 PID `4684`, and fresh owned S2 PID `6372`, each with kernel creation time and service generation. S0 survived GUI closes and Quit. S1 kept its PID/generation through lightweight release and later Quit. Fresh native Quit stopped S2. Normal GUI PID `10668` exited; the tray menu opened a new GUI PID `12012`. Tray PID `11084` and panel PID `9616` were distinct. Later lightweight closes ended every UI role. Driver cleanup then stopped only its own preserved S0/S1 generations.

All task processes, profiles and the guest stage were removed. The VM returned to its handed-over `suspended` state. See the compact [updated receipt](baselines/desktop-lightweight-close-20261120/summary.json), [replay instructions](baselines/desktop-lightweight-close-20261120/README.md), and [checksums](baselines/desktop-lightweight-close-20261120/SHA256SUMS). Local native images and original receipts remain under `.build/windows-native-b973-v2/outer/guest-artifacts/full-06/`; source and binary staging is removed after validation.

## Remaining evidence

Native Windows ARM64 tray clicks, global-hotkey delivery, rendered panel visibility and target focus restoration passed. Actual target insertion remains NOT_RUN. System clipboard, real paste, login execution/permission prompts, native x64 and formal installer replacement remain NOT_RUN. Official Desktop staging is BLOCKED by the existing accepted CLI lock; it requires a matching accepted CLI artifact, as recorded in the [lock receipt](baselines/desktop-cli-lock-20261120/README.md). No production publication, push, PR or merge is part of this task.

A compact [identity and cleanup receipt](baselines/desktop-processes-20261120/summary.json), [replay script](baselines/desktop-processes-20261120/replay.sh) and [checksums](baselines/desktop-processes-20261120/SHA256SUMS) are retained without programs, source bundles, databases or raw logs.
