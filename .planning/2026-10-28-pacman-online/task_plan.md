# pacman 在线软件源

## 目标
复用 AUR 配方构建正式版 uniclipboard，生成签名、可隔离验收的软件源快照。没有正式地址和签名身份前不公开上线。

## 阶段
1. 核对基线与现有所有者：complete
2. 正式标签构建入口与签名快照：complete
3. 隔离 pacman 安装、升级及拒绝损坏签名验证（小型测试包）：complete
4. 仅构建 CI 与中文运行文档：complete
5. 完整应用 CI 实跑、原生 ARM 验收、生产托管与发布接入：pending

## Next Step
推送分支运行完整应用 CI；生产签名身份和安全签名 runner 仍待用户提供。

## 待落实
托管已选择 GitHub Releases；待落实正式签名身份、安全签名 runner、包含更新保护的新稳定标签及原生 aarch64 构建环境。生产 release 原有门禁继续保留。

## 验收限制与错误记录
- 首次直接运行验收因 pacman-key 要求 root 失败；本机 sudo 与 Docker 均无权限。确认 unshare --user --map-root-user 可用后，全部测试改用用户命名空间。
- 文件归属查询首次使用宿主绝对路径失败；更正为隔离 root 下的实际文件路径后通过。
- 当前 HEAD 不是 v1.5.0 标签，正式渲染入口按预期拒绝；版本标签约束已由独立临时 Git 仓库验证。
- 完整应用未在本机编译或安装；不能把小型包通过称为产品发布通过。
