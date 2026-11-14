# v2 package channel implementation receipt

- Date: 2026-11-13.
- Status: partial implementation; the full execution plan is not complete.
- Plan: [package channel parity](0018-v2-package-channel-plan.md).
- Accepted policy: [decision 0025](../decisions/proposed/0025-desktop-package-identity-and-replacement.md).
- Baseline: `1a6e8d617a8719b575a0c5bf9a6eec1060d730e5`; implementation checks include the changes described by retained source-file hashes.
- Implementation commit: `c0e1b63c7` (local; not pushed). Architecture verification markers reference this committed source.

## Implemented behavior

- Exact accepted CLI manifest/package/executable verification before Desktop staging; immutable private CLI/service files, production bundle layout and source-change rejection.
- Native identity inspection without host initialization, rejection of existing output directories, CLI signing before archive hashing and Desktop-only signing entrypoints.
- DEB/RPM/pacman binary package, macOS DMG/application archive, Windows NSIS/portable ZIP and AppImage/update archive construction entrypoints.
- Real package lifecycle receipt attachment bound to target, format and package digest; signed pacman repository helper requires accepted production input.
- Six-target CLI/Desktop metadata and a Windows arm64 runner mapping. Declaring a target does not establish native acceptance.
- Portable data roots with explicit-profile precedence; development single-instance/data isolation; private bundled CLI file ownership.
- Twelve-channel definition renderer, source AUR recipe, exact artifact hashes, fixed destination URLs and explicit pending installation/publication states.
- Non-publishing Desktop build and six-target process-acceptance workflows. No hosted run was dispatched.

## Observed validation

| Check | Result | Evidence and limits |
|---|---|---|
| Existing `go test -race ./...` | Passed: 782 tests in 70 packages | Initial sandbox execution failed on local socket binds; the elevated rerun passed. No GUI acceptance ran. Subsequent packaging-only signing changes are covered by packager compilation/process checks, not a claim of real signing. |
| Existing CLI release/upgrade E2E | Passed: 45 checks | `.cache/release-package-regression/report.json`; real local npm/Go processes with loopback/command fixtures. No public npm or GitHub publication. |
| macOS arm64 staging/portable/lock faults | Passed | `.cache/package-final-native/report.json`, `.cache/package-portable-after/report.json`; no GUI. |
| Actual macOS arm64 DMG/application archive | Passed construction | `.cache/package-macos-final/report.json`; sandbox hdiutil failed, elevated non-mounting construction passed. Unsigned; no notarization or installation. |
| Actual Windows amd64/arm64 PE + EXE/ZIP | Passed cross-construction | `.cache/windows-package-e2e/report.json`; Go cross-compiler and NSIS 3.13. No Windows execution or installer lifecycle. Development fixture staging does not establish a production CLI lock. |
| Actual Linux arm64 DEB/RPM/pacman formats | Passed construction | `.cache/package-linux/receipt-formats/report.json`; native arm64 Docker container, not a physical desktop. |
| Linux arm64 DEB install/reinstall/upgrade/remove | Passed nonvisual lifecycle | `.cache/package-linux/receipt-formats/linux-lifecycle/report.json`; real build-1 to build-2 upgrade, private command ownership, temporary-service shutdown and preserved user data. Receipt attachment checks exact package hashes. |
| Twelve-channel renderer | Passed process orchestration | `.cache/channel-e2e/report.json`; synthetic artifact bytes exercise coverage, hash checks and conflicting-input rejection. No package-manager install or upstream validation is established. |
| Python, workflow and whitespace checks | Passed | Ruff, Python compilation, actionlint and `git diff --check`. |

Linux container acceptance freezes a task-owned source snapshot. Its Git SHA identifies that snapshot, not the original worktree commit. `source-origin.json` records the original baseline; report source hashes identify the tested files.
No actual v1 personal profile, clipboard, identity or signing secret was read. No Desktop GUI, browser or new VM was launched.
Temporary binaries/packages, source snapshots and downloaded tools are removed after validation; small reports/checksums remain under `.cache/`.

## Channel closure state

