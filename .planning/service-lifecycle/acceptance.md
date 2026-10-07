# 生命周期验收记录

2026-10-07 在 main 的 Go/Wails 宿主退役改动上重新构建并验收。

- main 基线：`8306fde45f150842b75706125b31b029a0a9ae25`（PR #1912）。
- 受验代码提交：`8adab21d6d6124ec654df67bb9ee8a03d86dbcb5`。
- Engine 依赖沿用该 main 的固定 revision `e86f94cebcec46c1b3a6f49f88cce7a833777640`，本 PR 不改引擎版本。
- uniclip SHA-256：`e046acbf292a121673134d5526284ae632b110192a9897fb43da7e043aaf4d25`。
- uniclipd SHA-256：`c7c448d8953cc8934826e1dea6eab498eafbcb83b9e660943c4a3539feca6624`。
- 使用原工作树和原分支；rebase 保留 CLI 业务提交，删除只修改已退休 tao 的旧修复提交。
- 未恢复 Tauri、tao、旧 CI 或宿主构建脚本；退休门禁通过。

## 真实 macOS 验收

macOS arm64，真实 daemon 和 GUI 用户域 launchd；隔离 HOME 含空格，唯一 profile，
文件密钥存储，禁用系统剪贴板。完整生命周期验收通过：

- 前台 SIGTERM/SIGINT 正常退出；原子锁占用不会驱逐已有 daemon。
- 安装、重复启动、状态、重启、停止、重复停止、停止后重启、HTTP 超时、profile 隔离。
- 拒绝运行中选项变更、生产工作树服务、便携服务、无效 AppImage 便携请求及符号链接临时目录内的生产可执行文件。
- 旧 start 仍为兼容后台启动并给弃用警告，后台冷启动/停止通过。
- 退出码夹具返回 37 且标准错误可见；这是进程边界验收，不冒充真实 daemon 崩溃。
- 清理后 launchd job 未加载、服务定义移除、前台进程全部退出；独立 ps 检查确认四个本次服务/后台 PID 均退出。
- 原生管理器清理调用均限时 30 秒。另以进程夹具注入管理器调用超时：报告错误并移除定义、退出所有本次前台进程；独立 launchctl 查询确认唯一测试 job 未加载。该异常验收不冒充真实管理器故障。

复现：

```sh
cargo build -p uc-daemon --bin uniclipd --locked
(cd apps/cli-go && go build -o ../../target/compat/go/uniclip ./cmd/uniclip)
cp target/debug/uniclipd target/compat/go/uniclipd
python3 apps/cli-go/e2e/service_lifecycle.py --bin target/compat/go --out target/compat/service
```

运行器输出 result.json、服务定义、前台日志与二进制哈希，并在 finally 中卸载本次服务。
不得以真实用户 HOME/profile 代替隔离环境。

## 证据边界

Go vet、buildinfo 无漂移、Cargo 格式与退休/Engine 仓库门禁、33 对中英文文档检查通过。
Linux amd64/arm64、Windows amd64/arm64、macOS amd64 交叉编译通过。
Linux 用户 systemd、Windows 控制台信号、实际注销/登录和开机恢复未验证；交叉编译不代替原生验收。

完整 CI 状态与本地工件保存在任务报告中，不能以旧 HEAD 的通过结果代替 rebase 后结果。

## 最新 CI 文件发送修正

最终 `4e6eeaff7` 的 CLI E2E 在已有交互文件接收场景失败。修正源码提交：`5c36493c108356d20eff74041f0835e3a2ff42d3`。
真实隔离复现显示 Engine 负责的调度返回 `accepted=0, pending=1`，旧文件发送命令忽略 pending 并误报失败。
沿用 resend 的现有语义：pending 表示后台继续处理已被接受，返回成功不表示传输完成；
人类输出显示 pending 数量并说明后台继续处理，JSON 在非零时包含 `totalPending`。
已接受目标的原有 delivery 等待、真实失败和取消退出码不变，不修改 Engine 或 daemon API。

macOS arm64 本地验收：

- 同一已有真实交互文件接收 E2E 修正前失败、修正后通过，64 MiB 文件及终端进度均校验成功。
- `get_entry` 全部 14 项真实 E2E 通过，包含 JSON 接收、文本/文件接收、交互进度和取消场景。
- JSON 发送变体返回 `totalPending: 1`，接收端完成同一文件与进度校验；没有改动接收测试断言。
- 完整 daemon/launchd 服务生命周期回归通过；清理确认 job 未加载、定义移除、任务进程退出。
- Go vet、buildinfo 无漂移、五个跨平台构建、帮助导出、退休门禁及空白检查通过。

双节点验收通过共享 `isolated_env()` 启动已有 `get_entry` 测试可执行文件，唯一 HOME、
开发环境文件密钥、禁用系统剪贴板。保留日志与二进制哈希，按创建时间和可执行文件核对已观察
子进程身份后清理；复现前后及全套运行均无任务子进程残留。
具体脚本、结果、Engine pending 日志与终端记录在任务工件 `ci-followup/`。
受验 Go 二进制 SHA-256：`555d98c487d2661b7eaf3c39f7f08ca5659c96348533460f836d99fdce8907a0`；
daemon（当前 main + e2e-rendezvous 特性）：`02e1a874dacc9d9b8a92a1642d58bc7805e2b1f4e17e5e12a19d9a48e7e40e97`。

重新推送后的 CI 尚须按新 HEAD 核对。Vercel 每日配额是独立外部阻塞，不升级计划、不刷空提交或连续重试。
