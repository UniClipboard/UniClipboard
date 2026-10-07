# 生命周期验收记录

2026-10-07 在 main 的 Go/Wails 宿主退役改动上重新构建并验收。

- main 基线：`8306fde45f150842b75706125b31b029a0a9ae25`（PR #1912）。
- 受验代码提交：`b369f5328ae7f6ca13a52f310636b453685dc455`。
- Engine 依赖沿用该 main 的固定 revision `e86f94cebcec46c1b3a6f49f88cce7a833777640`，本 PR 不改引擎版本。
- uniclip SHA-256：`9603dd01d0b273bb1e2405c96f82c57ea3821cb46aab61eb5ca697e1cf350ccb`。
- uniclipd SHA-256：`c7c448d8953cc8934826e1dea6eab498eafbcb83b9e660943c4a3539feca6624`。
- 使用原工作树和原分支；rebase 保留 CLI 业务提交，删除只修改已退休 tao 的旧修复提交。
- 未恢复 Tauri、tao、旧 CI 或宿主构建脚本；退休门禁通过。

## 真实 macOS 验收

macOS arm64，真实 daemon 和 GUI 用户域 launchd；隔离 HOME 含空格，唯一 profile，
文件密钥存储，禁用系统剪贴板。完整生命周期验收通过：

- 前台 SIGTERM/SIGINT 正常退出；原子锁占用不会驱逐已有 daemon。
- 安装、重复启动、状态、重启、停止、重复停止、停止后重启、HTTP 超时、profile 隔离。
- 拒绝运行中选项变更、生产工作树服务、便携服务和无效 AppImage 便携请求。
- 旧 start 仍为兼容后台启动并给弃用警告，后台冷启动/停止通过。
- 退出码夹具返回 37 且标准错误可见；这是进程边界验收，不冒充真实 daemon 崩溃。
- 清理后 launchd job 未加载、服务定义移除、前台进程全部退出；独立 ps 检查确认四个本次服务/后台 PID 均退出。

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