| Channel | Implemented preparation | Required remaining evidence |
|---|---|---|
| Homebrew Tap | CLI Formula and Desktop Cask renderer; existing external names inspected | Brew audit/install/upgrade/remove; actual immutable URLs and upstream update. |
| Official Cask | Same accepted-DMG definition generation | Official validation, installation, submission and merge. |
| Scoop | Two-architecture portable manifest and persistence configuration | Native Windows install/upgrade/service/relocation acceptance and submission. |
| Chocolatey | nupkg definition and install/uninstall scripts | choco pack/install/upgrade/remove, Windows certificate acceptance and moderation. |
| WinGet | Version/installer/locale definitions | Native schema/installer lifecycle checks and upstream acceptance. |
| AUR | Source `uniclipboard-git` PKGBUILD plus binary recipe generation | Real CLI lock, clean source builds, install/upgrade/remove and submission. `.SRCINFO` was generated by real makepkg in an offline x86_64 container (emulated on arm64); this is metadata validation only. |
| pacman repository | Real binary format construction and signing helper | Native x86_64 install, trusted/rejected keyring transactions, accepted production input and signed repository read-back. |
| COPR | Exact-input RPM spec renderer | SRPM/mock/COPR builds, dependency/ownership comparison and repo availability. |
| Snap | Architecture-specific strict definitions with persistent app data root | Snapcraft build, real snapd refresh/confinement/capabilities and Store state. |
| Nix | Fixed-input derivation/source definitions | Nix build, native launch, wrapper/daemon/GC/autostart acceptance and upstream submission. |
| Flatpak | Architecture-specific manifest/AppStream definitions | flatpak-builder, metadata/screenshots, sandbox clipboard/IPC/network/file acceptance and Flathub review. |
| npm | Existing controller retained; Windows arm64 added to declared matrix | Six native hosted targets, bootstrap/trusted publisher for the new platform, exact public integrity and channel read-back. |

## Release gates and unfinished implementation

The metadata still contains an unresolved Desktop CLI lock. A live read of `@uniclipboard/cli@2.0.0-build.1` returned E404 on this date. Local native fixtures are not a substitute for selecting and publishing the required complete CLI build.
`build-desktop.yml` intentionally rejects the unresolved lock. It currently constructs unsigned package evidence; signed/notarized release assembly is not complete.

No valid local code-signing identity was available. GitHub Secret **names** indicate existing Apple, Minisign and channel configuration; values and validity were not inspected. No configured production Windows signing identity was established.

The fixed FlareRelease source `953a87d9890065260c338a78d8298c89c878eb35` now contains build-number identity and `/api/contract` (`product-version-build-v1`). This supersedes the earlier source-gap assumption, but does not verify the deployed authenticated contract.
The legacy registration helper still uses a version-only SQL path; do not reuse it for numbered builds.
The connected-machine inventory exposed only the current macOS host online; it did not provide a Windows/Linux native acceptance host. No local R2/Cloudflare Access service credentials were configured. Desktop R2 publication, registration/promotion, updater integration, submission workflows and public destination read-back remain unfinished.

The first available real Linux lifecycle target was arm64 rather than the plan's preferred amd64. Keep amd64 acceptance pending; do not shrink the declared matrix.
Native GUI/system clipboard, actual v1 installer replacement and sandbox acceptance remain separately required. Their authorization and environment requirements have not been satisfied by nonvisual construction.
Do not archive 0018 or 0025, mark this plan complete or announce twelve published channels from this receipt.

## Hosted follow-up

Draft PR #2247 targets v2 from the independently authorized `feat/v2-package-release` branch. The current base includes the unrelated merged Expo shell; it remains unchanged by packaging work.
Initial Build CLI run 43407479757 passed native executable checks on all six targets. It is construction evidence, not the selected signed release candidate.
Initial package process run 43407479802 exposed Windows npm.cmd launch failure and an unlocalized Linux lifecycle assertion. Windows now uses npm's JavaScript entrypoint through Node; the Linux process trace identifies the exact assertion before any behavior change.
A separate explicit distribution build imports the repository's Apple certificate for actual signing validation. No public npm/installer publication or stable-channel mutation is performed by these checks.

