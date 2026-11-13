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
