# Windows 自建 GitHub Runner 运维说明

UniClipboard Desktop 的 Windows 应用构建和 CLI 构建固定使用 GitHub 托管的 `windows-latest`。x64 使用 `x86_64-pc-windows-msvc`；ARM64 在 x64 托管运行器上交叉构建，使用 `aarch64-pc-windows-msvc`。原生 ARM64 验收仍使用 `windows-11-arm`。签名策略与工件格式不变。

工作流不再查询自建运行器状态，也不再使用 `WINDOWS_RUNNER_STATUS_TOKEN`。下文保留旧运行器的运维与移除步骤；本次工作流变更不停止服务，也不移除注册。

已触发运行和已有标签中的工作流不会自动采用新配置。重新运行旧发布仍使用原工作流。应从包含修复的新版本标签发布，不要移动已有标签。`release.yml` 的手动入口会升级版本并推送提交，不是重发旧版本的入口。
## 历史配置

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

包含新配置的 Windows 构建始终使用 GitHub 托管运行器。旧工作流中已经选中自建运行器的任务不能自动改派。停用前，先确认没有等待或运行中的旧任务。

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
- 当前工作流不使用运行器状态凭据。删除旧凭据前，先确认没有其他使用方。
- 代码检出关闭凭据保留，避免仓库写入凭据留在自建机器的工作目录中。
- 保留旧运行器注册不影响当前发布路由。
