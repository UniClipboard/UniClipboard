# nixpkgs 打包

`package.nix` 是提交到 [nixpkgs](https://github.com/NixOS/nixpkgs) 的 derivation 源，保存在仓库内（与 `packaging/aur/` 同一惯例）。真正生效的是 nixpkgs 仓库里的那份副本，本目录只是源 + 提交说明。

## 为什么优先做 nixpkgs

Repology 抓取 nixpkgs 的 6 个 stable channel + unstable。**一次合并的 PR ≈ Repology packaging-status 徽章 +7 行**，是所有分发渠道里性价比最高的。

> 注意：Repology 不抓 Snap Store / COPR / 自建 Homebrew tap。当前 CI 已自动发布的那几个渠道对徽章没有贡献，nixpkgs 才是第一个能让徽章变丰富的渠道。

## 方案：从 AppImage 二进制重打包

本 derivation **不从源码编译**，而是用 `appimageTools.wrapType2` 包装官方发布的 AppImage。原因：UniClipboard 是 Tauri 应用（Rust workspace + bun 前端 + vendored `iroh-blobs` + sidecar `uniclipd`），在 Nix 沙箱里从源码构建需要 fixed-output 的 bun 依赖、带 git 源的 `cargoLock`、双二进制产物，落地和维护成本都很高。二进制重打包是 nixpkgs 对这类应用的常见、被接受的做法。

代价：部分 reviewer 偏好源码构建。若被要求，迁移路径见文末「迁移到源码构建」。

## 提交步骤

1. **fork 并 clone nixpkgs**
   ```bash
   git clone https://github.com/<你的账号>/nixpkgs
   cd nixpkgs
   git checkout -b uniclipboard-init
   ```

2. **放置文件**（nixpkgs 的 `by-name` 结构，按首两字母分目录）
   ```bash
   mkdir -p pkgs/by-name/un/uniclipboard
   cp <本仓库>/packaging/nix/package.nix pkgs/by-name/un/uniclipboard/package.nix
   ```

3. **算 hash**（`package.nix` 里目前是 `lib.fakeHash` 占位）

   本仓库环境没有 nix，无法预先算好。在任意装了 nix 的机器上二选一：
   ```bash
   # 方式 A：直接 build，把 Nix 报错里的 "got: sha256-..." 填回 package.nix
   nix-build -A uniclipboard

   # 方式 B：提前 prefetch
   nix store prefetch-file --hash-type sha256 \
     https://github.com/UniClipboard/UniClipboard/releases/download/v0.15.0/UniClipboard_0.15.0_amd64.AppImage
   ```
   该 hash 也可与 release 自带的 `SHA256SUMS.txt`（minisign 签名）交叉核对。

4. **本地验证**（必做——本仓库环境无法替你跑）
   ```bash
   nix-build -A uniclipboard          # 能 build
   ./result/bin/uniclipboard          # 能启动、托盘正常、能配对同步
   nix-shell -p nixpkgs-review --run "nixpkgs-review wip"
   ```
   若启动报缺 `.so`，把对应包加进 `package.nix` 的 `extraPkgs`。若 `extraInstallCommands` 里 desktop/icon 路径对不上，`ls` 一下 `appimageContents` 调整。

5. **填 maintainer**：把自己加进 `maintainers/maintainer-list.nix`，再写进 `package.nix` 的 `meta.maintainers`。也可以不当 maintainer 直接提，但有人维护更容易被合。

6. **提交**（nixpkgs 的 commit message 规范）
   ```bash
   git add pkgs/by-name/un/uniclipboard maintainers/maintainer-list.nix
   git commit -m "uniclipboard: init at 0.15.0"
   git push -u origin uniclipboard-init
   ```

7. **开 PR**：base 选 `NixOS/nixpkgs:master`，勾选 PR 模板里跑过的测试项（至少 `nix-build` + 实际运行）。ofborg 会自动在多平台构建。

8. **合并后**：约一个 channel 周期（unstable 几天、stable 跟随下次 release）后，Repology 会自动抓到，徽章多出 `nixpkgs unstable` 等行。

## 后续版本维护

合并后升级很轻量：改 `version` + 重新算 `hash` 即可。可让 nixpkgs 的 `nixpkgs-update` 机器人自动跟随 GitHub Release，无需每次手动提 PR。

## 待办 / 已知缺口

- **本环境未验证**：本仓库无 nix，`package.nix` 是经过事实校对（URL、版本、license、元数据均来自仓库真实值）的草稿，但 **build 与运行必须由你在有 nix 的机器上验证**。已知需现场核对的点都在 `package.nix` 注释里标了。
- **仅 `x86_64-linux`**：首版只覆盖 amd64。aarch64 的 AppImage 资产存在（`UniClipboard_0.15.0_aarch64.AppImage`），但 `appimageTools` 对 aarch64 支持较弱，建议先合 x86_64，之后用「解 `.deb` + `autoPatchelfHook`」方式补多架构。

## 迁移到源码构建（仅在 reviewer 要求时）

大方向：`rustPlatform.buildRustPackage` + `cargoLock.lockFileContents`（含 vendored `iroh-blobs` 的 `outputHashes`）+ 用 `stdenvNoCC` 预构建 bun 前端为 fixed-output derivation，再 `nativeBuildInputs` 加 `wrapGAppsHook4 pkg-config`、`buildInputs` 加 `webkitgtk_4_1 libsoup_3 libayatana-appindicator`。本 `meta` 与 desktop/icon 处理可直接复用。
