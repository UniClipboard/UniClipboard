# Windows 自建 GitHub Runner 运维说明

UniClipboard Desktop 的 Windows 应用构建和 CLI 构建会先检查专用 Windows 真机是否在线且空闲。可用时使用真机；离线、忙碌、查询失败或缺少状态凭据时，改用 GitHub 提供的 `windows-latest`，同一次构建只会选择一处执行。

## 当前配置

- Runner 名称：`uniclipboard-windows-x64-01`
- 专用标签：`uniclipboard-desktop-windows-x64`
- 安装目录：`D:\actions-runner-uniclipboard`
- Windows 服务：`actions.runner.UniClipboard-UniClipboard.uniclipboard-windows-x64-01`
- 工作目录：`D:\actions-runner-uniclipboard\_work`

Runner 只注册到 `UniClipboard/UniClipboard` 仓库，并关闭了默认标签。不要把这个标签分配给其他仓库或通用机器。

## 日常检查与重启

在管理员 PowerShell 中运行：

```powershell
Get-Service 'actions.runner.UniClipboard-UniClipboard.uniclipboard-windows-x64-01'
Restart-Service 'actions.runner.UniClipboard-UniClipboard.uniclipboard-windows-x64-01'
```

服务设置为延迟自动启动，并配置了失败恢复。Windows 重启后无需手动启动。

## 升级

Runner 默认自动升级。GitHub 发布新版本后，会在分配任务时或一周内的空闲期完成更新。可在仓库“设置 → Actions → Runners”中查看当前版本。

若自动升级失败，先停用服务并备份 `_diag` 日志，再按 GitHub 仓库设置页面提供的当前 Windows x64 安装命令重新安装。下载包必须核对 GitHub 发布页给出的摘要。

## 临时停用与恢复

```powershell
Stop-Service 'actions.runner.UniClipboard-UniClipboard.uniclipboard-windows-x64-01'
Set-Service 'actions.runner.UniClipboard-UniClipboard.uniclipboard-windows-x64-01' -StartupType Disabled
```

恢复：

```powershell
sc.exe config actions.runner.UniClipboard-UniClipboard.uniclipboard-windows-x64-01 start= delayed-auto
Start-Service 'actions.runner.UniClipboard-UniClipboard.uniclipboard-windows-x64-01'
```

停用后，新的 Windows 构建会在前置检查中选择 GitHub 提供的机器。已经选中自建机器但尚未开始的任务不具备原生二次改派能力，因此应先确认没有正在等待或运行的任务再停用。

## 永久移除

1. 在仓库“设置 → Actions → Runners”中为该 Runner 生成移除凭据。
2. 在管理员 PowerShell 中进入安装目录并运行：

```powershell
cd D:\actions-runner-uniclipboard
.\config.cmd remove --token <一次性移除凭据>
```

3. 确认仓库页面不再显示该 Runner 后，再删除安装目录。

不要只删除本地目录；那会在 GitHub 上留下永久离线的记录。

## 安全边界

- 不把公开 PR 或其他不受信任来源的代码交给这台长期存在的机器。
- `WINDOWS_RUNNER_STATUS_TOKEN` 只用于 GitHub 提供的前置选择任务，不会传入 Windows 构建。它至少需要目标仓库的管理只读权限；应优先使用只授予该仓库和该权限的专用凭据。
- 代码检出关闭凭据保留，避免仓库写入凭据留在自建机器的工作目录中。
- 状态查询失败时默认使用 GitHub 提供的机器，不因凭据或接口故障阻塞构建。
