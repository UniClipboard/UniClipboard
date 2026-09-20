# Windows 自建 GitHub Runner 调研来源

## GitHub 官方资料

- [Self-hosted runners reference](https://docs.github.com/en/actions/reference/runners/self-hosted-runners)
  - GitHub 只会把任务分给同时匹配全部标签的机器。
  - 没有在线且空闲的匹配机器时，任务会继续等待，最长可达 24 小时；`runs-on` 没有自动改派能力。
  - Runner 默认自动升级；GitHub 会在任务分配时或空闲期更新它。
- [Adding self-hosted runners](https://docs.github.com/en/actions/how-tos/manage-runners/self-hosted-runners/add-runners)
  - Windows 后台服务需要管理员权限，官方建议安装在 `C:\actions-runner` 一类系统账户可访问的位置。
- [REST API endpoints for self-hosted runners](https://docs.github.com/en/rest/actions/self-hosted-runners)
  - 仓库级接口可返回 Runner 的在线、忙碌、版本和标签状态。
  - 查询需要仓库管理只读权限；默认工作流凭据没有这项权限。

## 成熟项目模式

- [omniaura/mac-runner 的自动回退方案](https://github.com/omniaura/mac-runner#cicd-self-hosted-runner-with-automatic-cloud-fallback)
  - 先在 GitHub 提供的机器上读取自建 Runner 状态，再把唯一的后续任务指向自建或云端机器。
  - 状态凭据只用于前置选择，不传入实际构建任务。
- [GitHub Community #20019](https://github.com/orgs/community/discussions/20019)
  - 社区常用做法同样是先查询状态，再动态生成 `runs-on`；GitHub 本身没有优先级或自动回退语法。

## 本仓库与主机现状

- 仓库为公开仓库，现有正式构建由手动触发或受信任的发布流程调用，不把公开 PR 交给自建机器。
- 接入前仓库没有自建 Runner。
- Windows 主机为 x64 Windows，32 GiB 内存，具备 Git、Rust、Node.js 和 Bun。
- 官方 Runner `v2.337.0` 安装在 D 盘并作为 Windows 后台服务运行，专用标签为 `uniclipboard-desktop-windows-x64`。
- 状态选择使用仓库机密项 `WINDOWS_RUNNER_STATUS_TOKEN`。它只在 GitHub 提供的前置任务中读取 Runner 状态，不会传给构建任务；应使用仅限本仓库管理只读权限的专用凭据。
