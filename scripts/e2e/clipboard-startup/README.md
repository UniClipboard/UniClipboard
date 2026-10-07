# macOS 剪贴板启动恢复进程验收

此验收运行真实 `uniclipd`、HTTP 接口和 SQLite/profile。每次生成独立便携 profile、文件和命名 NSPasteboard；注入库只将当前测试进程的 `generalPasteboard` 重定向到该命名板，不操作用户通用剪贴板、钥匙串或历史。

权限拒绝使用真实文件权限 `000`；源打开失败与中途读取失败仅对测试指定的源文件注入 `ENOENT`/`EIO`。这是可重复的平台 I/O 验收，不是 Finder 操作或 TCC 根因证明。注入工具调用 macOS 的底层 syscall 绕过自身 interpose，编译器的弃用警告属于测试工具边界，不进入产品构建。

先用固定 Engine 来源构建 `uc-daemon`，然后执行（工具与工件目录应为独立可再生目录）：

```bash
cargo build -p uc-daemon --locked
mkdir -p "$CLIPBOARD_E2E_TOOLS"
clang -fobjc-arc -framework AppKit -DPRIVATE_PASTEBOARD_TOOL scripts/e2e/clipboard-startup/private-pasteboard.m -o "$CLIPBOARD_E2E_TOOLS/private-pasteboard"
clang -fobjc-arc -dynamiclib -framework AppKit scripts/e2e/clipboard-startup/private-pasteboard.m -o "$CLIPBOARD_E2E_TOOLS/private-pasteboard.dylib"
python3 scripts/e2e/clipboard-startup/real-daemon-clipboard-e2e.py --daemon target/debug/uniclipd --tools "$CLIPBOARD_E2E_TOOLS" --artifacts "$CLIPBOARD_E2E_ARTIFACTS"
```

成功工件包含固定断言 `result.json`、真实 daemon PID/退出码及通过 HTTP 标准导出的 `diagnostics.zip`。脚本清理本次生成的 profile/key/history，保留二进制、诊断与进程日志。标准 ZIP 同时检查 Engine 错误链和 Desktop 日志，不得包含源文件名哨兵。失败时保留已完成场景与失败类型，终止本次子进程并释放命名板。

失败模型覆盖：非空 register 读取失败阻断启动、错误分类或 OS 码在适配过程中丢失、失败导入留下空文件或部分文件、清理未持久导致同故障重启再次失败、恢复后的文本/文件捕获失败，以及标准诊断遗漏宿主/Engine 日志或泄漏源路径。验收逐项断言，三类故障分别保留 `PermissionDenied/13`、`NotFound/2` 和 `Uncategorized/5`。每个启动进程均记录 PID 与退出状态，`source-provenance.json` 保存构建输入、测试工具和 daemon 的 SHA-256。

真实适配器隔离进程验收另有以下入口（平台快照和安全存储使用测试替身，文件 I/O、Engine 与 SQLite 为真实实现；不等同于原生 NSPasteboard 验收）：

```bash
UC_TEST_ARTIFACTS_DIR="$CLIPBOARD_ADAPTER_ARTIFACTS" cargo test -p uc-bootstrap --locked desktop_clipboard_recovery_process -- --nocapture
```

PR 的 macOS E2E job 执行原生验收，并通过已有 `e2e-evidence` 上传结果、诊断 ZIP 和来源记录。
