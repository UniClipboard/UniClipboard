# 任务计划：开发者手动选择网卡配对工具

## 完成标准

- `uniclip` 提供隐藏的开发者配对入口，不影响正常 `invite` / `join`。
- 开发者可以列出当前 iroh endpoint 可发布的候选地址。
- 开发者可以指定一个 IP 地址生成配对码，生成的 ticket 只包含该 IP 对应的地址。
- 指定地址不存在时给出清楚错误，不生成无效配对码。
- 新测试先失败，再通过。
- 从 `src-tauri/` 跑过相关 Rust 测试和 CLI help 检查。

## 阶段

| 阶段 | 状态 | 内容 |
| --- | --- | --- |
| 1 | complete | 复核 CLI / app / infra 入口，确认最小接入点 |
| 2 | complete | 先写失败测试，覆盖地址选择和 ticket 限定 |
| 3 | complete | 实现 dev-only CLI 与底层地址选择 |
| 4 | complete | 运行测试、help 和编译检查 |
| 5 | complete | 汇总与归档 |

## 错误记录

| 错误 | 尝试 | 处理 |
| --- | --- | --- |
| `session-catchup.py` 不存在 | 使用 planning-with-files 推荐恢复脚本 | 当前安装目录没有脚本，继续使用现有计划文件 |
| `cargo test` 多个过滤名报 `unexpected argument` | 一次传入多个测试名 | 改用模块过滤重跑 |