## Authorized CI follow-up

Draft PR: [2247](https://github.com/UniClipboard/UniClipboard/pull/2247). The approved scope is the isolated branch, draft PR and acceptance CI. Public package publication, stable-channel mutation and merge are excluded.

Canonical distribution CLI run [43408274394](https://github.com/UniClipboard/UniClipboard/actions/runs/43408274394) passed all six native executable jobs. Both macOS targets imported the existing Developer ID identity and signed the CLI pair before archive hashing. The validated manifest SHA-256 is `086222adec4588a2b3a9ff1e134d9bae35d67b4a6df475956fa19ad58660b588`. Desktop now locks CLI `2.0.0 (1)` and this exact run. This evidence does not establish npm publication or Windows production signing.

Process run [43408815344](https://github.com/UniClipboard/UniClipboard/actions/runs/43408815344) passed Windows amd64/arm64 and macOS amd64/arm64. Both Linux jobs reached the immediate socket-removal assertion after listener retirement. The follow-up now waits at most 15 seconds for Engine shutdown cleanup. A passing rerun is required.

The local signed updater process fixture passed 16 assertions on macOS arm64. It exercised the production HTTP adapter, raw Ed25519 verification, signed identity binding, actual mygo bundle replacement, owned-file removal and synthetic user-data preservation. It used a disposable key and directory. It did not launch a GUI or use the production feed.

## Native and controller follow-up

The canonical native-window case `make desktop-e2e CASE=history-search-copy` passed on macOS arm64. Its frozen-source report is `.build/desktop-e2e/20261113T105049Z-118e1356/index.html`; the manifest and SHA256SUMS bind synthetic screenshots, continuous recording and assertions. This is application-native interaction evidence. It does not establish system clipboard delivery, OS focus or package-manager installation.

The release transport fixture passed nine process assertions. It uses actual boto3 S3 requests and real release subprocesses with disposable HTTP endpoints. It checks the full 22-artifact catalogue, pre-write contract/development rejection, conditional create-only writes, complete byte read-back, immutable retry, conflict/racing-writer rejection and revision-bound promotion. Production R2 credentials, Cloudflare Access authorization and public availability were not exercised.

Desktop run 43411974805 passed both Linux and both Windows construction jobs. Both macOS targets passed Developer ID signing and constructed a notarized DMG. A repeated submission/stapling path failed while deriving the second archive. The next implementation notarizes a derived stage once, then constructs both formats from that accepted stage. The follow-up run must pass before marking the complete macOS candidate accepted.

The sandbox harness and two-architecture CI jobs are defined for actual Nix, Flatpak and strict Snap construction and execution. These jobs must run; recipe generation is not sandbox acceptance. Production Windows signing, the durable raw Ed25519 update key and public package/channel operations remain pending.

The native recording contains 8 continuously sampled frames over 9.36 seconds. Cleanup passed. The directory clipboard is synthetic; system clipboard delivery remains NOT_RUN.

The complete existing Go race suite passed with `TMPDIR=/private/tmp GOCACHE=/private/tmp/unic-followup-go-cache go test -race ./...`. An earlier invocation used the macOS `/var` temporary-directory alias and failed the existing Engine no-symlink cache guard. A targeted replay with the canonical path passed before the complete rerun. No production cache guard was weakened.

## Acceptance corrections from real hosts

Run 43413805519 reported success for all jobs and verified macOS signing/notarization. Subsequent NSIS lifecycle run 43414455024 exposed a Windows mixed-separator `File` path error that the earlier PowerShell multi-command step did not propagate. Therefore the earlier green Windows jobs establish stage/portable/update construction only; they do **not** establish a complete NSIS package. The constructor now uses the native path separator, PowerShell propagates native exit codes, and candidate acquisition requires all 22 target/format records before accepting a successful run.

First sandbox run 43414689412 failed. Nix/Flatpak could not resolve relative candidate paths from the generated project directories. Snapcraft could not install build snaps as the non-root runner. The follow-up uses canonical absolute input paths and runs Snap's disposable-host build with the required privileges. These failures do not count as sandbox acceptance.

Production public-service probes returned HTTP 404 for `/contract` and `/v1/desktop/stable.json` with the application User-Agent. A default Python request was blocked with HTTP 403 before reaching the same route. The updater/release client now identifies itself explicitly. Both admin and public numbered-build contracts must pass before publication writes. Public Worker deployment remains pending under separate authorization.

Run 43415678890 passed all six native Desktop construction jobs. Both Windows targets passed real NSIS installation, same-build reinstallation, later-build upgrade, removal, temporary-service shutdown, private CLI ownership and synthetic user-data preservation. Both macOS targets passed Developer ID signing, Apple notarization and ticket stapling. Candidate acquisition now checks the complete 22-artifact matrix, including actual NSIS files.

Run 43416567154 passed Flatpak and strict Snap build/install/executable acceptance on both Linux architectures. Both Nix builds completed but their generated executable wrappers exited with SIGSEGV. A disposable arm64 Nix container reproduced this with the same pinned Nixpkgs. Controlled probes showed that the unmodified executable runs through the same Nix loader, while the autoPatchelf output crashes. ELF inspection showed that adding RUNPATH moved the writable Go `.dynamic` section into an executable segment. Updating only the interpreter, while retaining the existing GTK library wrapper, passed the same reproduction command. The generated Nix recipe now follows that path and preserves the locked CLI pair. Full native replay is still required.

The updated sandbox process cases additionally exercise real private CLI/service IPC and bounded shutdown. Refresh, native sandbox GUI, clipboard/file/network integration and Store submission remain separate pending acceptance.

A final dependency audit found that CLI candidate 43408274394 predates `desktop.update_channel`. Although that candidate passed native executable acceptance, its service does not have the current shared configuration schema. The follow-up selects CLI `2.0.0 (2)` rather than altering accepted build 1. Desktop retains `2.0.0 (1)`. Staging now compares target-selected shared configuration, API, lifecycle and server source hashes against the CLI dependency manifest. A mismatch fails before copying or building a Desktop stage. The new lock must pass six native jobs and macOS signing before selection.

## Final compatible CLI lock

CLI distribution run [43419452491](https://github.com/UniClipboard/UniClipboard/actions/runs/43419452491) passed all six native jobs and assembly from source `8ab86f230351ba2039af3c23b96126e765f053df`. Both macOS targets use real Developer ID signatures. Desktop now locks CLI `2.0.0 (2)` with manifest SHA-256 `ae2bd7d0cb96675d052544a4365b11319f0868bf01aea6afc01a4fac31e6c2be`.

The actual new signed macOS arm64 CLI pair passed local Desktop staging and the shared configuration/API round-trip. The Desktop stage is explicitly a development stage, not a production-signing or GUI receipt. The packager rejects the old CLI at `internal/localconfig/config.go` before staging. The process E2E also passes a changed shared-source rejection and an actual private CLI/service round-trip. Production Desktop builds set the SDK production flag independently of updater-key provisioning; development fixtures retain SDK development mode.

Sandbox run [43418866636](https://github.com/UniClipboard/UniClipboard/actions/runs/43418866636) passed all six Nix/Flatpak/strict Snap jobs. Each job built and executed the package, exercised real private CLI/service IPC and verified temporary shutdown. Nix also verified that both private CLI hashes remain unchanged. That run binds the earlier input candidate 43415678890. The final CLI-build-2 Desktop candidate must have its own native and sandbox replay.

The follow-up also defines a six-target hosted replay of the canonical native-window case. It uses the existing source-freeze/recording/cleanup runner. Windows arm64 omits race instrumentation because the pinned Go toolchain does not support it; the report records that limitation. Actual hosted results are required before claiming those native-window targets passed.

## Final paired construction and sandbox acceptance

Desktop run [43421800442](https://github.com/UniClipboard/UniClipboard/actions/runs/43421800442), source `09488aaaa20aaf297d7be5c89271cba5f40786c4`, passed all six native construction jobs with CLI build 2. It produced the complete 22-artifact matrix. Both macOS targets passed Developer ID signing, Apple notarization and stapling. Both Windows targets passed NSIS install/reinstall/upgrade/removal, registry PATH preservation, complete uninstaller removal and synthetic data preservation. Windows files remain unsigned for production.

Sandbox run [43423601595](https://github.com/UniClipboard/UniClipboard/actions/runs/43423601595), source `844bf0bb921fd8f13b29965d50c318d225c3ff63`, consumed that exact candidate. All six Nix/Flatpak/strict Snap jobs passed build/install/execution, real private CLI/service IPC and bounded shutdown. This supersedes the earlier sandbox run with the old CLI pair. It does not establish sandbox GUI permissions, refresh or Store submission.

## Native-window harness follow-up

Initial hosted native-window run `43423596312` passed Linux amd64/arm64 and macOS arm64. Windows failed in FFmpeg setup because the fixed version was outside the action provider's release index. macOS Intel started its window/service, then failed an eight-second capture RPC. No full six-target GUI acceptance is claimed from that run.

The harness now serializes only concurrent recording/checkpoint captures and retains the eight-second RPC deadline. A broader all-RPC serialization experiment did not pass and was discarded. Local canonical replay `.build/desktop-e2e/20261113T124355Z-427d0972/index.html` passed all history/search/copy assertions with seven continuous frames and cleanup PASS. Native manifests now record capture timings and actual FFmpeg/FFprobe versions and hashes. Windows FFmpeg setup uses available release `9.0.2`. A subsequent hosted Windows attempt exposed default-codepage decoding before app launch; evidence text I/O now specifies UTF-8.

Pinned `golangci-lint 2.15.0` passed with zero issues after file-close handling, import grouping and error-text corrections. The actual signed-update/mygo-install process fixture passed all 20 assertions after those corrections.

Run `43428490336` passed native-window history/search/copy on Windows amd64/arm64, Linux amd64/arm64 and macOS arm64 after explicit UTF-8 I/O. macOS Intel remained under investigation. The previous Intel report measured an eight-second first-capture timeout followed by captures of 3.04, 0.83 and 1.37 seconds. Capture serialization alone did not resolve cold startup. The harness now permits 30 seconds for only the first native capture; all later capture and semantic RPCs retain eight seconds. Local replay `.build/desktop-e2e/20261113T125459Z-61f21f19/index.html` passed with cleanup PASS. The final hosted replay must still pass before claiming six-target native-window acceptance.

## Six-target native-window assertion replay

Run [43429333711](https://github.com/UniClipboard/UniClipboard/actions/runs/43429333711), source `01bba22de7524ed0649711546c2df8e3abaa8dcc`, passed the canonical native-window history/search/copy case on macOS amd64/arm64, Windows amd64/arm64 and Linux amd64/arm64. Every target produced source/binary hashes, semantic assertions, screenshots, a continuously sampled playable recording and an owned-process/private-state cleanup receipt. The byte-verified Unix report measured a 14.63-second first capture on macOS Intel. This validates the separate startup bound on the previously failing host. Windows arm64 explicitly records unavailable race instrumentation.

These are source-frozen application-native cases with a synthetic directory clipboard. They do not establish production package-specific system clipboard, focus, cross-application paste, sandbox GUI permissions, package refresh, real v1 replacement or Store availability. The production installer candidate remains run `43421800442`; the final native-window run does not rebuild or replace its immutable artifacts.

No public package or stable channel was changed. Production Windows signing, the durable raw updater key, deployed public numbered contracts and separately authorized publication remain required. Temporary downloaded package binaries and Nix diagnosis binaries/source snapshots were removed after retaining small checksummed receipts.

Post-download verification found that the Windows manifests in run `43429333711` use CRLF while their expected SHA-256 values were calculated over LF JSON bytes. The four Unix-target reports verified fully. Both Windows manifests match their expected checksum only after CRLF-to-LF conversion; their original artifacts therefore fail byte verification despite passing scenario assertions. The runner now publishes JSON as explicit UTF-8 bytes and verifies the final manifest digest after its atomic write. The six-target replay must regenerate accepted evidence; do not silently modify the downloaded reports.

Run `43431039759` verified all six downloaded SHA256SUMS, including both corrected Windows manifests. Five targets passed their native case. macOS Intel passed every history/search/copy assertion and cleanup, but its second continuous-recording capture exceeded eight seconds. This disproves the earlier first-frame-only budget assumption. Capture RPCs now have a dedicated 30-second bound, while semantic RPCs retain eight seconds and scenario conditions retain 20 seconds. Recording shutdown allows 40 seconds to join an in-flight capture. Capture duration remains visible; no operation assertion or required recording is skipped. A final replay is required.

The dedicated capture/recording-shutdown budgets passed local replay `.build/desktop-e2e/20261113T131656Z-bd7f96b7/index.html`, with unchanged semantic assertions and cleanup PASS. Pinned `golangci-lint 2.15.0` and Ruff passed after this final harness change. Production public read-back was repeated: `/contract` and `/v1/desktop/stable.json` still returned HTTP 404 with the application User-Agent.

## Final verified native evidence

Run [43432479654](https://github.com/UniClipboard/UniClipboard/actions/runs/43432479654), source `cb1749765fb824c05babd9ad490aa8b6d929a6bb`, passed all six native-window targets after the capture and byte-publication corrections. Independent verification of the downloaded artifacts passed every SHA256SUMS entry on all six targets. Every manifest reports PASS, private cleanup PASS and case cleanup PASS. Recordings contain 10 macOS Intel frames (13.08 seconds), 10 macOS arm64 frames (8.16 seconds), 15 Linux amd64 frames (8.28 seconds), 11 Linux arm64 frames (6.04 seconds), 12 Windows amd64 frames (8.64 seconds) and 14 Windows arm64 frames (3.04 seconds).

The small aggregate receipt is `.cache/package-followup/native-43432479654-receipt.json`; the original verified native reports remain under `.cache/package-followup/native-final-43432479654/`. Six-target package process run [43432489289](https://github.com/UniClipboard/UniClipboard/actions/runs/43432489289) also passed at the same source. This supersedes prior incomplete native-window evidence; it does not remove the production activation or real-OS integration limits above.

## v1 GitHub Release page migration

The migration inspected the actual [v1.6.2 page](https://github.com/UniClipboard/UniClipboard/releases/tag/v1.6.2) and the v1 template, generator and release workflow at `c6483d73c7a0234be68571fdb5fa3d38a17c4871`. v2 now uses `.github/release-notes/desktop.md.tmpl` and `tools/release/github_desktop.py`. It retains changelog/comparison links, installation and CLI tables, verification and optional contributors. It omits unsupported mobile claims and the v1 blanket signing statement.

The new workflow supports `github-draft`. Draft staging binds the exact accepted source and 32 attachments, verifies existing bytes, resumes missing assets and downloads every new upload for hash verification. A persisted page context restores comparison/contributor input during promotion. Origin publication and channel promotion first verify/stage the GitHub page. Alpha publication requires origin availability; stable page publication additionally requires verified stable promotion. All production signature gates remain enforced.

The local real-process fixture passed 18 checks using the actual `gh` executable and disposable loopback HTTP service. It verifies the v1-style body, 22 native files plus six updater signatures and four metadata/checksum attachments, immutable recovery, upload read-back, receipt gates and downgrade/conflict rejection. `.cache/github-release-e2e/report.json`, its preview and `SHA256SUMS` are the repeatable artifact. Ruff and actionlint passed. This is not live GitHub-token, real draft creation or public publication acceptance.

GitHub's current default branch is `v1`. Manual dispatch of the new workflow requires reviewed default-branch registration during the v2 rollout. No real Release page, tag, installation asset, origin object or channel was changed during this migration acceptance.

Hosted process job [131944655579](https://github.com/UniClipboard/UniClipboard/actions/runs/43442859173/job/131944655579) passed all 18 checks at source `65cb731c092e7659f286a8e73386e6c2232d30c3`. The downloaded artifact `.cache/github-release-ci-43442859173/` passed independent SHA256SUMS verification, and its recorded source-file hashes match the implementation. The other native/package jobs in that workflow remain separate results; this receipt establishes the new GitHub page process job only.
