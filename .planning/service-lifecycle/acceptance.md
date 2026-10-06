# 生命周期验收记录

macOS arm64 上使用当前源码构建的 Go uniclip 和 Rust uniclipd；Engine 固定版本没有改变。

- 源码基线：`aed71aa487b53e2f8f90ab594c52dd2625e01059`，加本 PR 生命周期改动。
- uniclip SHA-256：`b831dc95e9e82523653f2b57b0ea4781419bd4c8628b77809adea7ee6adb1f5c`。
- uniclipd SHA-256：`a778878b3daed6205926469147909372d9646626540c54e6a3f58e0f4f940dc3`。
- 隔离 HOME 含空格，唯一 profile，文件密钥存储，禁用系统剪贴板。
- 真实 launchd 完整验收通过，退出码夹具返回 37 且标准错误可见。
- SIGTERM/SIGINT 正常退出；原子锁占用不会驱逐已有 daemon。
- 安装、重复启动、状态、重启、停止、重复停止、停止后重启、HTTP 超时、profile 隔离、
  选项变更拒绝、生产工作树拒绝、旧 start 警告及后台冷启动均通过。
- 清理：launchd job 未加载，服务定义已移除，前台进程全部退出；独立 ps 检查确认所有本任务服务和后台 PID 均退出。

复现：

```sh
cargo build -p uc-daemon --bin uniclipd
(cd apps/cli-go && go build -o ../../target/compat/go/uniclip ./cmd/uniclip)
cp target/debug/uniclipd target/compat/go/uniclipd
python3 apps/cli-go/e2e/service_lifecycle.py --bin target/compat/go --out target/compat/service
```

运行器会输出 result.json、服务定义、前台日志与二进制哈希，并在 finally 中卸载本次服务。
不得以真实用户 HOME/profile 代替隔离环境。

Go vet、buildinfo 无漂移、Cargo 格式检查，以及 Linux amd64/arm64、Windows amd64/arm64、
macOS amd64 交叉编译通过。Linux 用户 systemd、Windows 控制台信号、实际注销/登录和开机恢复未验证。

旧 Rust/Go 差分选取四组：stale_connection_records、member_without_space 一致；
malformed_health_bodies 有既有 JSON 解析文本差异和预期 start 弃用提示；search_local
有未修改客户端对 HTTP 429 错误正文的输出差异。未声称全套差分通过。
