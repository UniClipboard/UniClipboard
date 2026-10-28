# pacman 二进制软件源

## 当前状态

本仓提供正式版 Arch 包构建、签名快照、验收和 GitHub Releases 发布入口。**生产签名环境尚未配置，软件源尚未上线**。计划使用现有 GitHub 仓库，不需要自建服务器或域名。既有应用生产发布门禁、R2 发布目录与 AUR 发布流程不变。

当前自动构建只覆盖 x86_64。快照格式支持 aarch64，但上线该架构前必须提供原生 Arch Linux ARM 构建环境，确认 GTK4/WebKitGTK 依赖并完成原生安装验收；不使用 x86_64 Arch 容器模拟已支持 ARM。

## 构建所有权

`packaging/aur/uniclipboard-git/PKGBUILD` 继续唯一负责依赖、编译与文件布局。`scripts/ci/render-arch-release.sh` 读取目标提交的 `apps/gui-go/app.json`，要求 HEAD 正好等于对应的 `vX.Y.Z` 标签，仅覆盖包身份、固定版本、冲突与源提交，并取消动态 `pkgver()`。

正式包名为 `uniclipboard`，提供 GUI 和 `uniclipd`，当前不含 `uniclip` CLI。它与 `uniclipboard-git`、`uniclipboard-bin` 冲突。AUR 继续追踪源码，两者不是自动替换关系。正式源仅接受稳定版本，版本为 `X.Y.Z-1`；预发布不进入稳定源。修改同一版本配方时应发布新产品版本，当前入口不支持独立递增 pkgrel。

在一次性原生 Arch 构建容器中运行（脚本创建用户、升级系统，只能在可丢弃环境中执行）：

```bash
bash scripts/ci/build-aur-package.sh /work/source <release-commit> /work/output stable
```

默认不传 `stable` 的行为仍为 AUR 构建。目标提交必须有正式标签并与应用版本一致；工具使用当前检出的脚本，构建配方和应用源来自指定提交。

`.github/workflows/pacman-repo.yml` 手动输入已有稳定标签，生成未签名正式包和构建证据。验收作业使用临时密钥检验完整包的签名下载，以及小型测试包的安装、升级、卸载、文件归属、数据库篡改、包篡改和缺包拒绝。默认只构建和验收；只有在 main 上手动选择 publish，且安全签名环境配置完成时，才会发布。工作流的 native-install 作业复用既有 `scripts/ci/check-arch-package.sh` 执行完整 GUI 的原生启动验收；本地下载验收不能称为 GUI 安装成功。

## 签名与不可变快照

生产签名必须使用既有安全签名设备或安全存储，由 GPG agent 提供密钥操作；脚本只接受完整指纹，不接收、导入或导出私钥。不要为方便 CI 将私钥写入仓库或普通配置文件。pacman 的 OpenPGP 信任链独立于应用自更新的 minisign 信任链。

签名机需要 Python、GnuPG、libarchive 的 bsdtar 和 pacman 的 repo-add：

```bash
python3 scripts/ci/assemble-pacman-repo.py \
  output/uniclipboard-X.Y.Z-1-x86_64.pkg.tar.zst \
  output/snapshot-X.Y.Z \
  --arch x86_64 --fingerprint <FULL_SIGNING_FINGERPRINT>
```

目标目录必须不存在。工具检查 `.PKGINFO` 的包名、架构与稳定版本，先签包，再通过 repo-add 签数据库，逐一验证指纹；成功后才把暂存目录改名为目标目录。失败清理暂存目录。每个快照包含：

```text
uniclipboard.asc
signing-fingerprint.txt
x86_64/
  uniclipboard-X.Y.Z-1-x86_64.pkg.tar.zst
  uniclipboard-X.Y.Z-1-x86_64.pkg.tar.zst.sig
  uniclipboard.db[.tar.gz][.sig]
  uniclipboard.files[.tar.gz][.sig]
```

repo-add 生成的符号链接会转换为普通文件，以便对象存储上传。快照只索引本次输入的版本；旧快照须保留，允许已同步旧数据库的客户端继续下载旧包。

## GitHub Releases 托管与发布

固定渠道标签为 `pacman-stable-x86_64`，资产直接放在该 Release 下：

```text
https://github.com/UniClipboard/UniClipboard/releases/download/pacman-stable-x86_64
```

此地址是已选定的发布入口，但尚未上线。Release 标记为预发布且不设为 latest，以免影响正常应用版本发现；这里的预发布标记只用于 GitHub 的渠道隔离，包仍只接受正式稳定版本。

