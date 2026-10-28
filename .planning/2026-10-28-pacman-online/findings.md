# 调研

- 当前 HEAD 与 origin/main 均为 10c469c3e，工作区开始时干净；前序 pacman 安装类型与自更新限制已合入，无需 cherry-pick。
- AUR RPC 实测存在 uniclipboard-git，版本 1.5.0.r1.g20c5ddc-1。
- 单一构建配方：packaging/aur/uniclipboard-git/PKGBUILD；正式版应覆盖身份、版本、冲突与固定源提交，复用 build/package 函数。
- repo-add 官方手册说明相邻包签名会嵌入数据库，--sign 签数据库。https://man.archlinux.org/man/repo-add.8.en
- 本机提供 pacman、repo-add、gpg、docker。不得改系统包数据库或系统 GPG 信任库。
- 包当前只包含 GUI 与 daemon，不包含 uniclip CLI；本轮不扩大包内容。
- 采用不可变完整快照；发布端后续必须原子切换整个快照，不能逐文件覆盖线上数据库及签名。
