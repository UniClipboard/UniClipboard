# COPR 发布手册

本文档说明如何把 UniClipboard 发布到 [Fedora COPR](https://copr.fedorainfracloud.org/)，让 Fedora/RHEL/openSUSE 用户能用 `dnf install uniclipboard` 一键安装并自动跟随版本升级。

> 本管线与 `.github/workflows/snap.yml` 对位 — snap 走 Snapcraft，dnf 走 COPR，构成 Linux 双渠道分发。

## 整体方案

- **打包模式**：binary repackage。COPR 不重新编译 Tauri，而是把 GitHub Release 上 `release.yml` 已经产出的 binary RPM 当作 Source0，在 COPR mock chroot 里 `rpm2cpio` 解包后重新装配。
  - 优点：构建时间从 30 min（重新跑 Tauri）压到 1 min；二进制与 Releases 页严格一致；不需要把 webkit2gtk-devel 等重型依赖塞进 COPR chroot。
  - 缺点：违反 Fedora "build from source" 原则 — 因此不能进 Fedora 官方仓库，只能走 COPR 第三方渠道。这是有意取舍。
- **渠道映射**：
  - `mkdir700/uniclipboard` → 正式版（无 prerelease 后缀的 tag）
  - `mkdir700/uniclipboard-alpha` → alpha/beta/rc 预发版
- **触发**：`Release` workflow 成功后自动触发；也支持 `workflow_dispatch` 手动重发某个版本。

## 一次性准备

### 1. 在 COPR 上创建项目

1. 访问 https://copr.fedorainfracloud.org/，用 Fedora Account / GitHub OAuth 登录。
2. 顶部 **New Project**，分别创建：
   - `uniclipboard`（正式版）
   - `uniclipboard-alpha`（预发版）
3. 每个项目的 **Build options → Chroots** 至少勾上：
   - `fedora-40-x86_64`、`fedora-40-aarch64`
   - `fedora-41-x86_64`、`fedora-41-aarch64`
   - `fedora-rawhide-x86_64`（可选，跟踪 Fedora 滚动）
   - `epel-9-x86_64`、`epel-9-aarch64`（覆盖 RHEL 9 / Rocky / Alma）
4. 项目 **Settings → Description** 建议填上"Repackage of upstream UniClipboard binary RPMs from GitHub Releases. See https://github.com/UniClipboard/UniClipboard"，方便 COPR 主页用户识别。

### 2. 取 COPR API token

1. 访问 https://copr.fedorainfracloud.org/api/，登录后页面会展示一段 `~/.config/copr` 的内容，长这样：

   ```ini
   [copr-cli]
   login = xxxxxxxxxxxxxxxx
   username = mkdir700
   token = yyyyyyyyyyyyyyyy
   copr_url = https://copr.fedorainfracloud.org
   # expiration date: 2027-05-09
   ```

2. 把三个字段记到 GitHub 仓库 secrets：

   | secret 名 | 值 |
   |---|---|
   | `COPR_LOGIN` | `login` 字段（短的那个） |
   | `COPR_TOKEN` | `token` 字段（长的那个） |
   | `COPR_USERNAME` | COPR 用户名（`mkdir700`） |

   路径：`Settings → Secrets and variables → Actions → New repository secret`。

3. token 默认有效期约 6 个月。COPR 主页 token 过期前会提醒，到期后重新访问 API 页拿新 token 替换 secret 即可。

### 3. 检查 spec 中的 maintainer email

`packaging/uniclipboard.spec` 末尾 changelog 块里写的是 `mkdir700 <release@uniclipboard.app>`。如果你不持有这个邮箱，改成你 COPR 注册时用的邮箱，否则 COPR 不会拒绝构建，但用户在 `dnf info uniclipboard` 里看到的 packager 信息会不一致。

## 触发流程

### 自动触发（推荐）

1. 走常规 release：手动 `gh workflow run release.yml -f bump=patch -f channel=alpha`，或者打 git tag `vX.Y.Z`。
2. `Release` workflow 跑完后，`Publish to COPR`（`copr.yml`）通过 `workflow_run` 自动起来。
3. 它会按 tag 后缀决定推到哪个 COPR 项目：
   - tag 含 `-alpha` / `-beta` / `-rc` → `uniclipboard-alpha`
   - 其余 → `uniclipboard`
4. job 结束在 `GITHUB_STEP_SUMMARY` 里能看到 COPR build 链接。COPR 端跑完 mock chroot（每个 chroot 1-3 min）后包就会进入仓库。

### 手动重发

`Release` 已经发完但 COPR 那次失败了？直接 dispatch：

```bash
gh workflow run copr.yml \
  -f version=0.7.0-alpha.7 \
  -f project=uniclipboard-alpha
```

`version` 不填则读 `package.json`。`project` 必须显式选。

## 用户侧使用

```bash
# alpha 渠道
sudo dnf copr enable mkdir700/uniclipboard-alpha
sudo dnf install uniclipboard

# 正式渠道
sudo dnf copr enable mkdir700/uniclipboard
sudo dnf install uniclipboard

# 切换渠道：先关旧渠道再开新渠道
sudo dnf copr disable mkdir700/uniclipboard-alpha
sudo dnf copr enable mkdir700/uniclipboard
sudo dnf install uniclipboard --refresh
```

之后 `sudo dnf upgrade` 会自动跟随 COPR 仓库里的最新版。

## 故障排查

### COPR build 失败：`Source0: Bad URL`

原因：spec 里 `Source0` 用的 GitHub Release URL 在那个版本 tag 下不存在该架构的 RPM。

排查：手动 curl 一下：

```bash
curl -fsSLI \
  "https://github.com/UniClipboard/UniClipboard/releases/download/v0.7.0-alpha.7/UniClipboard-0.7.0-alpha.7-1.x86_64.rpm"
```

返回 200 才说明文件存在。如果 404，回头检查 `release.yml` 是否真的产出并上传了 RPM（参考 `assemble-update-manifest.js` 收集逻辑）。

### COPR build 失败：`error: cpio: lsetfilecon failed`

mock chroot 里 SELinux context 不能写，cpio 在还原 xattr 时报错但 payload 已经解出来了。一般忽略即可，rpmbuild 会继续走 `%install`。如果它真的中止，把 spec 的 `%prep` 改成 `rpm2cpio %{SOURCE0} | cpio -idm --no-preserve-owner` 跳过 metadata 还原。

### `dnf install uniclipboard` 报缺依赖

最常见是 `webkit2gtk4.1` 在 RHEL/Rocky 9 上叫 `webkit2gtk4.0` 或 `webkit2gtk5.0`，包名映射不一样。短期 workaround：在 spec 里加 conditional：

```spec
%if 0%{?rhel} >= 9
Requires: webkit2gtk4.0
%else
Requires: webkit2gtk4.1
%endif
```

中长期解决方案是再写一个 RHEL 专门的 SRPM，或在 spec 上层加一组 BuildArch/dist 条件。

## 维护清单

- [ ] COPR token 到期前更新 `COPR_TOKEN` secret
- [ ] 新增 Fedora 版本（如 fedora-42）时去 COPR 项目 Settings 勾上对应 chroot
- [ ] Tauri 升级导致依赖名变化（如 webkit2gtk-4.1 → 4.2）时同步改 spec 的 `Requires`
- [ ] 当 release.yml 改动 RPM 文件名规范时（例如 `UniClipboard-` → `uniclipboard-`），同步修改 spec 的 `Source0` 和 workflow 的 `download` 步骤