`scripts/ci/publish-pacman-github.py` 默认只验证与展示计划；传入 `--publish` 才写入 GitHub。发布前校验数据库和包的签名及完整指纹、旧数据库签名和版本排序，拒绝同版本、降级、不可变 Release 与不完整旧数据库。首次发布先创建草稿，上传完整内容后再公开。后续发布先追加包与签名、检查 GitHub 报告的 SHA-256，再更新数据库；旧包不删除，不重写已有包名。CI 串行化同一分支发布，只允许 main 写生产渠道，不导入私钥。

GitHub 不提供多个 Release 附件的原子替换，因此数据库与独立签名切换期间可能短暂验签失败；用户稍后重新执行完整更新即可，禁止关闭签名校验。写入失败可能留下待修复的渠道，维护者必须核对数据库与签名后恢复；脚本不会自动绕过已有渠道校验。此方案保证包校验与旧包保留，不承诺无中断切换。若未来需要原子快照切换，可再迁移到具备该能力的托管入口。

仓库配置：

- 准备带 `self-hosted`、`Linux`、`pacman-signing` 标签的专用 runner，安装 Python、GnuPG、bsdtar、repo-add，并接入安全 GPG agent。不要在普通文件中存储生产私钥，也不要在不可信 PR 上运行此 runner。
- 配置 `pacman-production` Environment，仅允许 main 部署；按团队管理要求配置审核人。
- 配置仓库变量 `PACMAN_SIGNING_FINGERPRINT` 为完整大写指纹，确认签名环境就绪后才将 `PACMAN_SIGNING_ENABLED` 设为 `true`。
- main 上手动运行 Pacman repository 工作流，输入正式标签并选 publish。签名和发布均依赖构建、签名事务验收及原生 GUI 安装验收成功。
- 生产包源提交必须包含 b28e77779 引入的包管理器更新保护；现有 v1.5.0 只能用于构建验收，不能发布到生产软件源。需要包含该修复的新稳定标签。

首次上线必须通过真实公开 HTTPS 地址执行下载与完整安装升级验收，并在独立可信渠道公布正式指纹。正式密钥尚未确定时不得用临时测试密钥公开发布。

## 安装与迁移（上线后）

以下 GitHub 地址已确定但尚未上线，指纹仍为占位符；必须先由维护者公布完整指纹及独立可信渠道的指纹说明。用户需验证下载公钥的完整指纹，再导入并本地签名：

```bash
curl --fail --location --output uniclipboard.asc \
  'https://github.com/UniClipboard/UniClipboard/releases/download/pacman-stable-x86_64/uniclipboard.asc'
gpg --show-keys --with-fingerprint uniclipboard.asc
sudo pacman-key --add uniclipboard.asc
sudo pacman-key --lsign-key '<VERIFIED_FULL_FINGERPRINT>'
```

上线后在 `/etc/pacman.conf` 追加以下内容（目前仅规划 x86_64）：

```ini
[uniclipboard]
SigLevel = Required
Server = https://github.com/UniClipboard/UniClipboard/releases/download/pacman-stable-$arch
```

```bash
sudo pacman -Syu uniclipboard
# 后续随系统更新
sudo pacman -Syu
```

使用 AUR 或本地 `uniclipboard-git` / `uniclipboard-bin` 包时，pacman 会请求移除冲突包；先核对事务只替换预期软件包。不要使用 `--overwrite` 强行覆盖文件，也不要使用 `pacman -Sy` 做部分升级。同名本地正式包可由软件源提供的更高版本升级，同版本不会强制重装。应用内更新仍交由包管理器处理。

卸载应用使用 `sudo pacman -Rns uniclipboard`；移除软件源时删除上述配置段，再运行完整系统更新。只有确认没有其他包使用该签名密钥后才删除对应信任项。卸载包不代表删除个人剪贴板资料。

密钥轮换需要提前通过独立可信渠道公布新指纹，在切换前指导用户导入并信任新公钥；不得关闭 `SigLevel` 绕过验签。自动 keyring 包分发、撤销与生产签名设备接入仍需另行落实。

## 本地隔离验收

无需系统 root 权限，可用 Linux 用户命名空间运行小型包验收；所有 pacman 数据库、缓存、日志、安装根和 GPG 信任库均在临时目录中：

```bash
unshare --user --map-root-user python3 scripts/tests/pacman_repo_acceptance.py
python3 scripts/tests/test_aur_package.py
```

CI 的完整包验签下载使用 `--package-dir built`。这不会宣称完整 GUI 或两种架构均已原生安装成功。上线前仍需实际执行完整包的原生安装与 GUI 检查、真实版本间升级、AUR 冲突迁移，并确认生产 HTTPS 托管的完整性和渠道切换行为。
