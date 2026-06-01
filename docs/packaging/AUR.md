# AUR Packaging Plan for UniClipboard (Desktop)

> Status:
> - `uniclipboard-git` — **implemented** (2026-05-19). PKGBUILD lives at `packaging/aur/uniclipboard-git/PKGBUILD`; CI at `.github/workflows/aur.yml` syncs it to AUR on every relevant push.
> - `uniclipboard` (stable source) — still planning, blocked on `v0.10.1` stable tag (§5).
> - `uniclipboard-bin` — co-maintainer outreach to czyt (§10), no upstream-controlled PKGBUILD yet.
>
> Snapshot taken: 2026-05-19.
> Target repo: this one (the desktop client at `github.com/UniClipboard/UniClipboard`).

## 1. Background

The UniClipboard desktop client ships `.deb` artifacts on GitHub releases (currently v0.10.0). On the Arch User Repository (AUR), a third-party packager `czyt` has been publishing `uniclipboard-bin`, which simply repackages our `.deb` releases. The upstream team (us) has no official presence on AUR.

Goal of this doc: establish an official AUR presence under the upstream's control, without antagonizing the existing volunteer packager.

## 2. Current AUR State

| Package | Maintainer | Last update | Source |
|---|---|---|---|
| `uniclipboard-bin` | czyt | 2026-05-17 | repackages our `.deb` from GitHub releases |
| `uniclipboard` | (vacant) | — | — |
| `uniclipboard-git` | (vacant) | — | — |

czyt's current PKGBUILD is clean: only downloads our official `.deb`, SHA256-verified, no install scripts, no sudo/suid, no extra network calls. czyt maintains 26 `-bin`-style packages overall — a typical Chinese-community AUR volunteer, not a malicious actor. Structural risk: a single compromised AUR account would affect all 26 packages simultaneously.

## 3. Goals (priority order)

