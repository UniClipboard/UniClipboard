# Desktop native integration acceptance

Status: **INCOMPLETE**. A native hidden-window capture failure remains open. This receipt does not close execution-plan group D.

## Source and isolation

- Baseline: `28e1c5a93fb908a394034ff84d9086ef638f0300`.
- Branch: `feat/desktop-native-acceptance`.
- Worktree ID: `5a4a3767-82a1-4041-96f7-9bac84b56418`, independent of the source session.
- Origin: `https://github.com/UniClipboard/UniClipboard.git`.
- The native runs used uncommitted source. Each accepted canonical run binds its own frozen source-file and executable hashes. A baseline commit alone does not identify the tested dirty source.
- All acceptance content, profiles, documents and processes were task-owned. The native acceptance did not access the host clipboard or personal history, change production configuration, or publish/merge a release. Subsequent PR delivery was separately authorized by the user.

Detailed VM identities and local evidence are under the ignored `.build/desktop-native-d/`. This document uses role names rather than publishing local addresses or machine IDs.

| Role | Platform | Initial state | Execution condition |
|---|---|---|---|
| `fedora-qemu` | Fedora 44 Workstation, arm64, KDE Wayland, 16 GiB | Stopped | UTM disposable mode; user SPICE clipboard agent stopped before synthetic clipboard writes |
| `arch-vz` | Arch Linux ARM, arm64, 4 GiB | Stopped | Apple virtualization backend; disposable mode and UTM guest input unavailable; GDM login pending |
| `ubuntu-vz` | Configured name Ubuntu Arm 20.04; actual guest OS/session unverified, 4 GiB | Stopped | Started for X11 inventory; no matching observed DHCP/ARP identity or working exact-window capture |
| `macos-prl` | macOS guest, arm64, 4 CPUs, 6 GiB | Stopped | Guest launch failed with a security-information error; exact guest OS version unavailable |
| Windows | No authorized machine | NOT RUN | A suspended Windows VM was observed and left untouched |

## Confirmed product correction

With automatic cleanup disabled, the production storage page left age, count and excluded-tag controls enabled. This violated the accepted cleanup behavior. The native `storage-cleanup-settings` scenario reproduced it before the product change. The correction disables those controls while preserving their saved values. The independent cache quota stays editable.

