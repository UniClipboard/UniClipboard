# alpha.5 剪贴板升级基线

本样本由官方 `v1.0.0-alpha.5` macOS ARM 命令行发布包在开发环境实际运行生成。
发布包 SHA-256 已与 GitHub 发布资产及 `SHA256SUMS.txt` 核对。

样本只包含独立测试档案、开发文件密钥及三条合成剪贴板记录：普通文字、多行文字、链接。
三条内容均通过系统剪贴板复制，由旧版后台捕获；正常停止后台后按文件白名单打包。
未包含进程锁、连接凭据、日志、数据库共享内存或真实用户内容。

`expected.json` 保存合成口令、记录标识和预期正文。`manifest.json` 保存压缩包及逐文件摘要。
每轮复现会校验所有摘要，并恢复到新的随机开发档案，不修改原始样本。

## 实际结果

2026-09-09，在 Desktop `6dc638139d72265f35b1cf6b2e77ea6b421809c3`、Engine
`68ff88a392f495b3906985d1c31d1dde8071dab3` 下实测：

1. alpha.5 启动基线，三条记录逐条读取，正文全部匹配。
2. 正常停止旧版，启动当前版本，升级失败：`corrupt`。
3. 直接重启当前版本，仍为 `corrupt`。
4. 重新启动 alpha.5，原三条记录仍可完整读取；通过系统剪贴板新增一条记录并确认读回。
5. 正常停止旧版，再启动当前版本，升级失败：`source_changed`。
6. 再次启动当前版本，仍为 `source_changed`。

对照实验中，只重新启动旧版而不显式新增剪贴板记录，并不总是出现 `source_changed`。
已证明“失败后的旧数据再次变化”可以造成用户提供的错误序列；尚未证明用户的 Windows
现场经过完全相同。首次 `corrupt` 后续已定位：缺少 generation manifest 的旧版仍有 V2
群组密钥，升级错误地安装替代材料，丢失当前会话中的旧内容密钥，导致 inline 解密失败。

## 重复运行

先准备已核验的 alpha.5 发布目录和上述提交编译的当前 `uniclip`、`uniclipd`：

```bash
node .planning/alpha5-upgrade/replay.mjs <alpha.5发布目录> <当前程序目录>
```

此实验会写系统剪贴板。输出保存在 `.planning/alpha5-upgrade/dev-upgrade-alpha5-replay-*/`。
结果包含每步退出状态与错误分类，原始日志保留在本机，供继续定位。
这是一份失败复现样本，不代表升级通过；Windows 与系统钥匙串路径未执行。

## 修复验证

Engine `fix/alpha5-storage-upgrade` 分支修复后，使用同一份样本完成以下实际运行检查：

| 场景 | 升级后逐条读取 | 重启后逐条读取 |
| --- | --- | --- |
| alpha.5 直接升级 | 三条正文全部一致 | 三条正文全部一致 |
| 未修复版本先产生 `corrupt`，再启动修复版 | 三条正文全部一致 | 三条正文全部一致 |
| 升级失败后由 alpha.5 新增记录，并确认未修复版报 `source_changed`，再启动修复版 | 四条正文全部一致 | 四条正文全部一致 |

```bash
node .planning/alpha5-upgrade/verify-repair.mjs <alpha.5发布目录> <未修复程序目录> <修复程序目录>
```

验证器为每个场景恢复独立档案，先校验样本摘要，再检查预期故障、升级后的记录数量、逐条正文和重启结果。
结果与进程日志保存在 `.planning/alpha5-upgrade/dev-upgrade-alpha5-repair-*/`，不纳入版本管理。
