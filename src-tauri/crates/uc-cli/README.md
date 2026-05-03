# uc-cli

`uc-cli` 是 UniClipboard 的终端入口 crate，构建出的二进制名是 `uniclip`。

它用于在终端里完成空间初始化、设备加入、配对查看、文本发送、入站监听、搜索诊断、blob 诊断，以及本机 daemon 的启动和停止。

## 运行方式

所有 Cargo 命令都从 `src-tauri/` 目录执行：

```bash
cargo run -p uc-cli -- --help
cargo run -p uc-cli -- status
cargo run -p uc-cli -- --json status
```

构建后可直接运行：

```bash
cargo build -p uc-cli
./target/debug/uniclip --help
```

## 全局参数

| 参数 | 说明 |
| --- | --- |
| `--json` | 用 JSON 输出结果，适合脚本调用。 |
| `-v`, `--verbose` | 打开更详细的诊断日志。 |
| `--profile <NAME>` | 使用独立 profile，隔离本地数据、密钥和网络身份；常用于单机模拟多设备。 |
| `--dev` | 开发模式下使用，避免依赖系统 keychain。 |

## 常用命令

| 命令 | 用途 |
| --- | --- |
| `uniclip start` | 启动本机 daemon。默认后台运行。 |
| `uniclip start --foreground` | 前台启动 daemon，并把日志输出到终端。 |
| `uniclip stop` | 停止本机 daemon。 |
| `uniclip status` | 查看当前应用状态。 |
| `uniclip init` | 在当前 profile 创建新的加密空间。 |
| `uniclip invite` | 作为 sponsor 发起配对邀请。 |
| `uniclip join` | 使用邀请加入已有空间。 |
| `uniclip devices` | 列出已配对设备。 |
| `uniclip members` | 列出空间成员和在线状态。 |
| `uniclip send [TEXT]` | 向在线配对设备发送一段文本；省略 `TEXT` 时从 stdin 读取。 |
| `uniclip watch` | 监听并打印收到的剪贴板 payload；不会写入系统剪贴板。 |

## 搜索命令

```bash
uniclip search status
uniclip search rebuild
uniclip search query "keyword"
```

`search query` 支持内容类型、文件扩展名、时间范围、分页和详细输出：

```bash
uniclip search query "report" --type text --ext md --limit 20 --detailed
uniclip search query "report" --from-ms 1710000000000 --to-ms 1710100000000
```

`search rebuild` 是同步命令，完成后才返回。

## Blob 诊断命令

`blob` 命令用于发布或拉取加密的大 payload，主要服务于文件同步和传输诊断。

```bash
uniclip blob publish ./sample.bin
uniclip blob fetch <TICKET> --entry-id <ENTRY_ID> --out ./restored.bin
```

发布会输出后续拉取需要的 ticket 和 entry id。拉取时必须同时提供这两个值。

## 空间切换和测试辅助命令

| 命令 | 用途 |
| --- | --- |
| `uniclip switch-space` | 切换到另一个 sponsor 的空间，并迁移本地历史数据。 |
| `uniclip seed-clipboard --text <TEXT>` | 调试 / 端到端测试用，直接写入一条加密文本记录。 |
| `uniclip dump-clipboard --limit <N>` | 调试 / 端到端测试用，打印最近的解密记录预览。 |

## 隐藏的剪贴板诊断命令组（`probe`）

`uniclip probe` 是隐藏子命令（不会出现在 `--help` 中），收编自原先的
`clipboard-probe` 二进制，仅供开发与 E2E 调试。`probe restore` 是 CLI
中唯一允许直接写系统剪贴板的入口，详见 `AGENTS.md` 的诊断例外条款。

```bash
uniclip probe watch                    # 监听剪贴板变化
uniclip probe watch --max-events 10    # 最多观察 10 个事件
uniclip probe capture --out snap.json  # 抓取当前剪贴板到文件
uniclip probe inspect --in snap.json   # 解析快照文件
uniclip probe restore --in snap.json   # 把快照写回系统剪贴板（诊断用）
uniclip probe restore --in snap.json --select 0  # 多 representation 时选其一
```

## 行为边界

- CLI 是终端交互层，不拥有业务规则。
- 业务命令必须通过应用层 facade 执行动作，不能直接绕过应用层访问底层实现。
- 独立业务命令会构造自己的 CLI application session；同一个 profile 已有 daemon 运行时，应先停止 daemon 或换用 `--profile`。
- `start` / `stop` 只负责本机 daemon 生命周期。
- 隐藏的 `daemon` 子命令只供 `start` 内部启动后台进程，不作为用户命令记录或宣传。
- CLI 的终端可见输出必须保持英文；项目文档和代码注释按仓库约定使用中文。

## 验证命令

修改本 crate 后，优先运行：

```bash
cargo test -p uc-cli
cargo run -p uc-cli -- --help
```

如果改动涉及搜索或 blob 命令，也要查看对应帮助：

```bash
cargo run -p uc-cli -- search --help
cargo run -p uc-cli -- blob --help
```