The same native scenario then passed enable/disable transitions, application restart and reopened settings. The implementation and UI-state contract are in the [desktop architecture](../architecture/desktop-runtime.md#settings). No unit test was added after the fix.

## Native evidence

Paths below are local, ignored artifacts. Native frames capture window content. Physical guest input is recorded separately by the UTM adapter. Each canonical report includes hashes and cleanup results.

| Run | Result | Exact scope |
|---|---|---|
| `20261119T164108Z-ecb5b9e3` | PASS | Fedora real text clipboard: startup baseline excluded; fresh Chinese text captured; history application through physical Enter; exact clipboard read-back; no echo; private cleanup passed |
| `20261119T164226Z-008102ff` | FAIL | Native storage controls remained enabled while cleanup was off; failure screenshot and continuous recording retained |
| `20261119T164453Z-bf7ca423` | PASS | Same storage scenario after the product correction; rules disabled/enabled correctly; choice and control state survived Desktop restart; private cleanup passed |

Additional native results:

| Run | Result | Exact scope |
|---|---|---|
| `20261119T175050Z-544e1ea6` | PASS | Physical Enter applies text; standalone GNOME Text Editor receives physical Ctrl+V; AT-SPI text equals the source; physical Ctrl+S saves the exact fixture; KDE Spectacle captures the focused guest target window; cleanup passes |
| `20261119T180336Z-702cb4e9` | PASS | Wayland panel reports unsupported automatic paste; Enter preserves the clipboard; physical Ctrl+C applies exact text; reopened panel dismisses through physical Escape; cleanup passes |
| `20261119T180447Z-d1324736` | PASS | External HTML, PNG and file capture; exact stored bytes; selection and physical Enter restore after another offer replaces the clipboard; external MIME read-back matches; no echo; original file remains intact |
| `20261119T180604Z-3b50ff14` and `20261119T182422Z-1297d9f7` | FAIL | Same physical Alt+F4 and fresh-copy input reproduced twice; the main window is hidden, external text is exact, but production history remains empty for 20 seconds; both cleanups pass |
| `20261119T180750Z-e0b99580` | PASS | Two production services pair through exact fingerprint/revision approvals; peer text reaches the real Fedora clipboard and remote history without sender echo; only one endpoint is graphical |
| `20261119T180911Z-207418a4` | PASS | Physical main-window close followed by second launch reopens the original window; second process exits; service generation remains unchanged; fresh capture works after reopening; cleanup passes |
| `20261119T181102Z-4809ebdd` | PASS | Actual Linux skin, 600x480 light appearance, Chinese text, physical search focus, accessible search and command menu; no IME assertion |
| `20261119T181132Z-ebd2ec3e` | PASS | Same native Linux scope in dark appearance |
| `20261119T182151Z-a76f6867` | PASS | Standalone service receives while Desktop is offline; reopening retains history and preserves the actual clipboard baseline; new peer text then applies without echo; normal ownership retires the standalone process |
| `20261119T181813Z-ece86fe7` | PASS | Real received-file URI and bytes; excluded tag protects history/file; exclusion removal deletes history while clipboard holder keeps file; clipboard replacement releases holder and reclaims managed file; original user source remains intact |

The target editor is GNOME Text Editor 50.0 with GTK 4.22.4 and GtkSourceView 5.20. Its default implicit trailing newline is represented by a separate exact saved-byte fixture. Accessible inserted text must still match the original clipboard bytes exactly. The target screenshot is an actual guest OS-window capture. Other mygo checkpoints and continuous recordings capture the running unic window content only.

KDE exposes the HTML markup itself as a text/plain alternative. The format scenario compares that alternative against an independent external OS read. It does not infer plain-text stripping or rich rendering in a receiving editor.

Earlier unattended HTML API-restore attempts (`20261119T175820Z-b188d4eb`, `20261119T180020Z-a5f35d7b`) failed external read-back or returned a clipboard-write failure. The normal physical-Enter path passes. The unattended path's native-input prerequisites and root cause remain unresolved; it is not accepted by the later UI result. The managed-cleanup predecessor failures used an incorrect HTTP-status assertion; the client exposes `not_found` through its error reason, and the captured failure inventory already showed eligible history removed. The corrected same-input scenario verifies the subsequent holder-release and physical-reclamation steps.

Earlier failures are not discarded: an initial runner build-path error, a private migration-discovery error, locked-session frames and a semantic-input clipboard application failure remain failed attempts. The unlocked physical-input run replaced none of those historical verdicts. The recorder now supports the distribution's OpenH264 encoder and appends recording errors to the primary scenario failure.

## D matrix

| Requirement | Fedora Wayland | Arch | macOS guest | Windows |
|---|---|---|---|---|
| Text capture, explicit application, startup baseline, no echo | PASS for the recorded single-desktop loop | NOT RUN | NOT RUN | NOT RUN |
| Actual target-application insertion | PASS: exact editor text, saved bytes and OS-window screenshot | NOT RUN | NOT RUN | NOT RUN |
| HTML, PNG and file clipboard capture/application | PASS: exact payload roundtrip; receiver rendering/insertion NOT RUN | NOT RUN | NOT RUN | NOT RUN |
| Two native endpoints and disconnected backlog | PASS for peer text to one native clipboard and offline backlog preservation; two graphical endpoints NOT RUN | NOT RUN | NOT RUN | NOT RUN |
| Quick-panel global activation and focus/paste race matrix | PASS for Wayland refusal, Copy and Escape; global portal activation NOT RUN; supported-platform target race matrix unavailable here | NOT RUN | NOT RUN | NOT RUN |
| Native permission denial, secure input and hotkey conflicts | NOT RUN | NOT RUN | NOT RUN | NOT RUN |
| Window/tray/background/login lifecycle | FAIL: hidden-window capture; PASS: second launch and explicit Quit; tray/login/background startup NOT RUN | NOT RUN | NOT RUN | NOT RUN |
| Cleanup control state and persistence | PASS for the recorded storage scenario | NOT RUN | NOT RUN | NOT RUN |
| Native file/tag/holder reclamation and user-file protection | PASS: received managed-file count cleanup, excluded tag, native clipboard holder release and preserved source; custom receive-directory scenario NOT RUN | NOT RUN | NOT RUN | NOT RUN |
| UI Kit platform matrix | PASS for native Linux narrow light/dark search/menu/Chinese display; IME, materials and assistive-technology matrix NOT RUN | NOT RUN | NOT RUN | NOT RUN |
| Linux X11 | NOT RUN: no installed X11 desktop session in the inspected guests | NOT RUN | N/A | N/A |

Wayland automatic-paste refusal and Copy fallback have executed native evidence above. Portal binding approval and global shortcut activation remain NOT RUN. The native target race matrix requires a platform with supported target capture/injection. The inspected Wayland session does not provide that capability.

## Regression and evidence boundaries

- `quick-panel-contract` process regression passed in `20261119T065846Z-a20bc080`. It uses OS fixtures and a directory clipboard.
- `event-wakeup` process regression passed in `20261119T165415Z-114acb9b`. It does not prove native input or frame policy.
- Full ordinary canonical regression passed 48/48 in `20261119T165707Z-060095c1`. All 909 checksum entries verified; all 13 recordings fully decoded. It excludes guest-only cases. The 283 untagged production source/embedded files match the tested snapshot. This is not macOS native system-clipboard evidence.
- The acceptance-time documentation check passed with eight freshness warnings. Its verification-marker update refused uncommitted scoped source. PR delivery subsequently permits committing the reviewed change and updating architecture verification metadata; this does not turn the unresolved native cases into passes.

## Replay and remaining conditions

Use the [canonical Desktop guide](../../app/desktop/e2e/README.md#explicit-disposable-guest-acceptance). The UTM adapter calls `make desktop-e2e`; it does not provide an alternative acceptance suite. It verifies that the SSH guest's network identity matches the selected UTM VM before delivering input. It never retries an input request with an unknown outcome.

The target case requires an unlocked QEMU Wayland guest, disabled guest/host clipboard sharing, GNOME Text Editor, AT-SPI and KDE Spectacle. A native case holds a standard temporary idle inhibitor only during execution. It never unlocks a session or changes lock settings.

macOS is stopped with its original shared-clipboard setting enabled. The unrelated Windows VM remains suspended. Fedora and Arch returned to their initial stopped state through normal guest shutdown requests; Fedora required the logged-in KDE desktop's normal logout-and-shutdown interface after an SSH power-off request refused interactive authentication. No permissions were changed. The Fedora task workspace was removed after making only its owned Go-module cache directories writable. All owned Desktop/service/editor/clipboard-provider processes and the temporary idle inhibitor had exited before shutdown. Fedora ran in disposable mode, so no guest setting change was saved to its base disk.

Ubuntu remains started. Its actual OS/session is unverified, no matching SSH identity was found, and a normal UTM shutdown request did not stop it. Manual shutdown or explicit permission for forced power-off is pending. No forced power-off was performed. This is an incomplete VM-state restoration, not a successful cleanup verdict. Detailed receipts remain in the ignored artifact directory.

## Open failures and unexecuted conditions

- Hidden-window fresh capture fails twice on Fedora KDE Wayland. Its root cause and correction remain open. Reopening passes independently. No Engine or clipboard-backend change was made to mask this failure.
- Unattended HTML API restore has failed native evidence. UI-driven physical Enter passes; its input prerequisites and root cause remain unresolved.
- macOS native cases require the provided guest to boot past its security-information error and provide a usable graphical login. Host personal clipboard access was not used as a substitute.
- X11 needs a verified X11 graphical session. Arch has only inspected Wayland sessions; Ubuntu's actual session is unknown. Neither provides current X11 acceptance.
- Global shortcut/portal approval and conflicts, supported-platform target races, native tray interaction, actual login-start execution, IME and broader assistive-technology acceptance remain NOT RUN. These gaps are not closed by the 48 ordinary cases or the Wayland Copy fallback.
- The native managed-file case protects the task-owned original source; it does not establish a custom received-directory scenario.
- Windows native acceptance is NOT RUN because no Windows machine was provided for this task. Cross-compilation is only a build check.

The current canonical catalogue has 59 cases: 48 ordinary isolated cases and 11 explicit guest cases. Fedora has ten passing guest cases and one failing guest case, plus the passing ordinary native storage-control regression. The failed hidden-window case was reproduced twice. The same source also retains earlier failed fixture/precondition attempts; those historical verdicts are unchanged.
