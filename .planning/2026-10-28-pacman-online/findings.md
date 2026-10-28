# 调研

- 当前 HEAD 与 origin/main 均为 10c469c3e，工作区开始时干净；前序 pacman 安装类型与自更新限制已合入，无需 cherry-pick。
- AUR RPC 实测存在 uniclipboard-git，版本 1.5.0.r1.g20c5ddc-1。
- 单一构建配方：packaging/aur/uniclipboard-git/PKGBUILD；正式版应覆盖身份、版本、冲突与固定源提交，复用 build/package 函数。
- repo-add 官方手册说明相邻包签名会嵌入数据库，--sign 签数据库。https://man.archlinux.org/man/repo-add.8.en
- 本机提供 pacman、repo-add、gpg、docker。不得改系统包数据库或系统 GPG 信任库。
- 包当前只包含 GUI 与 daemon，不包含 uniclip CLI；本轮不扩大包内容。
- 采用不可变完整快照；发布端后续必须原子切换整个快照，不能逐文件覆盖线上数据库及签名。

## GitHub 托管接入
- 用户已授权继续完成后续工作，选定 GitHub Releases 托管，不需要新域名。
- gh 当前账号具有仓库管理与写权限；现有组织 runner 只有离线 Windows runner，没有安全 Linux 签名 runner。本机没有正式 GPG 私钥。
- 已选固定渠道 Release 标签 pacman-stable-x86_64；标记 prerelease 且 latest=false，避免影响应用正式 release。
- GitHub 附件无法原子替换数据库与签名；保持 SigLevel=Required，切换窗口失败关闭，重试而不放宽验签。首次用草稿完整上传后公开，后续先追加包并校验服务端 SHA-256，保留旧包后更新索引。
- 当前 v1.5.0 早于包管理器更新保护 b28e77779；只用于验收。生产签名作业要求源提交包含该保护，因此需要新稳定标签。
- 多个其他工作流也持续排队（包括比本次早一小时的作业），目前不能声称完整应用 CI 已通过。