1. **Claim the `uniclipboard` name.** Prevent squatting; create a clearly-official install path.
2. **Add `uniclipboard-git` for development builds.** Standard practice for any non-trivial Arch project.
3. **Become co-maintainer of `uniclipboard-bin`** without taking it over from czyt. Earns push access for emergencies (CVE, supply-chain incident) while preserving his active maintenance.
4. **Sign release artifacts** so all three packages (including czyt's `-bin`) can eventually verify upstream signatures.

## 4. Recommended Path

Execute in this order:

1. ~~**(15 min) Register official AUR account** and push `uniclipboard-git` to claim the name.~~ **Done 2026-05-19** — account registered; PKGBUILD + CI implemented (§6). First push happens automatically once `AUR_SSH_PRIVATE_KEY` secret is added and the workflow is triggered.
2. **(5 min) Email czyt** requesting co-maintainer on `uniclipboard-bin` (template in §10).
3. **(blocked on `v0.10.1` stable) Push `uniclipboard`** source-build package targeting the next non-alpha tag. Decided 2026-05-19 to wait rather than backfill `v0.10.0` — keeps the AUR repo aligned with the active release branch and avoids a same-week version bump. Until then, Arch users on stable either use `uniclipboard-bin` (czyt) or `uniclipboard-git`.
4. **(next release pipeline change) Add `minisign` or `cosign` signatures** to GitHub release artifacts; publish the public key fingerprint in `SECURITY.md` and release notes.
5. **(after signing lands) Update all three PKGBUILDs** to verify the signature. Coordinate the `-bin` update with czyt.

## 5. Repo Facts (filled 2026-05-19 — verify before each push)

These were collected by scanning the repo on 2026-05-19. The PKGBUILDs in §6/§7 already consume these values. Re-check anything marked **HUMAN** before pushing.

### Build & runtime
- [x] **Tech stack:** Tauri 2.11 + React 19 + TypeScript + Tailwind 4 (`src-tauri/Cargo.toml:136`, `package.json:92,113`).
- [x] **Build command:** `bun run tauri build` (`.github/workflows/build.yml:268-269`).
- [x] **Package manager:** **bun** (not pnpm). Lockfile is `bun.lock`. The `bun` package is in Arch `extra`.
- [x] **`makedepends`:** `git rust nodejs bun pkgconf`. No `openssl-sys`/`libsqlite3-sys`-style sys crates spotted in workspace deps.
- [x] **`depends`:** `webkit2gtk-4.1 gtk3 libayatana-appindicator libnotify` (mapped from `src-tauri/tauri.conf.json:48-54` .deb runtime deps).
- [x] **Arches:** `x86_64 aarch64`. CI builds both for Linux (`.github/workflows/build.yml:71-86`).
- [ ] **Minimum glibc:** **HUMAN** — CI builds in `debian:bookworm` (glibc 2.36) but no documented floor. Arch ships glibc ≥ 2.39, so practically a non-issue.

### Distribution
- [x] **License:** `AGPL-3.0-only` (`LICENSE` header; `src-tauri/Cargo.toml:4`). **Note:** the initial skeleton placeholder said MIT — that was wrong; §6/§7 are now corrected.
- [x] **Release artifacts:** `.deb`, `.rpm`, `.AppImage` (Linux); `.dmg` (macOS); `.msi`/`.exe` (Windows). No upstream source tarball published — the `uniclipboard` AUR package pulls GitHub's auto-generated `archive/refs/tags/v$VER.tar.gz`.
- [x] **Signatures:** Tauri **updater** already signs payloads with minisign (pubkey embedded at `src-tauri/tauri.conf.json:64`). **But** that key signs the in-app update bundle, not the release tarball or .deb — §11 (release-artifact signing) is still required for PKGBUILD-side verification.
- [x] **Tag format:** `v$VERSION` (e.g. `v0.10.0`, `v0.10.1-alpha.1`). v-prefixed.
- [x] **systemd user service:** none. App runs as a normal GUI process started by the user.
- [x] **Desktop integration:** generic `.desktop` file lives at `packaging/linux/uniclipboard.desktop` (created 2026-05-19, content forked from `snap/local/uniclipboard.desktop`). Icons come from `src-tauri/icons/` (32/64/128/128@2x); PKGBUILD renames them into hicolor `apps/uniclipboard.png` at install. Note there are now three desktop sources: this AUR file, `snap/local/uniclipboard.desktop`, and the Handlebars template `packaging/linux/uniclipboard.desktop.hbs` consumed by the Tauri bundler for deb/rpm/appimage (wired via `bundle.linux.deb.desktopTemplate` + `rpm.desktopTemplate` in `src-tauri/tauri.conf.json`; the path is resolved relative to `src-tauri/` since `tauri build` chdirs there). All three must keep `Categories=Network;Utility;` in sync — consolidate later if any drifts.
- [x] **Config / data paths (Linux):** `$XDG_DATA_HOME/app.uniclipboard.desktop/` + `$XDG_CACHE_HOME/app.uniclipboard.desktop/` (`src-tauri/crates/uc-platform/src/app_dirs.rs:92-99`). Note the unusual `app.uniclipboard.desktop` dir name — matters for any future uninstall hook.

### Project metadata
- [x] **Homepage:** `https://www.uniclipboard.app` (confirmed in README).
- [x] **Bug tracker:** `https://github.com/UniClipboard/UniClipboard/issues`.
- [x] **Binary name:** `uniclipboard` (`src-tauri/Cargo.toml:2`). Product name `UniClipboard` is display-only.
- [x] **AUR account:** **`uniclipboard`** — `aur@uniclipboard.app` (registered 2026-05-19). Org-owned, not tied to a personal AUR identity, so continuity survives maintainer turnover.

### Version target for the `uniclipboard` (stable) AUR package
- Latest stable tag: **`v0.10.0`**. Repo is currently at `0.10.1-alpha.1` (commit `9e76963c`).
- `uniclipboard-git`: tracks `main`, version computed from `git describe`. Safe to push now.
- `uniclipboard` (stable): **blocked on `v0.10.1` stable** (decided 2026-05-19, not backfilling `v0.10.0`). When the next non-alpha tag lands, fill `pkgver=` in §7 and push.

## 6. PKGBUILD — `uniclipboard-git` (implemented)

**Source of truth:** `packaging/aur/uniclipboard-git/PKGBUILD` (+ `.SRCINFO`).
**Sync mechanism:** `.github/workflows/aur.yml` — runs on push to `main` that touches `packaging/aur/uniclipboard-git/**` or the workflow file itself, plus `workflow_dispatch` (with optional `dry_run`).

How the sync works:
1. Job runs in `archlinux:base-devel` container.
2. Checks out this repo, computes `pkgver` from `git describe` (same algorithm as the in-PKGBUILD `pkgver()` function — keeps the AUR web snapshot in sync with current `main`).
3. Clones `ssh://aur@aur.archlinux.org/uniclipboard-git.git` using the `AUR_SSH_PRIVATE_KEY` repo secret.
4. Renders PKGBUILD, regenerates `.SRCINFO` via `makepkg --printsrcinfo` as a non-root `builder` user.
5. Diffs against AUR HEAD — pushes only if something changed (no empty commits).
6. Concurrency group `aur-uniclipboard-git` serializes pushes so two CI runs never race on the same remote.

Required secret:
- `AUR_SSH_PRIVATE_KEY` — ed25519 private key. Generate via `ssh-keygen -t ed25519 -C aur-uniclipboard -f aur`, upload `aur.pub` at `https://aur.archlinux.org/account/<user>/edit/`, paste the private key (`aur`) into repo secrets.

Recommended repo variable (defense-in-depth against MITM on `ssh-keyscan`):
- `AUR_HOST_FINGERPRINTS` — comma-separated list of pinned `SHA256:...` fingerprints for `aur.archlinux.org` host keys. Configure via `gh variable set AUR_HOST_FINGERPRINTS --body 'SHA256:...,SHA256:...'`. Source from `https://wiki.archlinux.org/title/AUR_submission_guidelines` (out-of-band verification — that's the whole point). When set, the workflow `ssh-keyscan`s and then verifies at least one scanned key matches a pinned fingerprint; mismatch → fail-fast. When unset, falls back to TOFU keyscan with a warning so first-time setup still works.

Reference skeleton (kept here for documentation; the live file may have drifted):

```bash
# Maintainer: UniClipboard <aur@uniclipboard.app>

pkgname=uniclipboard-git
_pkgname=uniclipboard
pkgver=0.10.1.alpha.1.r0.g0000000
pkgrel=1
pkgdesc="Real-time clipboard sync across macOS, Windows and Linux — local-first, peer-to-peer, and end-to-end encrypted"
arch=('x86_64' 'aarch64')
url="https://www.uniclipboard.app"
license=('AGPL-3.0-only')
depends=('webkit2gtk-4.1' 'gtk3' 'libayatana-appindicator' 'libnotify')
makedepends=('git' 'rust' 'nodejs' 'bun' 'pkgconf')
provides=("$_pkgname" "$_pkgname=$pkgver")
conflicts=("$_pkgname")
source=("$_pkgname::git+https://github.com/UniClipboard/UniClipboard.git")
sha256sums=('SKIP')

pkgver() {
  cd "$_pkgname"
  git describe --long --tags --abbrev=7 2>/dev/null \
    | sed 's/^v//; s/\([^-]*-g\)/r\1/; s/-/./g' \
    || printf "0.0.0.r%s.g%s" \
         "$(git rev-list --count HEAD)" \
         "$(git rev-parse --short=7 HEAD)"
}

prepare() {
  cd "$_pkgname"
  bun install --frozen-lockfile
}

build() {
  cd "$_pkgname"
  # --no-bundle skips .deb/.rpm/.AppImage generation; we only need the binary.
  bun run tauri build --no-bundle
}

package() {
  cd "$_pkgname"

  install -Dm755 "src-tauri/target/release/uniclipboard" \
                 "$pkgdir/usr/bin/uniclipboard"

  install -Dm644 "packaging/linux/uniclipboard.desktop" \
                 "$pkgdir/usr/share/applications/uniclipboard.desktop"

  # Hicolor icons, renamed from Tauri's size-named source files.
  install -Dm644 "src-tauri/icons/32x32.png"       "$pkgdir/usr/share/icons/hicolor/32x32/apps/uniclipboard.png"
  install -Dm644 "src-tauri/icons/64x64.png"       "$pkgdir/usr/share/icons/hicolor/64x64/apps/uniclipboard.png"
  install -Dm644 "src-tauri/icons/128x128.png"     "$pkgdir/usr/share/icons/hicolor/128x128/apps/uniclipboard.png"
  install -Dm644 "src-tauri/icons/128x128@2x.png"  "$pkgdir/usr/share/icons/hicolor/256x256/apps/uniclipboard.png"

  install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
}
```

## 7. PKGBUILD Skeleton — `uniclipboard` (release source)

Same as `-git` with these differences:

```bash
pkgname=uniclipboard
pkgver=0.10.0  # update on every stable release; do NOT track alpha tags here
# Drop pkgver() function entirely.
# Drop 'git' from makedepends.
source=("$pkgname-$pkgver.tar.gz::https://github.com/UniClipboard/UniClipboard/archive/refs/tags/v$pkgver.tar.gz")
sha256sums=('<fill in via: sha256sum the-downloaded-tarball>')
# Once §11 signing lands:
# source+=("$pkgname-$pkgver.tar.gz.sig::https://github.com/UniClipboard/UniClipboard/releases/download/v$pkgver/source.tar.gz.minisig")
# validpgpkeys=()  # minisign uses sha256sums[]='SKIP' + a separate verify step, not validpgpkeys
```

GitHub's auto-generated tarball extracts to `UniClipboard-$pkgver/` (matches the repo name, `v` prefix stripped). So change every `cd "$_pkgname"` line to `cd "UniClipboard-$pkgver"` — confirm once with `tar tf` on the downloaded tarball before pushing.

## 8. Validation Before Pushing

```bash
# Lint the PKGBUILD and the built package
namcap PKGBUILD
namcap *.pkg.tar.zst

# Clean-room build — catches missing makedepends/depends
# Requires the 'devtools' package; this runs inside a fresh chroot.
extra-x86_64-build

# Final smoke test
sudo pacman -U uniclipboard-git-*-x86_64.pkg.tar.zst
uniclipboard --version
```

Don't push until `namcap` is clean and the clean-room build succeeds. The clean-room is what catches "works on my laptop because I happen to have $library installed already".

## 9. Push to AUR

```bash
# One-time, per AUR account
ssh-keygen -t ed25519 -C "aur-uniclipboard" -f ~/.ssh/aur
# Add ~/.ssh/aur.pub at https://aur.archlinux.org/account/\<username\>/edit/

# In ~/.ssh/config:
#   Host aur.archlinux.org
#     IdentityFile ~/.ssh/aur
#     User aur

# Per package
git clone ssh://aur@aur.archlinux.org/uniclipboard-git.git
cd uniclipboard-git
cp /path/to/PKGBUILD .
makepkg --printsrcinfo > .SRCINFO
git add PKGBUILD .SRCINFO
git commit -m "Initial import: uniclipboard-git 0.10.0.r0.g..."
git push
```

`.SRCINFO` must be committed alongside `PKGBUILD` on every change — AUR's web UI reads from it, not the PKGBUILD.

## 10. Co-maintainer Request — Email Template

Send to czyt (AUR profile email, or via GitHub @czyt). Replace placeholders before sending.

> **Subject:** Co-maintainer request for `uniclipboard-bin`
>
> Hi czyt,
>
> I'm the upstream maintainer of UniClipboard desktop (`github.com/UniClipboard/UniClipboard`). First, thanks for keeping `uniclipboard-bin` up to date — you tracked 0.7 → 0.10 in nine days, which is great for our Linux users.
>
> I'd like to ask if you'd add me as co-maintainer on `uniclipboard-bin`. Goals:
>
> 1. Emergency push access (CVE, broken release artifact) so I can hotfix without waiting on you.
> 2. We're about to start signing release artifacts (likely `minisign`); when that lands I can help update the PKGBUILD to verify the signature.
> 3. I'll be publishing `uniclipboard` (source build) and `uniclipboard-git` under the upstream account. You'd remain primary on `-bin`; the three packages will share `provides`/`conflicts` cleanly.
>
> My AUR username: `uniclipboard`
>
> Upstream identity proof: I'll attach a signed message to the next GitHub release notes confirming this request.
>
> Thanks for the work!
>
> — mkdir700 (UniClipboard upstream)

## 11. Supply Chain Hardening (deferred, post-launch)

Out of scope for the initial AUR push, but plan for:

- **Heads-up — there's already a minisign key in `src-tauri/tauri.conf.json:64`.** That key signs the Tauri **updater** payload (the in-app auto-update bundle), not the GitHub release tarball or .deb. For AUR verification we need a **separate** signing step over the release artifacts (or, debatably, repurpose the existing key — but mixing the two roles makes key rotation harder, so prefer a second key).
- Add `minisign -S` to the GitHub Actions release workflow. Generate a dedicated release-artifact key (`minisign -G`), store the secret key encrypted in repo secrets, publish public key in `SECURITY.md`.
- Update `uniclipboard` PKGBUILD to download `.sig` alongside tarball and verify in `prepare()`.
- Coordinate the same change into `uniclipboard-bin` with czyt (he can verify the `.deb`'s `.sig` before extraction).
- Once all three PKGBUILDs verify upstream signatures, a compromised AUR account alone is no longer enough to ship a malicious build — attacker also needs the upstream signing key.

## 12. Open Questions

- Do we want a Flatpak parallel to this AUR work? (Out of scope for this doc, but the answer affects how much we invest in Linux-native packaging.)
- Once `uniclipboard` source build is published, do we also push to `[community]`/`[extra]` via a Trusted User? (Long timeline, requires sustained AUR votes first.)
