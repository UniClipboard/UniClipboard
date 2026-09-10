# alpha.5 升级修复

## 根因

1. `RuntimeUpgradeBootstrap` 以缺少 V2/V3 存储 manifest 判断旧资料缺少群组材料。
   alpha.5 正常存在 V2 inline 密文与已建立的群组密钥；静默恢复成功后，旧逻辑又安装了只含
   legacy-v1 与确定性迁移密钥的材料。随后 `convert_inline` 读取原 V2 key id 时得到
   `key material not found`，包装为 `corrupt`，阻断启动。
2. 失败后的升级 journal 保存了旧数据库修订。旧版本继续写入后，既有检查拒绝过期快照，
   但不会为仍未激活的候选重新生成计划，之后每次启动都重复 `source_changed`。

## 修复所有权

- 正式修复位于 Engine 仓库的 `fix/alpha5-storage-upgrade` 分支，已推送提交 `32d0cbb0d0dfc3c482a64b6ffdc5a1db56e3b766`。
- Desktop 调查分支基于 `origin/main` 的 `6dc638139d72265f35b1cf6b2e77ea6b421809c3`。
- Engine 修复基于 Desktop 固定的 `68ff88a392f495b3906985d1c31d1dde8071dab3`，没有混入其他开发变更。
- 调用方继续只执行 `ensure_v3`，不增加桌面补丁或恢复接口。
- 新的材料判定读取安全仓储，布局标记不再替代安全状态。
- 仅本次恢复入口、原 source 身份不变、候选尚未激活时，允许来源修订变化后重建。
  先保存 Detected journal，再由 staging 清理同一组未激活候选目录；重启后仍可继续。
  已激活 V3、source manifest 改变、同次升级中的并发写入仍保持原保护边界。

## 验证

- 修改前：实际 alpha.5 基线与合成 V2 历史测试都报相同缺钥错误；来源更新恢复测试报 SourceChanged。
- 修改后：存储升级集成检查与外部 alpha.5 基线合计 18 项全部通过。
- 转换、加密字段、候选验证相关检查 13 项通过。
- Engine 全 workspace、全部目标编译检查、格式检查和架构检查均通过。
- 三个真实进程场景均通过升级和重启后的逐条正文检查，结果见基线 README。
- 没有修改真实用户档案，没有删除旧正文来绕过错误；合成基线一直保持原始摘要。
- Windows x64 测试包已由 Desktop 提交 `4358f14e6d097dbf239e85faa6f62193cf50ac1c` 构建，运行 [34318344515](https://github.com/UniClipboard/UniClipboard/actions/runs/34318344515) 成功；日志确认使用上述 Engine 修复提交，安装包与便携包已下载核对。
- Windows 安装后的实际升级与系统钥匙串路径仍待用户验证。

## 构建注意

本机全局 Cargo 配置会替换 Engine 源码。验证时显式将两个 Engine patch 指向本修复分支，
并核对构建输出的实际路径。由本机 patch 导致的锁文件变化不属于修复，应保持原锁文件。
Desktop `fix/engine-alpha5-upgrade-windows` 分支已固定上述 Engine 提交。普通远端 main 在合入这次更新之前，不包含本次修复。
